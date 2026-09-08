//! Incremental Claude Code / Codex CLI usage collector.
//!
//! Designed so antivirus realtime scanners barely see it:
//! - never spawns `claude` / `codex` / Node
//! - never spawns `claude` / `codex` / Node
//! - OAuth refresh rewrites `.credentials.json` only via `ReplaceFileW` after a
//!   reparse-point-safe open; it never follows credential symlinks
//! - `GetFileAttributesEx`-style metadata first; open a file only when size grew
//! - read only newly appended JSONL bytes, incomplete trailing lines left unread
//! - directory listings cached until the directory mtime moves
//! - work is budgeted per timer tick so the tray message loop stays idle

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::{self, File},
    hash::{Hash, Hasher},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Deserialize;
use windows_sys::Win32::{
    Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, HWND, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{
        CreateFileW, FlushFileBuffers, GetFileInformationByHandle, ReadFile, ReplaceFileW,
        WriteFile, BY_HANDLE_FILE_INFORMATION, CREATE_NEW, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        REPLACEFILE_WRITE_THROUGH,
    },
    System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION},
    UI::WindowsAndMessaging::PostMessageW,
};

use crate::core::{
    cancel_fetch, claude_dedupe_digest, cost_nanos, decide_codex_event, fetch_result_code,
    finish_fetch, is_long_context_request, local_ymd, parse_rfc3339_ms, persist_result_code,
    persist_result_detail, rebuild_reason_code, record_spawn_failure, reject_late_result,
    retain_keys_for_month, should_start_fetch, start_fetch, windows_tz_bias_minutes, ymd_iso,
    ymd_key, CheckpointSource, ClaudeDedupeKey, CodexTokenTotals, CodexUsageDecision, CursorKind,
    CursorRebuildReason, DiagnosticEvent, DiagnosticKind, DiagnosticRing, DiagnosticSnapshot,
    FetchErrorKind, FetchOutcome, FileCheckpointKey, LimitWindow, LoadStatus, PersistStatus,
    ProviderFetchKind, ProviderFetchState, ProviderUsage, RebuildState, RescanReason,
    RestoreResult, StartupMode, TokenUsage, UsageCursor, UsageSnapshot, UsageState,
    USAGE_STATE_SCHEMA_VERSION,
};

#[cfg(test)]
use crate::core::UsageCheckpoint;

use super::usage_store::FileUsageStore;

pub const USAGE_TIMER_ID: usize = 4;
pub use super::messages::USAGE_READY_MESSAGE;
/// One-shot delay before the first JSONL ingest after the tray starts.
pub const USAGE_FIRST_INTERVAL_MS: u32 = 8_000;
/// Steady-state JSONL discover / read / ingest interval.
pub const USAGE_IDLE_INTERVAL_MS: u32 = 60_000;
/// Next ingest when unread bytes remain. Same floor as idle — never a
/// sub-minute token poll (the old 400ms catch-up loop is gone).
pub const USAGE_CONTINUE_INTERVAL_MS: u32 = USAGE_IDLE_INTERVAL_MS;

const MAX_FILES_PER_TICK: usize = 3;
const MAX_STAT_PER_TICK: usize = 12;
const MAX_BYTES_PER_TICK: u64 = 96 * 1_024;
const MAX_DIRS_PER_TICK: usize = 6;
const CATCH_UP_FILES_PER_TICK: usize = 12;
const CATCH_UP_BYTES_PER_TICK: u64 = 512 * 1_024;
const CATCH_UP_DIRS_PER_TICK: usize = 24;
/// Assistant / token_count records are small. Larger lines are skipped so a
/// single 300KiB+ user blob cannot stall the jsonl cursor.
const MAX_PARSE_LINE: usize = 256 * 1_024;
const MAX_SKIP_PER_READ: u64 = 512 * 1_024;
const CODEX_LIMITS_TAIL: u64 = 256 * 1_024;
const CODEX_LIMITS_FILES: usize = 5;
const CLAUDE_LIMITS_PERIOD_MS: u64 = 5 * 60 * 1_000;
const CODEX_LIMITS_PERIOD_MS: u64 = 60 * 1_000;
/// Hot files are restatted at most this often. Kept ≥ idle so even an
/// early caller cannot read tokens more than once a minute.
const STAT_COOLDOWN_MS: u64 = USAGE_IDLE_INTERVAL_MS as u64;
/// Known files that are not hot are still restatted this often. Cached
/// mtime/size must not freeze a cursor forever.
const COLD_RESTAT_MS: u64 = 10 * 60 * 1_000;
const HOT_AGE_MS: u64 = 48 * 60 * 60 * 1_000;
const REDISCOVER_MS: u64 = USAGE_IDLE_INTERVAL_MS as u64;
const MAX_HEADER_VALUE_BYTES: usize = 8 * 1_024;
const MAX_CREDENTIAL_FILE_BYTES: usize = 256 * 1_024;
const CLAUDE_OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageTick {
    Idle,
    MoreWork,
}

struct FileCursor {
    size: u64,
    mtime_ms: u64,
    offset: u64,
    last_stat_ms: u64,
    last_model: Option<String>,
    last_codex_total: Option<CodexTokenTotals>,
    last_prefix: Option<u64>,
    last_file_id: Option<FileId>,
    waiting_incomplete: bool,
    kind: SourceKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
enum SourceKind {
    Claude,
    Codex,
}

struct DirListing {
    mtime_ms: u64,
    dirs: Vec<PathBuf>,
}

#[derive(Clone, Copy)]
struct DayWindow {
    bias_minutes: i32,
    today: u32,
    month_start: u32,
}

pub struct UsageCollector {
    claude_dir: PathBuf,
    codex_home: PathBuf,
    files: HashMap<PathBuf, FileCursor>,
    dirs: HashMap<PathBuf, DirListing>,
    discover: VecDeque<PathBuf>,
    claude_keys: HashSet<ClaudeDedupeKey>,
    pending: VecDeque<PathBuf>,
    snapshot: UsageSnapshot,
    month_key: u32,
    day_key: u32,
    last_collected_ms: u64,
    file_checkpoint: HashMap<FileCheckpointKey, UsageCursor>,
    last_discover_ms: u64,
    last_codex_limits_ms: u64,
    restat_skip: usize,
    catch_up: bool,
    deferred: VecDeque<PathBuf>,
    read_buf: Vec<u8>,
    codex_from_remote: bool,
    claude_fetch: ProviderRemoteSlot,
    codex_fetch: ProviderRemoteSlot,
    diagnostics: DiagnosticRing,
    startup_mode: StartupMode,
    dirs_enumerated: u64,
    files_stated: u64,
    files_opened: u64,
    usage_parse_bytes: u64,
    integrity_probe_bytes: u64,
    limits_tail_bytes: u64,
    cursor_reset_count: u64,
    cancelled: bool,
    checkpoint_dirty: bool,
    persist_checkpoint: bool,
    month_rescan_notify: bool,
    store: Option<FileUsageStore>,
    last_rebuild_reason: Option<CursorRebuildReason>,
}

struct SlotOutcome {
    generation: u64,
    usage: Option<ProviderUsage>,
    error: Option<FetchErrorKind>,
    observed_at_ms: u64,
}

struct ProviderRemoteSlot {
    in_flight: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    latest: Arc<Mutex<Option<SlotOutcome>>>,
    state: ProviderFetchState,
}

impl ProviderRemoteSlot {
    fn new() -> Self {
        Self {
            in_flight: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
            latest: Arc::new(Mutex::new(None)),
            state: ProviderFetchState::default(),
        }
    }

    fn take(&mut self) -> Option<SlotOutcome> {
        self.latest.lock().ok().and_then(|mut guard| guard.take())
    }

    fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
        cancel_fetch(&mut self.state);
        self.generation
            .store(self.state.request_generation, Ordering::SeqCst);
    }
}

impl UsageCollector {
    #[must_use]
    pub fn new() -> Self {
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap_or_default();
        let claude_dir = std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".claude"));
        let codex_home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        Self::with_dirs(claude_dir, codex_home, true)
    }

    #[must_use]
    fn with_dirs(claude_dir: PathBuf, codex_home: PathBuf, persist_checkpoint: bool) -> Self {
        let store = persist_checkpoint
            .then(FileUsageStore::production)
            .flatten();
        Self::with_dirs_and_store(claude_dir, codex_home, store)
    }

    #[must_use]
    fn with_dirs_and_store(
        claude_dir: PathBuf,
        codex_home: PathBuf,
        store: Option<FileUsageStore>,
    ) -> Self {
        let persist_checkpoint = store.is_some();
        let mut collector = Self {
            claude_dir,
            codex_home,
            files: HashMap::new(),
            dirs: HashMap::new(),
            discover: VecDeque::new(),
            claude_keys: HashSet::new(),
            pending: VecDeque::new(),
            snapshot: UsageSnapshot::default(),
            month_key: 0,
            day_key: 0,
            last_collected_ms: 0,
            file_checkpoint: HashMap::new(),
            last_discover_ms: 0,
            last_codex_limits_ms: 0,
            restat_skip: 0,
            catch_up: true,
            deferred: VecDeque::new(),
            read_buf: Vec::new(),
            codex_from_remote: false,
            claude_fetch: ProviderRemoteSlot::new(),
            codex_fetch: ProviderRemoteSlot::new(),
            diagnostics: DiagnosticRing::new(),
            startup_mode: StartupMode::Test,
            dirs_enumerated: 0,
            files_stated: 0,
            files_opened: 0,
            usage_parse_bytes: 0,
            integrity_probe_bytes: 0,
            limits_tail_bytes: 0,
            cursor_reset_count: 0,
            cancelled: false,
            checkpoint_dirty: false,
            persist_checkpoint,
            month_rescan_notify: false,
            store,
            last_rebuild_reason: None,
        };
        collector.startup_mode = if persist_checkpoint && collector.store_is_production() {
            StartupMode::Production
        } else {
            StartupMode::Test
        };
        collector.record_diag(
            DiagnosticKind::StartupMode,
            collector.startup_mode as u64,
            0,
        );
        collector.record_diag(
            DiagnosticKind::CheckpointSchema,
            u64::from(USAGE_STATE_SCHEMA_VERSION),
            0,
        );
        collector.record_diag(DiagnosticKind::ClaudeFetch, 0, 0);
        collector.record_diag(DiagnosticKind::CodexFetch, 0, 0);
        collector.record_diag(DiagnosticKind::RescanReason, RescanReason::None as u64, 0);
        if persist_checkpoint {
            collector.restore_checkpoint(day_window(unix_now_ms()));
        } else {
            collector.record_diag(
                DiagnosticKind::RestoreResult,
                RestoreResult::Missing as u64,
                0,
            );
            collector.record_diag(
                DiagnosticKind::CheckpointSource,
                CheckpointSource::Missing as u64,
                0,
            );
        }
        collector
    }

    pub fn flush_checkpoint(&mut self) {
        if !self.persist_checkpoint {
            return;
        }
        let window = day_window(unix_now_ms());
        self.persist_checkpoint_if_needed(window);
    }

    #[must_use]
    pub fn snapshot(&self) -> UsageSnapshot {
        UsageSnapshot {
            month_scan_in_progress: self.catch_up,
            ..self.snapshot
        }
    }

    #[must_use]
    pub fn take_month_rescan_finished(&mut self) -> bool {
        if self.month_rescan_notify && !self.catch_up {
            self.month_rescan_notify = false;
            return true;
        }
        false
    }

    pub fn take_claude_limits(&mut self) -> bool {
        let now_ms = unix_now_ms();
        let claude = self.claude_fetch.take();
        let codex = self.codex_fetch.take();
        let mut changed = false;
        if let Some(outcome) = claude {
            changed |= self.apply_remote_outcome(ProviderFetchKind::Claude, outcome, now_ms);
        }
        if let Some(outcome) = codex {
            changed |= self.apply_remote_outcome(ProviderFetchKind::Codex, outcome, now_ms);
        }
        changed
    }

    /// Reject in-flight results after shutdown. Generation/cancel is the
    /// contract; dropping the worker thread is not.
    pub fn cancel_remote_fetches(&mut self) {
        self.cancelled = true;
        self.claude_fetch.cancel();
        self.codex_fetch.cancel();
        self.record_diag(
            DiagnosticKind::ClaudeFetch,
            u64::from(FetchErrorKind::Cancelled.as_u32()),
            0,
        );
        self.record_diag(
            DiagnosticKind::CodexFetch,
            u64::from(FetchErrorKind::Cancelled.as_u32()),
            0,
        );
    }

    #[must_use]
    pub fn diagnostics(&self) -> DiagnosticSnapshot {
        let mut snapshot = DiagnosticSnapshot::from_ring(&self.diagnostics);
        snapshot.startup_mode = self.startup_mode as u64;
        snapshot.known_files = self.files.len() as u64;
        snapshot.dirs_enumerated = self.dirs_enumerated;
        snapshot.files_stated = self.files_stated;
        snapshot.files_opened = self.files_opened;
        snapshot.usage_parse_bytes = self.usage_parse_bytes;
        snapshot.integrity_probe_bytes = self.integrity_probe_bytes;
        snapshot.limits_tail_bytes = self.limits_tail_bytes;
        snapshot.cursor_reset_count = self.cursor_reset_count;
        if let Some(reason) = self.last_rebuild_reason {
            snapshot.cursor_reset_reason = rebuild_reason_code(reason);
        }
        snapshot.catch_up = u64::from(self.catch_up);
        snapshot.rebuild_state = if self.month_rescan_notify {
            RebuildState::Rescan as u64
        } else if self.catch_up {
            RebuildState::CatchUp as u64
        } else {
            RebuildState::Idle as u64
        };
        snapshot.pending_files = self.pending.len() as u64;
        snapshot.oldest_pending_age_ms = oldest_pending_age_ms(&self.pending, &self.files);
        snapshot.claude_fetch = fetch_result_code(
            self.claude_fetch.state.last_error,
            self.claude_fetch.state.freshness,
        );
        snapshot.codex_fetch = fetch_result_code(
            self.codex_fetch.state.last_error,
            self.codex_fetch.state.freshness,
        );
        snapshot
    }

    #[must_use]
    pub fn tick(&mut self, hwnd: HWND) -> UsageTick {
        let now_ms = unix_now_ms();
        let window = day_window(now_ms);
        self.reset_if_month_changed(window);
        self.reset_if_day_changed(window);
        let _ = self.take_claude_limits();

        if self.last_discover_ms == 0
            || (!self.catch_up && now_ms.saturating_sub(self.last_discover_ms) >= REDISCOVER_MS)
        {
            self.queue_roots();
            self.last_discover_ms = now_ms;
        }

        let mut dirs = 0;
        let dir_budget = self.dir_budget();
        while dirs < dir_budget {
            let Some(dir) = self.discover.pop_front() else {
                break;
            };
            self.discover_dir(&dir, window, now_ms);
            dirs += 1;
        }
        if self.catch_up {
            self.prioritize_newest_pending();
        }

        let (opens, unread_remaining) = self.scan_pending_and_hot(window, now_ms);
        if !self.codex_from_remote {
            let has_codex = self
                .files
                .values()
                .any(|cursor| cursor.kind == SourceKind::Codex);
            if has_codex
                && (self.snapshot.codex.primary.is_none()
                    || now_ms.saturating_sub(self.last_codex_limits_ms) >= CODEX_LIMITS_PERIOD_MS)
            {
                self.apply_codex_limits(now_ms);
                self.last_codex_limits_ms = now_ms;
            }
        }
        self.maybe_fetch_remote_limits(hwnd, now_ms);
        self.record_tick_gauges();

        if self.catch_up && self.discover.is_empty() && !unread_remaining {
            self.finish_catch_up(window);
        }

        let tick = if self.discover.is_empty() && !unread_remaining && opens < self.file_budget() {
            self.release_scratch();
            UsageTick::Idle
        } else {
            UsageTick::MoreWork
        };
        // Persist after this ingest tick when totals/cursors changed.
        // Mid-catch-up crash recovery needs that write, not a 400ms timer.
        if self.checkpoint_dirty {
            self.persist_checkpoint_if_needed(window);
        }
        tick
    }

    fn restore_checkpoint(&mut self, window: DayWindow) {
        self.record_diag(
            DiagnosticKind::CheckpointSchema,
            u64::from(USAGE_STATE_SCHEMA_VERSION),
            0,
        );
        if let Some(store) = self.store.as_ref() {
            if store.refuses_provider_roots(&self.claude_dir, &self.codex_home) {
                self.store = None;
                self.persist_checkpoint = false;
                self.record_diag(
                    DiagnosticKind::RestoreResult,
                    RestoreResult::Refused as u64,
                    0,
                );
                self.record_diag(
                    DiagnosticKind::CheckpointSource,
                    CheckpointSource::Missing as u64,
                    0,
                );
                return;
            }
            match store.load() {
                LoadStatus::Loaded(state) => {
                    let bytes = state.encode().len() as u64;
                    self.record_diag(
                        DiagnosticKind::CheckpointSource,
                        CheckpointSource::FileStore as u64,
                        0,
                    );
                    self.record_diag(
                        DiagnosticKind::RestoreResult,
                        RestoreResult::Loaded as u64,
                        0,
                    );
                    self.record_diag(DiagnosticKind::CheckpointBytes, bytes, 0);
                    self.apply_state(window, state);
                    return;
                }
                LoadStatus::RecoveredPrior { state, .. } => {
                    let bytes = state.encode().len() as u64;
                    self.record_diag(
                        DiagnosticKind::CheckpointSource,
                        CheckpointSource::RecoveredPrior as u64,
                        0,
                    );
                    self.record_diag(
                        DiagnosticKind::RestoreResult,
                        RestoreResult::Recovered as u64,
                        0,
                    );
                    self.record_diag(DiagnosticKind::CheckpointBytes, bytes, 0);
                    self.apply_state(window, state);
                    return;
                }
                LoadStatus::Missing => {}
            }
        }
        if !self.store_is_production() {
            self.record_diag(
                DiagnosticKind::RestoreResult,
                RestoreResult::Missing as u64,
                0,
            );
            self.record_diag(
                DiagnosticKind::CheckpointSource,
                CheckpointSource::Missing as u64,
                0,
            );
            return;
        }
        let Some(checkpoint) = super::registry::load_usage_checkpoint() else {
            self.record_diag(
                DiagnosticKind::RestoreResult,
                RestoreResult::Missing as u64,
                0,
            );
            self.record_diag(
                DiagnosticKind::CheckpointSource,
                CheckpointSource::Missing as u64,
                0,
            );
            return;
        };
        let migrated = UsageState::from_registry_checkpoint(&checkpoint, 0);
        self.record_diag(
            DiagnosticKind::CheckpointSource,
            CheckpointSource::Registry as u64,
            0,
        );
        self.record_diag(
            DiagnosticKind::RestoreResult,
            RestoreResult::Loaded as u64,
            0,
        );
        self.record_diag(
            DiagnosticKind::CheckpointBytes,
            migrated.encode().len() as u64,
            0,
        );
        self.apply_state(window, migrated);
        self.checkpoint_dirty = true;
        self.persist_checkpoint_if_needed(window);
    }

    fn store_is_production(&self) -> bool {
        match (self.store.as_ref(), FileUsageStore::production_root()) {
            (Some(store), Some(production)) => store.root() == production,
            _ => false,
        }
    }

    #[cfg(test)]
    fn apply_checkpoint(&mut self, window: DayWindow, checkpoint: UsageCheckpoint) {
        self.apply_state(window, UsageState::from_registry_checkpoint(&checkpoint, 0));
    }

    fn apply_state(&mut self, window: DayWindow, state: UsageState) {
        self.snapshot = state.aggregate.snapshot;
        self.snapshot.month_scan_in_progress = !state.aggregate.catch_up_done;
        if state.aggregate.month_start != window.month_start {
            self.snapshot.claude.clear_month_cost();
            self.snapshot.claude.month_input_tokens = 0;
            self.snapshot.claude.month_output_tokens = 0;
            self.snapshot.codex.clear_month_cost();
            self.snapshot.codex.month_input_tokens = 0;
            self.snapshot.codex.month_output_tokens = 0;
        }
        if state.aggregate.today != window.today {
            self.snapshot.claude.clear_today_cost();
            self.snapshot.codex.clear_today_cost();
        }
        self.last_collected_ms = state.aggregate.last_collected_ms;
        self.month_key = window.month_start;
        self.day_key = window.today;
        self.file_checkpoint = state
            .cursors
            .into_iter()
            .map(|cursor| {
                let key = match cursor.kind {
                    CursorKind::Claude => FileCheckpointKey::Claude(cursor.logical_id.clone()),
                    CursorKind::Codex => FileCheckpointKey::Codex(cursor.logical_id.clone()),
                };
                (key, cursor)
            })
            .collect();
        self.claude_keys = state.claude_keys;
        self.catch_up = !state.aggregate.catch_up_done;
        self.last_discover_ms = 0;
    }

    fn build_state(&self, window: DayWindow) -> UsageState {
        let mut cursors = Vec::new();
        let mut seen = HashSet::new();
        for (path, cursor) in &self.files {
            let Some(logical_id) =
                file_logical_id(path, &self.claude_dir, &self.codex_home, cursor.kind)
            else {
                continue;
            };
            seen.insert((cursor.kind, logical_id.clone()));
            cursors.push(UsageCursor {
                kind: match cursor.kind {
                    SourceKind::Claude => CursorKind::Claude,
                    SourceKind::Codex => CursorKind::Codex,
                },
                logical_id,
                offset: cursor.offset,
                size: cursor.size,
                prefix: cursor.last_prefix,
                last_model: cursor.last_model.clone(),
                last_codex_total: cursor.last_codex_total,
            });
        }
        for (key, cursor) in &self.file_checkpoint {
            let kind = match key {
                FileCheckpointKey::Claude(_) => SourceKind::Claude,
                FileCheckpointKey::Codex(_) => SourceKind::Codex,
            };
            if seen.contains(&(kind, cursor.logical_id.clone())) {
                continue;
            }
            cursors.push(cursor.clone());
        }
        UsageState {
            generation: 0,
            schema_version: crate::core::USAGE_STATE_SCHEMA_VERSION,
            aggregate: crate::core::UsageAggregate {
                month_start: window.month_start,
                today: window.today,
                last_collected_ms: self.last_collected_ms,
                catch_up_done: !self.catch_up,
                snapshot: UsageSnapshot {
                    claude: self.snapshot.claude,
                    codex: self.snapshot.codex,
                    month_scan_in_progress: false,
                },
            },
            cursors,
            claude_keys: self.claude_keys.clone(),
        }
    }

    fn persist_checkpoint_if_needed(&mut self, window: DayWindow) {
        if !self.persist_checkpoint || !self.checkpoint_dirty {
            return;
        }
        if self
            .store
            .as_ref()
            .is_some_and(|store| store.refuses_provider_roots(&self.claude_dir, &self.codex_home))
        {
            return;
        }
        let state = self.build_state(window);
        let write_bytes = state.encode().len() as u64;
        let Some(store) = self.store.as_mut() else {
            return;
        };
        let status = store.persist(&state);
        self.record_diag(DiagnosticKind::CheckpointWriteBytes, write_bytes, 0);
        self.record_diag(
            DiagnosticKind::CheckpointSaveResult,
            persist_result_code(status),
            persist_result_detail(status),
        );
        match status {
            PersistStatus::Applied { .. } => {
                self.file_checkpoint = state
                    .cursors
                    .into_iter()
                    .map(|cursor| {
                        let key = match cursor.kind {
                            CursorKind::Claude => {
                                FileCheckpointKey::Claude(cursor.logical_id.clone())
                            }
                            CursorKind::Codex => {
                                FileCheckpointKey::Codex(cursor.logical_id.clone())
                            }
                        };
                        (key, cursor)
                    })
                    .collect();
                self.checkpoint_dirty = false;
            }
            PersistStatus::Failed => {}
        }
    }

    fn reset_if_day_changed(&mut self, window: DayWindow) {
        if self.day_key == 0 {
            self.day_key = window.today;
            return;
        }
        if self.day_key == window.today {
            return;
        }
        self.day_key = window.today;
        self.snapshot.claude.clear_today_cost();
        self.snapshot.codex.clear_today_cost();
        self.checkpoint_dirty = true;
    }

    fn reset_if_month_changed(&mut self, window: DayWindow) {
        if self.month_key == 0 {
            self.month_key = window.month_start;
            return;
        }
        if self.month_key == window.month_start {
            return;
        }
        self.month_key = window.month_start;
        self.snapshot.claude.clear_today_cost();
        self.snapshot.claude.clear_month_cost();
        self.snapshot.claude.month_input_tokens = 0;
        self.snapshot.claude.month_output_tokens = 0;
        self.snapshot.codex.clear_today_cost();
        self.snapshot.codex.clear_month_cost();
        self.snapshot.codex.month_input_tokens = 0;
        self.snapshot.codex.month_output_tokens = 0;
        self.claude_keys = retain_keys_for_month(&self.claude_keys, window.month_start);
        self.checkpoint_dirty = true;
    }

    /// Clears the durable checkpoint and rescans every JSONL file for the
    /// current month. Use when Month totals look stale after a bad checkpoint.
    pub fn rescan_current_month(&mut self) {
        let window = day_window(unix_now_ms());
        if self.month_key == 0 {
            self.month_key = window.month_start;
        }
        self.month_rescan_notify = true;
        self.record_diag(DiagnosticKind::RescanReason, RescanReason::User as u64, 0);
        self.begin_month_rescan(window);
    }

    fn begin_month_rescan(&mut self, window: DayWindow) {
        self.day_key = window.today;
        self.last_collected_ms = 0;
        self.file_checkpoint.clear();
        self.files.clear();
        self.dirs.clear();
        self.claude_keys.clear();
        self.pending.clear();
        self.deferred.clear();
        self.catch_up = true;
        self.snapshot.claude.clear_today_cost();
        self.snapshot.claude.clear_month_cost();
        self.snapshot.claude.month_input_tokens = 0;
        self.snapshot.claude.month_output_tokens = 0;
        self.snapshot.codex.clear_today_cost();
        self.snapshot.codex.clear_month_cost();
        self.snapshot.codex.month_input_tokens = 0;
        self.snapshot.codex.month_output_tokens = 0;
        self.checkpoint_dirty = true;
        self.last_discover_ms = 0;
        self.queue_roots();
    }

    fn queue_roots(&mut self) {
        self.discover.clear();
        self.discover.push_back(self.claude_dir.join("projects"));
        let now_ms = unix_now_ms();
        let window = day_window(now_ms);
        let (year, month, _) = local_ymd(now_ms, window.bias_minutes);
        self.discover
            .push_back(codex_month_dir(&self.codex_home, year, month));
        let (prev_year, prev_month) = previous_month(year, month);
        self.discover
            .push_back(codex_month_dir(&self.codex_home, prev_year, prev_month));
        queue_recent_codex_month_dirs(
            &mut self.discover,
            &self.codex_home,
            year,
            now_ms,
            HOT_AGE_MS,
        );
        self.reconcile_known_paths(window);
    }

    fn discover_dir(&mut self, dir: &Path, window: DayWindow, _now_ms: u64) {
        let mtime_ms = path_mtime_ms(dir).unwrap_or(0);
        if let Some(cached) = self.dirs.get(dir) {
            if cached.mtime_ms == mtime_ms {
                for child in &cached.dirs {
                    self.discover.push_back(child.clone());
                }
                return;
            }
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        self.dirs_enumerated = self.dirs_enumerated.saturating_add(1);
        let mut files = Vec::new();
        let mut dirs = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                if path_is_under(&path, &self.claude_dir) || path_is_under(&path, &self.codex_home)
                {
                    dirs.push(path);
                }
            } else if file_type.is_file() && is_jsonl(&path) {
                files.push(path);
            }
        }
        for child in &dirs {
            self.discover.push_back(child.clone());
        }
        for path in &files {
            self.register_jsonl_file(path, window, true);
        }
        self.dirs
            .insert(dir.to_path_buf(), DirListing { mtime_ms, dirs });
    }

    fn reconcile_known_paths(&mut self, window: DayWindow) {
        let keys: Vec<FileCheckpointKey> = self.file_checkpoint.keys().cloned().collect();
        for key in keys {
            let path = checkpoint_disk_path(&self.claude_dir, &self.codex_home, &key);
            if self.files.contains_key(&path) {
                continue;
            }
            self.register_jsonl_file(&path, window, false);
        }
    }

    fn register_jsonl_file(&mut self, path: &Path, window: DayWindow, require_recent_mtime: bool) {
        let Some((size, mtime_ms)) = path_size_mtime(path) else {
            return;
        };
        self.files_stated = self.files_stated.saturating_add(1);
        if require_recent_mtime
            && ymd_key_from_unix(mtime_ms, window.bias_minutes) < previous_month_start(window)
        {
            return;
        }
        let kind = if path_is_under(path, &self.codex_home) {
            SourceKind::Codex
        } else if path_is_under(path, &self.claude_dir) {
            SourceKind::Claude
        } else {
            return;
        };
        let mut registered_rebuild = None;
        if let std::collections::hash_map::Entry::Vacant(entry) =
            self.files.entry(path.to_path_buf())
        {
            let prefix = file_prefix_fingerprint(path);
            let file_id = jsonl_file_id(path);
            let (offset, rebuild) = restored_cursor_offset(
                &self.file_checkpoint,
                path,
                &self.claude_dir,
                &self.codex_home,
                kind,
                size,
                prefix,
            );
            if let Some(reason) = rebuild {
                self.last_rebuild_reason = Some(reason);
                self.cursor_reset_count = self.cursor_reset_count.saturating_add(1);
                registered_rebuild = Some(reason);
                self.checkpoint_dirty = true;
            }
            let stored = file_checkpoint_key(path, &self.claude_dir, &self.codex_home, kind)
                .and_then(|key| self.file_checkpoint.get(&key));
            let (last_model, last_codex_total) = if rebuild.is_some() {
                (None, None)
            } else {
                (
                    stored.and_then(|cursor| cursor.last_model.clone()),
                    stored.and_then(|cursor| cursor.last_codex_total),
                )
            };
            entry.insert(FileCursor {
                size,
                mtime_ms,
                offset,
                last_stat_ms: 0,
                last_model,
                last_codex_total,
                last_prefix: prefix,
                last_file_id: file_id,
                waiting_incomplete: false,
                kind,
            });
            if size > offset {
                self.enqueue_scan(path.to_path_buf(), mtime_ms, window);
            }
        }
        if let Some(reason) = registered_rebuild {
            self.record_diag(DiagnosticKind::CursorResetCount, self.cursor_reset_count, 0);
            self.record_diag(
                DiagnosticKind::CursorResetReason,
                rebuild_reason_code(reason),
                0,
            );
        }
    }

    fn scan_file(&mut self, path: &Path, window: DayWindow, now_ms: u64) -> u64 {
        let Some((size, mtime_ms)) = path_size_mtime(path) else {
            return 0;
        };
        self.files_stated = self.files_stated.saturating_add(1);
        let max_bytes = self.byte_budget();
        let prefix = file_prefix_fingerprint(path);
        let file_id = jsonl_file_id(path);
        let (kind, offset, previous_model, rebuild, size_matches_offset) = {
            let Some(cursor) = self.files.get_mut(path) else {
                return 0;
            };
            cursor.last_stat_ms = now_ms;
            let rebuild = cursor_rebuild_reason(cursor, size, mtime_ms, prefix, file_id);
            if rebuild.is_some() {
                cursor.offset = 0;
                cursor.last_model = None;
                cursor.last_codex_total = None;
                cursor.waiting_incomplete = false;
            }
            cursor.last_prefix = prefix;
            cursor.last_file_id = file_id;
            let kind = cursor.kind;
            let offset = cursor.offset;
            let previous_model = cursor.last_model.clone();
            if size == cursor.offset {
                cursor.size = size;
                cursor.mtime_ms = mtime_ms;
                cursor.waiting_incomplete = false;
            }
            (kind, offset, previous_model, rebuild, size == cursor.offset)
        };
        if let Some(reason) = rebuild {
            self.last_rebuild_reason = Some(reason);
            self.cursor_reset_count = self.cursor_reset_count.saturating_add(1);
            self.record_diag(DiagnosticKind::CursorResetCount, self.cursor_reset_count, 0);
            self.record_diag(
                DiagnosticKind::CursorResetReason,
                rebuild_reason_code(reason),
                0,
            );
            self.checkpoint_dirty = true;
        }
        if size_matches_offset {
            return 0;
        }
        self.integrity_probe_bytes = self.integrity_probe_bytes.saturating_add(64);
        self.files_opened = self.files_opened.saturating_add(1);
        let chunk = read_appended(
            path,
            offset,
            size,
            kind,
            previous_model.as_deref(),
            max_bytes,
            &mut self.read_buf,
        );
        self.usage_parse_bytes = self.usage_parse_bytes.saturating_add(chunk.consumed);
        let Some(cursor) = self.files.get_mut(path) else {
            return chunk.consumed;
        };
        let previous_model = cursor.last_model.clone();
        let mut previous_codex_total = cursor.last_codex_total;
        cursor.offset = chunk.new_offset;
        cursor.size = size;
        cursor.mtime_ms = mtime_ms;
        cursor.last_model = chunk.last_model;
        cursor.waiting_incomplete =
            cursor.size > cursor.offset && chunk.consumed == 0 && chunk.new_offset == offset;
        if cursor.last_model != previous_model {
            self.checkpoint_dirty = true;
        }
        if let Some(limits) = chunk.limits {
            if kind == SourceKind::Codex
                && !self.codex_from_remote
                && is_subscription_limits(&limits)
            {
                apply_provider_limits(&mut self.snapshot.codex, limits);
            }
        }
        // Count every in-month event. A global timestamp watermark would drop
        // older JSONL after a newer file is scanned first (Codex catch-up).
        for (mut event, digest) in chunk.events.into_iter().zip(chunk.dedupe) {
            let day = ymd_key_from_unix(event.timestamp_ms, window.bias_minutes);
            let (year, month, day_of_month) = local_ymd(event.timestamp_ms, window.bias_minutes);
            if let Some(digest) = digest {
                let key = ClaudeDedupeKey {
                    digest,
                    month: ymd_key(year, month, 1),
                };
                if !self.claude_keys.insert(key) {
                    continue;
                }
            }
            if kind == SourceKind::Codex {
                if let Some(last) = event.codex_last {
                    let (decision, next) =
                        decide_codex_event(previous_codex_total, last, event.codex_total);
                    if next != previous_codex_total {
                        self.checkpoint_dirty = true;
                    }
                    previous_codex_total = next;
                    match decision {
                        CodexUsageDecision::Count(mut usage) => {
                            if is_long_context_request(&event.model, last.input) {
                                usage.long_context_input = usage.input;
                                usage.long_context_cached_input = usage.cached_input;
                                usage.long_context_output = usage.output;
                            }
                            event.usage = usage;
                        }
                        CodexUsageDecision::IgnoreReplay
                        | CodexUsageDecision::IgnoreInheritedBaseline => continue,
                    }
                }
            }
            let day_iso = ymd_iso(year, month, day_of_month);
            // Unknown models have no API-equivalent dollars. Still count tokens
            // so month activity is visible even when Today cannot be priced.
            let nanos = cost_nanos(&event.model, event.usage, Some(&day_iso)).unwrap_or(0);
            let target = match kind {
                SourceKind::Claude => &mut self.snapshot.claude,
                SourceKind::Codex => &mut self.snapshot.codex,
            };
            if day == window.today {
                target.add_today_nanos(nanos);
            }
            if day >= window.month_start {
                target.add_month_nanos(nanos);
                target.month_input_tokens = target
                    .month_input_tokens
                    .saturating_add(event.usage.processed_input_tokens());
                target.month_output_tokens = target
                    .month_output_tokens
                    .saturating_add(event.usage.processed_output_tokens());
            }
            self.last_collected_ms = self.last_collected_ms.max(event.timestamp_ms);
            self.checkpoint_dirty = true;
        }
        if let Some(cursor) = self.files.get_mut(path) {
            cursor.last_codex_total = previous_codex_total;
        }
        chunk.consumed
    }

    fn scan_pending_and_hot(&mut self, window: DayWindow, now_ms: u64) -> (usize, bool) {
        let mut bytes = 0_u64;
        let mut opens = 0_usize;
        let file_budget = self.file_budget();
        let byte_budget = self.byte_budget();
        let mut queued = self.pending.len();
        while queued > 0 && opens < file_budget && bytes < byte_budget {
            queued -= 1;
            let Some(path) = self.pending.pop_front() else {
                break;
            };
            let consumed = self.scan_file(&path, window, now_ms);
            if consumed > 0 {
                opens += 1;
                bytes += consumed;
            }
            if path_size_mtime(&path).is_some()
                && self.files.get(&path).is_some_and(|cursor| {
                    cursor.size != cursor.offset && !cursor.waiting_incomplete
                })
            {
                self.pending.push_back(path);
            }
        }

        if !self.catch_up {
            let mut skipped = 0_usize;
            let mut stated = 0_usize;
            let mut restat = Vec::new();
            for (path, cursor) in &self.files {
                if cursor.size != cursor.offset && !cursor.waiting_incomplete {
                    continue;
                }
                let cooldown = if is_hot(cursor, now_ms) {
                    STAT_COOLDOWN_MS
                } else {
                    COLD_RESTAT_MS
                };
                if now_ms.saturating_sub(cursor.last_stat_ms) < cooldown {
                    continue;
                }
                if skipped < self.restat_skip {
                    skipped += 1;
                    continue;
                }
                if stated >= MAX_STAT_PER_TICK || opens >= file_budget {
                    break;
                }
                restat.push(path.clone());
                stated += 1;
            }
            self.restat_skip = if stated < MAX_STAT_PER_TICK {
                0
            } else {
                self.restat_skip.saturating_add(stated)
            };

            for path in restat {
                if opens >= file_budget || bytes >= byte_budget {
                    break;
                }
                let consumed = self.scan_file(&path, window, now_ms);
                if consumed > 0 {
                    opens += 1;
                    bytes += consumed;
                    if self.files.get(&path).is_some_and(|cursor| {
                        cursor.size != cursor.offset && !cursor.waiting_incomplete
                    }) {
                        self.pending.push_back(path);
                    }
                }
            }
        }

        let unread_remaining = self.pending.iter().any(|path| {
            self.files
                .get(path)
                .is_some_and(|cursor| cursor.size != cursor.offset && !cursor.waiting_incomplete)
        });
        (opens, unread_remaining)
    }

    fn enqueue_scan(&mut self, path: PathBuf, mtime_ms: u64, window: DayWindow) {
        if self.catch_up && !is_current_month(mtime_ms, window) {
            self.deferred.push_back(path);
            return;
        }
        self.pending.push_back(path);
    }

    fn prioritize_newest_pending(&mut self) {
        let mut items: Vec<PathBuf> = self.pending.drain(..).collect();
        items.sort_by_key(|path| {
            std::cmp::Reverse(
                self.files
                    .get(path)
                    .map(|cursor| cursor.mtime_ms)
                    .unwrap_or(0),
            )
        });
        self.pending.extend(items);
    }

    fn finish_catch_up(&mut self, window: DayWindow) {
        self.catch_up = false;
        self.checkpoint_dirty = true;
        self.pending.extend(self.deferred.drain(..));
        self.persist_checkpoint_if_needed(window);
    }

    fn release_scratch(&mut self) {
        self.read_buf.clear();
        self.read_buf.shrink_to(0);
    }

    fn file_budget(&self) -> usize {
        if self.catch_up {
            CATCH_UP_FILES_PER_TICK
        } else {
            MAX_FILES_PER_TICK
        }
    }

    fn byte_budget(&self) -> u64 {
        if self.catch_up {
            CATCH_UP_BYTES_PER_TICK
        } else {
            MAX_BYTES_PER_TICK
        }
    }

    fn dir_budget(&self) -> usize {
        if self.catch_up {
            CATCH_UP_DIRS_PER_TICK
        } else {
            MAX_DIRS_PER_TICK
        }
    }

    fn apply_codex_limits(&mut self, now_ms: u64) {
        let mut newest: Vec<(&PathBuf, u64, u64)> = self
            .files
            .iter()
            .filter(|(_, cursor)| cursor.kind == SourceKind::Codex)
            .map(|(path, cursor)| (path, cursor.size, cursor.mtime_ms))
            .collect();
        newest.sort_by_key(|item| std::cmp::Reverse(item.2));
        for (path, size, mtime_ms) in newest.into_iter().take(CODEX_LIMITS_FILES) {
            if now_ms.saturating_sub(mtime_ms) > 7 * 86_400_000 {
                continue;
            }
            self.limits_tail_bytes = self
                .limits_tail_bytes
                .saturating_add(size.min(CODEX_LIMITS_TAIL));
            self.files_opened = self.files_opened.saturating_add(1);
            if let Some(limits) = read_codex_limits_tail(path, size) {
                if is_subscription_limits(&limits) {
                    self.snapshot.codex.primary = limits.primary;
                    self.snapshot.codex.secondary = limits.secondary;
                    if limits.plan_len != 0 {
                        self.snapshot.codex.plan = limits.plan;
                        self.snapshot.codex.plan_len = limits.plan_len;
                    }
                    break;
                }
            }
        }
    }

    fn maybe_fetch_remote_limits(&mut self, hwnd: HWND, now_ms: u64) {
        if self.cancelled || hwnd.is_null() {
            return;
        }
        let claude_dir = self.claude_dir.clone();
        self.spawn_provider_fetch(
            ProviderFetchKind::Claude,
            hwnd,
            now_ms,
            CLAUDE_LIMITS_PERIOD_MS,
            "run-dog-claude-limits",
            move || fetch_claude_limits(&claude_dir),
        );
        let codex_home = self.codex_home.clone();
        self.spawn_provider_fetch(
            ProviderFetchKind::Codex,
            hwnd,
            now_ms,
            CODEX_LIMITS_PERIOD_MS,
            "run-dog-codex-limits",
            move || fetch_codex_wham_limits(&codex_home),
        );
    }

    fn spawn_provider_fetch<F>(
        &mut self,
        kind: ProviderFetchKind,
        hwnd: HWND,
        now_ms: u64,
        period_ms: u64,
        name: &str,
        fetch: F,
    ) where
        F: FnOnce() -> Result<ProviderUsage, FetchErrorKind> + Send + 'static,
    {
        let spawn_failed = {
            let slot = match kind {
                ProviderFetchKind::Claude => &mut self.claude_fetch,
                ProviderFetchKind::Codex => &mut self.codex_fetch,
            };
            if slot.cancelled.load(Ordering::SeqCst) {
                return;
            }
            if !should_start_fetch(&slot.state, now_ms, period_ms) {
                return;
            }
            if slot
                .in_flight
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                return;
            }
            let generation = start_fetch(&mut slot.state, now_ms);
            slot.generation.store(generation, Ordering::SeqCst);
            let latest = Arc::clone(&slot.latest);
            let in_flight = Arc::clone(&slot.in_flight);
            let cancelled = Arc::clone(&slot.cancelled);
            let hwnd_bits = hwnd as isize;
            let worker = thread::Builder::new().name(name.to_owned()).spawn(move || {
                let observed_at_ms = unix_now_ms();
                let (usage, error) = if cancelled.load(Ordering::SeqCst) {
                    (None, Some(FetchErrorKind::Cancelled))
                } else {
                    match fetch() {
                        Ok(usage) => (Some(usage), None),
                        Err(error) => (None, Some(error)),
                    }
                };
                if let Ok(mut guard) = latest.lock() {
                    *guard = Some(SlotOutcome {
                        generation,
                        usage,
                        error,
                        observed_at_ms,
                    });
                }
                in_flight.store(false, Ordering::SeqCst);
                let _ = unsafe { PostMessageW(hwnd_bits as HWND, USAGE_READY_MESSAGE, 0, 0) };
            });
            if worker.is_err() {
                slot.in_flight.store(false, Ordering::SeqCst);
                record_spawn_failure(&mut slot.state, now_ms);
                true
            } else {
                false
            }
        };
        if spawn_failed {
            self.record_diag(
                fetch_kind_diag(kind),
                u64::from(FetchErrorKind::SpawnFailed.as_u32()),
                0,
            );
        }
    }

    fn apply_remote_outcome(
        &mut self,
        kind: ProviderFetchKind,
        outcome: SlotOutcome,
        now_ms: u64,
    ) -> bool {
        let cancelled = self.cancelled
            || match kind {
                ProviderFetchKind::Claude => self.claude_fetch.cancelled.load(Ordering::SeqCst),
                ProviderFetchKind::Codex => self.codex_fetch.cancelled.load(Ordering::SeqCst),
            };
        let started_generation = match kind {
            ProviderFetchKind::Claude => self.claude_fetch.state.request_generation,
            ProviderFetchKind::Codex => self.codex_fetch.state.request_generation,
        };
        if reject_late_result(cancelled, started_generation, outcome.generation) {
            match kind {
                ProviderFetchKind::Claude => self.claude_fetch.state.in_flight = false,
                ProviderFetchKind::Codex => self.codex_fetch.state.in_flight = false,
            }
            self.record_diag(
                fetch_kind_diag(kind),
                u64::from(FetchErrorKind::GenerationMismatch.as_u32()),
                0,
            );
            return false;
        }
        let reset_at_ms = outcome.usage.as_ref().map(provider_reset_at).unwrap_or(0);
        let fetch_outcome = FetchOutcome {
            generation: outcome.generation,
            succeeded: outcome.usage.is_some(),
            error: outcome.error,
            observed_at_ms: outcome.observed_at_ms,
            reset_at_ms,
        };
        let fetch_code = {
            let slot = match kind {
                ProviderFetchKind::Claude => &mut self.claude_fetch,
                ProviderFetchKind::Codex => &mut self.codex_fetch,
            };
            finish_fetch(&mut slot.state, now_ms, &fetch_outcome);
            fetch_result_code(slot.state.last_error, slot.state.freshness)
        };
        self.record_diag(
            fetch_kind_diag(kind),
            fetch_code,
            outcome.error.map(FetchErrorKind::as_u32).unwrap_or(0),
        );
        let Some(limits) = outcome.usage else {
            return false;
        };
        match kind {
            ProviderFetchKind::Claude => apply_provider_limits(&mut self.snapshot.claude, limits),
            ProviderFetchKind::Codex => {
                apply_provider_limits(&mut self.snapshot.codex, limits);
                self.codex_from_remote = true;
            }
        }
        true
    }

    fn record_diag(&mut self, kind: DiagnosticKind, value: u64, detail: u32) {
        self.diagnostics
            .push(DiagnosticEvent::new(kind, value, detail, unix_now_ms()));
    }

    fn record_tick_gauges(&mut self) {
        let snapshot = self.diagnostics();
        self.record_diag(DiagnosticKind::KnownFiles, snapshot.known_files, 0);
        self.record_diag(DiagnosticKind::DirsEnumerated, snapshot.dirs_enumerated, 0);
        self.record_diag(DiagnosticKind::FilesStated, snapshot.files_stated, 0);
        self.record_diag(DiagnosticKind::FilesOpened, snapshot.files_opened, 0);
        self.record_diag(
            DiagnosticKind::UsageParseBytes,
            snapshot.usage_parse_bytes,
            0,
        );
        self.record_diag(
            DiagnosticKind::IntegrityProbeBytes,
            snapshot.integrity_probe_bytes,
            0,
        );
        self.record_diag(
            DiagnosticKind::LimitsTailBytes,
            snapshot.limits_tail_bytes,
            0,
        );
        self.record_diag(
            DiagnosticKind::CursorResetCount,
            snapshot.cursor_reset_count,
            0,
        );
        self.record_diag(DiagnosticKind::CatchUpState, snapshot.catch_up, 0);
        self.record_diag(DiagnosticKind::RebuildState, snapshot.rebuild_state, 0);
        self.record_diag(DiagnosticKind::PendingFiles, snapshot.pending_files, 0);
        self.record_diag(
            DiagnosticKind::OldestPendingAge,
            snapshot.oldest_pending_age_ms,
            0,
        );
    }
}

fn fetch_kind_diag(kind: ProviderFetchKind) -> DiagnosticKind {
    match kind {
        ProviderFetchKind::Claude => DiagnosticKind::ClaudeFetch,
        ProviderFetchKind::Codex => DiagnosticKind::CodexFetch,
    }
}

fn provider_reset_at(usage: &ProviderUsage) -> u64 {
    usage
        .primary
        .or(usage.secondary)
        .or(usage.fable)
        .map(|window| window.resets_at_ms)
        .unwrap_or(0)
}

fn oldest_pending_age_ms(pending: &VecDeque<PathBuf>, files: &HashMap<PathBuf, FileCursor>) -> u64 {
    let now = unix_now_ms();
    pending
        .iter()
        .filter_map(|path| files.get(path).map(|cursor| cursor.mtime_ms))
        .min()
        .map(|mtime| now.saturating_sub(mtime))
        .unwrap_or(0)
}

struct ParsedEvent {
    model: String,
    timestamp_ms: u64,
    usage: TokenUsage,
    codex_last: Option<CodexTokenTotals>,
    codex_total: Option<CodexTokenTotals>,
}

fn is_hot(cursor: &FileCursor, now_ms: u64) -> bool {
    now_ms.saturating_sub(cursor.mtime_ms) <= HOT_AGE_MS || cursor.size != cursor.offset
}

fn is_jsonl(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "jsonl")
}

#[must_use]
pub(super) fn is_safe_header_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_HEADER_VALUE_BYTES
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn bearer_headers(token: &str, extra_lines: &str) -> Option<String> {
    is_safe_header_value(token).then(|| format!("Authorization: Bearer {token}\r\n{extra_lines}"))
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileId {
    volume: u32,
    index_high: u32,
    index_low: u32,
}

impl FileId {
    fn from_info(info: &BY_HANDLE_FILE_INFORMATION) -> Self {
        Self {
            volume: info.dwVolumeSerialNumber,
            index_high: info.nFileIndexHigh,
            index_low: info.nFileIndexLow,
        }
    }
}

fn read_regular_file(path: &Path) -> Option<String> {
    String::from_utf8(read_regular_file_bytes(path, MAX_CREDENTIAL_FILE_BYTES)?.0).ok()
}

fn read_regular_file_bytes(path: &Path, maximum_bytes: usize) -> Option<(Vec<u8>, FileId)> {
    let owned = OwnedHandle(open_existing_without_following_reparse(path)?);
    let id = regular_file_id(owned.0)?;
    Some((read_handle_bytes(owned.0, maximum_bytes)?, id))
}

fn regular_file_id(handle: HANDLE) -> Option<FileId> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
        return None;
    }
    if info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0 {
        return None;
    }
    Some(FileId::from_info(&info))
}

fn read_handle_bytes(handle: HANDLE, maximum_bytes: usize) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    let mut chunk = [0_u8; 8_192];
    loop {
        let mut read = 0_u32;
        if unsafe {
            ReadFile(
                handle,
                chunk.as_mut_ptr(),
                chunk.len() as u32,
                &mut read,
                ptr::null_mut(),
            )
        } == 0
        {
            return None;
        }
        if read == 0 {
            break;
        }
        let read = read as usize;
        if body.len().saturating_add(read) > maximum_bytes {
            return None;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Some(body)
}

fn open_existing_without_following_reparse(path: &Path) -> Option<HANDLE> {
    create_file(
        path,
        GENERIC_READ,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        OPEN_EXISTING,
        FILE_FLAG_OPEN_REPARSE_POINT,
    )
}

fn create_file(
    path: &Path,
    access: u32,
    share: u32,
    disposition: u32,
    flags: u32,
) -> Option<HANDLE> {
    let path = wide_os_path(path)?;
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            access,
            share,
            ptr::null(),
            disposition,
            flags,
            ptr::null_mut(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        None
    } else {
        Some(handle)
    }
}

fn wide_os_path(path: &Path) -> Option<Vec<u16>> {
    path.to_str()
        .map(|value| value.encode_utf16().chain(Some(0)).collect())
}

fn persist_claude_credentials(claude_dir: &Path, creds: &ClaudeCreds) -> bool {
    let path = claude_dir.join(".credentials.json");
    let Some((raw, original_id)) = read_regular_file_bytes(&path, MAX_CREDENTIAL_FILE_BYTES) else {
        return false;
    };
    let Ok(text) = String::from_utf8(raw) else {
        return false;
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    let Some(oauth) = value.get_mut("claudeAiOauth") else {
        return false;
    };
    oauth["accessToken"] = serde_json::Value::String(creds.access_token.clone());
    if let Some(refresh) = &creds.refresh_token {
        oauth["refreshToken"] = serde_json::Value::String(refresh.clone());
    }
    if let Some(expires) = creds.expires_at_ms {
        oauth["expiresAt"] = serde_json::Value::from(expires);
    }
    let Ok(encoded) = serde_json::to_vec(&value) else {
        return false;
    };

    let Some(tmp) = create_exclusive_tmp(&path, &encoded) else {
        return false;
    };
    let replaced = replace_regular_file(&path, &tmp, original_id);
    if !replaced {
        let _ = fs::remove_file(&tmp);
    }
    replaced
}

fn create_exclusive_tmp(original: &Path, encoded: &[u8]) -> Option<PathBuf> {
    let directory = original.parent()?;
    for attempt in 0..8_u32 {
        let tmp = directory.join(format!(
            ".credentials.{}.{attempt}.tmp",
            unix_now_ms().saturating_add(u64::from(attempt))
        ));
        let Some(handle) = create_file(
            &tmp,
            GENERIC_READ | GENERIC_WRITE,
            0,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
        ) else {
            continue;
        };
        let owned = OwnedHandle(handle);
        if regular_file_id(owned.0).is_none() {
            drop(owned);
            let _ = fs::remove_file(&tmp);
            continue;
        }
        if !write_handle_all(owned.0, encoded) {
            drop(owned);
            let _ = fs::remove_file(&tmp);
            return None;
        }
        drop(owned);
        return Some(tmp);
    }
    None
}

fn write_handle_all(handle: HANDLE, encoded: &[u8]) -> bool {
    let mut written_total = 0_usize;
    while written_total < encoded.len() {
        let mut written = 0_u32;
        let remaining = &encoded[written_total..];
        if unsafe {
            WriteFile(
                handle,
                remaining.as_ptr(),
                remaining.len() as u32,
                &mut written,
                ptr::null_mut(),
            )
        } == 0
            || written == 0
        {
            return false;
        }
        written_total += written as usize;
    }
    (unsafe { FlushFileBuffers(handle) }) != 0
}

fn replace_regular_file(original: &Path, tmp: &Path, original_id: FileId) -> bool {
    let Some(handle) = open_existing_without_following_reparse(original) else {
        return false;
    };
    let owned = OwnedHandle(handle);
    let Some(current_id) = regular_file_id(owned.0) else {
        return false;
    };
    drop(owned);
    if current_id != original_id {
        return false;
    }
    let backup = original.with_file_name(format!(".credentials.{}.bak", unix_now_ms()));
    let original_wide = wide_os_path(original);
    let tmp_wide = wide_os_path(tmp);
    let backup_wide = wide_os_path(&backup);
    let (Some(original_wide), Some(tmp_wide), Some(backup_wide)) =
        (original_wide, tmp_wide, backup_wide)
    else {
        return false;
    };
    let replaced = unsafe {
        ReplaceFileW(
            original_wide.as_ptr(),
            tmp_wide.as_ptr(),
            backup_wide.as_ptr(),
            REPLACEFILE_WRITE_THROUGH,
            ptr::null(),
            ptr::null(),
        )
    } != 0;
    let _ = fs::remove_file(backup);
    replaced
}

fn path_is_under(path: &Path, root: &Path) -> bool {
    let Ok(root) = fs::canonicalize(root) else {
        return false;
    };
    let Ok(path) = fs::canonicalize(path) else {
        return false;
    };
    path.starts_with(root)
}

fn is_current_month(mtime_ms: u64, window: DayWindow) -> bool {
    ymd_key_from_unix(mtime_ms, window.bias_minutes) >= window.month_start
}

struct AppendedChunk {
    new_offset: u64,
    consumed: u64,
    events: Vec<ParsedEvent>,
    dedupe: Vec<Option<[u8; 16]>>,
    limits: Option<ProviderUsage>,
    last_model: Option<String>,
}

fn read_appended(
    path: &Path,
    offset: u64,
    size: u64,
    kind: SourceKind,
    last_model: Option<&str>,
    max_bytes: u64,
    buf: &mut Vec<u8>,
) -> AppendedChunk {
    let mut events = Vec::new();
    let mut keys = Vec::new();
    let mut limits = None;
    let mut model = last_model.map(str::to_owned);
    let empty = || AppendedChunk {
        new_offset: offset,
        consumed: 0,
        events: Vec::new(),
        dedupe: Vec::new(),
        limits: None,
        last_model: model.clone(),
    };
    if size <= offset {
        return empty();
    }
    let Ok(mut file) = File::open(path) else {
        return empty();
    };
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return empty();
    }
    buf.clear();
    buf.resize(8_192, 0);
    let tick_limit = max_bytes.max(1);
    let mut line = Vec::new();
    let mut pos = offset;
    let mut committed = offset;
    let mut oversize = false;
    while pos < size {
        let want = (size - pos) as usize;
        let cap = buf.len();
        let n = match file.read(&mut buf[..want.min(cap)]) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let mut stop = false;
        for &byte in &buf[..n] {
            pos += 1;
            if byte == b'\n' {
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if !oversize && !line.is_empty() && line.len() <= MAX_PARSE_LINE {
                    take_jsonl_line(kind, &line, &mut model, &mut events, &mut keys, &mut limits);
                }
                line.clear();
                oversize = false;
                committed = pos;
                if committed.saturating_sub(offset) >= tick_limit {
                    stop = true;
                    break;
                }
            } else if oversize {
                committed = pos;
                if committed.saturating_sub(offset) >= MAX_SKIP_PER_READ {
                    stop = true;
                    break;
                }
            } else if line.len() < MAX_PARSE_LINE {
                line.push(byte);
            } else {
                line.clear();
                oversize = true;
                committed = pos;
            }
        }
        if stop {
            break;
        }
    }
    AppendedChunk {
        new_offset: committed,
        consumed: committed.saturating_sub(offset),
        events,
        dedupe: keys,
        limits,
        last_model: model,
    }
}

fn take_jsonl_line(
    kind: SourceKind,
    line: &[u8],
    model: &mut Option<String>,
    events: &mut Vec<ParsedEvent>,
    keys: &mut Vec<Option<[u8; 16]>>,
    limits: &mut Option<ProviderUsage>,
) {
    let text = String::from_utf8_lossy(line);
    match kind {
        SourceKind::Claude => {
            if let Some((event, key)) = parse_claude_line(&text) {
                events.push(event);
                keys.push(Some(key));
            }
        }
        SourceKind::Codex => {
            if let Some(next_model) = parse_codex_model(&text) {
                *model = Some(next_model);
                return;
            }
            if let Some(found) = parse_codex_limits_line(&text) {
                *limits = Some(found);
            }
            if let Some(event) = parse_codex_usage_line(&text, model.as_deref()) {
                events.push(event);
                keys.push(None);
            }
        }
    }
}

fn read_codex_limits_tail(path: &Path, size: u64) -> Option<ProviderUsage> {
    let start = size.saturating_sub(CODEX_LIMITS_TAIL);
    let mut file = File::open(path).ok()?;
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    file.take(CODEX_LIMITS_TAIL).read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = text.lines().collect();
    let first = usize::from(start > 0);
    let mut fallback = None;
    for line in lines.iter().skip(first).rev() {
        if !line.contains("\"rate_limits\"") {
            continue;
        }
        let Some(found) = parse_codex_limits_line(line) else {
            continue;
        };
        if is_subscription_limits(&found) {
            return Some(found);
        }
        if fallback.is_none() {
            fallback = Some(found);
        }
    }
    fallback
}

#[derive(Deserialize)]
struct ClaudeAssistantLine {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    message: Option<ClaudeMessage>,
}

#[derive(Deserialize)]
struct ClaudeMessage {
    id: Option<String>,
    model: Option<String>,
    usage: Option<ClaudeUsage>,
}

#[derive(Deserialize)]
struct ClaudeUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_creation: Option<ClaudeCacheCreation>,
    speed: Option<String>,
}

#[derive(Deserialize)]
struct ClaudeCacheCreation {
    ephemeral_5m_input_tokens: Option<u64>,
    ephemeral_1h_input_tokens: Option<u64>,
}

fn parse_claude_line(line: &str) -> Option<(ParsedEvent, [u8; 16])> {
    let rec: ClaudeAssistantLine = serde_json::from_str(line).ok()?;
    if rec.kind.as_deref() != Some("assistant") {
        return None;
    }
    let message = rec.message?;
    let model = message.model.filter(|model| model != "<synthetic>")?;
    let usage = message.usage?;
    let timestamp_ms = parse_timestamp(rec.timestamp.as_deref()?)?;
    let cache_write_5m = usage
        .cache_creation
        .as_ref()
        .and_then(|cache| cache.ephemeral_5m_input_tokens)
        .or(usage.cache_creation_input_tokens)
        .unwrap_or(0);
    let cache_write_1h = usage
        .cache_creation
        .as_ref()
        .and_then(|cache| cache.ephemeral_1h_input_tokens)
        .unwrap_or(0);
    let model = if usage.speed.as_deref() == Some("fast") {
        format!("{model}-fast")
    } else {
        model
    };
    let key = claude_dedupe_digest(
        message.id.as_deref().unwrap_or(""),
        rec.request_id.as_deref().unwrap_or(""),
    );
    Some((
        ParsedEvent {
            model,
            timestamp_ms,
            usage: TokenUsage {
                input: usage.input_tokens.unwrap_or(0),
                output: usage.output_tokens.unwrap_or(0),
                cache_read: usage.cache_read_input_tokens.unwrap_or(0),
                cache_write_5m,
                cache_write_1h,
                ..TokenUsage::default()
            },
            codex_last: None,
            codex_total: None,
        },
        key,
    ))
}

#[derive(Deserialize)]
struct CodexLine {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    payload: Option<CodexPayload>,
}

#[derive(Deserialize)]
struct CodexPayload {
    #[serde(rename = "type")]
    kind: Option<String>,
    model: Option<String>,
    info: Option<CodexInfo>,
    rate_limits: Option<CodexRateLimits>,
}

#[derive(Deserialize)]
struct CodexInfo {
    last_token_usage: Option<CodexTokens>,
    total_token_usage: Option<CodexTokens>,
}

#[derive(Deserialize)]
struct CodexTokens {
    input_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    /// Breakdown of `output_tokens`. Not an extra billed bucket.
    #[allow(dead_code)]
    reasoning_output_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct CodexRateLimits {
    primary: Option<CodexWindow>,
    secondary: Option<CodexWindow>,
    plan_type: Option<String>,
    rate_limit_reset_credits: Option<ResetCredits>,
}

#[derive(Deserialize)]
struct CodexWindow {
    used_percent: Option<f64>,
    resets_at: Option<f64>,
    window_minutes: Option<u16>,
}

fn parse_codex_model(line: &str) -> Option<String> {
    let rec: CodexLine = serde_json::from_str(line).ok()?;
    if rec.kind.as_deref() == Some("turn_context") {
        rec.payload?.model
    } else {
        None
    }
}

fn parse_codex_usage_line(line: &str, model: Option<&str>) -> Option<ParsedEvent> {
    let rec: CodexLine = serde_json::from_str(line).ok()?;
    if rec.kind.as_deref() != Some("event_msg") {
        return None;
    }
    let payload = rec.payload?;
    if payload.kind.as_deref() != Some("token_count") {
        return None;
    }
    let model = crate::core::resolve_codex_model(model?);
    let info = payload.info?;
    let last_tokens = info.last_token_usage?;
    let last = codex_tokens_to_totals(&last_tokens);
    let total = info.total_token_usage.as_ref().map(codex_tokens_to_totals);
    let mut usage = last.to_usage();
    if is_long_context_request(model, last.input) {
        usage.long_context_input = usage.input;
        usage.long_context_cached_input = usage.cached_input;
        usage.long_context_output = usage.output;
    }
    Some(ParsedEvent {
        model: model.to_owned(),
        timestamp_ms: parse_timestamp(rec.timestamp.as_deref()?)?,
        usage,
        codex_last: Some(last),
        codex_total: total,
    })
}

fn codex_tokens_to_totals(tokens: &CodexTokens) -> CodexTokenTotals {
    CodexTokenTotals {
        input: tokens.input_tokens.unwrap_or(0),
        cached: tokens.cached_input_tokens.unwrap_or(0),
        // Codex `reasoning_output_tokens` is a breakdown of `output_tokens`,
        // not an extra billed bucket (`total_tokens == input + output`).
        output: tokens.output_tokens.unwrap_or(0),
    }
}

fn parse_codex_limits_line(line: &str) -> Option<ProviderUsage> {
    let rec: CodexLine = serde_json::from_str(line).ok()?;
    let limits = rec.payload?.rate_limits?;
    let mut usage = ProviderUsage {
        primary: limits.primary.and_then(codex_window),
        secondary: limits.secondary.and_then(codex_window),
        banked_reset_available: reset_credit_count(limits.rate_limit_reset_credits),
        ..ProviderUsage::default()
    };
    if let Some(plan) = limits.plan_type {
        usage.set_chatgpt_plan(&plan);
    }
    if usage.primary.is_none()
        && usage.secondary.is_none()
        && usage.banked_reset_available.is_none()
    {
        None
    } else {
        Some(usage)
    }
}

fn codex_window(window: CodexWindow) -> Option<LimitWindow> {
    let used = window.used_percent?;
    Some(LimitWindow {
        used_tenths: (used * 10.0).round().clamp(0.0, 1000.0) as u16,
        resets_at_ms: window
            .resets_at
            .map(|seconds| (seconds * 1000.0) as u64)
            .unwrap_or(0),
        window_minutes: window.window_minutes.unwrap_or(0),
    })
}

#[derive(Deserialize)]
struct ClaudeCredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    oauth: Option<ClaudeOauth>,
}

#[derive(Deserialize)]
struct ClaudeOauth {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at: Option<u64>,
    #[serde(rename = "subscriptionType")]
    subscription_type: Option<String>,
    #[serde(rename = "rateLimitTier", alias = "rate_limit_tier")]
    rate_limit_tier: Option<String>,
}

struct ClaudeCreds {
    access_token: String,
    refresh_token: Option<String>,
    expires_at_ms: Option<u64>,
    plan: Option<String>,
}

fn read_claude_credentials(claude_dir: &Path) -> Option<ClaudeCreds> {
    let raw = read_regular_file(&claude_dir.join(".credentials.json"))?;
    let file: ClaudeCredentialsFile = serde_json::from_str(&raw).ok()?;
    let oauth = file.oauth?;
    let access_token = oauth.access_token.filter(|token| !token.is_empty())?;
    Some(ClaudeCreds {
        access_token,
        refresh_token: oauth.refresh_token.filter(|token| !token.is_empty()),
        expires_at_ms: oauth.expires_at.map(normalize_expiry_ms),
        plan: oauth
            .rate_limit_tier
            .filter(|tier| !tier.is_empty())
            .or(oauth.subscription_type.filter(|kind| !kind.is_empty())),
    })
}

fn normalize_expiry_ms(expires: u64) -> u64 {
    if expires < 100_000_000_000 {
        expires.saturating_mul(1_000)
    } else {
        expires
    }
}

fn claude_token_expired(creds: &ClaudeCreds) -> bool {
    creds
        .expires_at_ms
        .is_some_and(|expires| expires <= unix_now_ms())
}

#[derive(Deserialize)]
struct ClaudeUsageResponse {
    five_hour: Option<ClaudeUsageWindow>,
    seven_day: Option<ClaudeUsageWindow>,
    seven_day_fable: Option<ClaudeUsageWindow>,
    #[serde(default)]
    model_scoped: Vec<ClaudeModelScoped>,
    #[serde(default)]
    limits: Vec<ClaudeScopedLimit>,
}

#[derive(Deserialize)]
struct ClaudeModelScoped {
    display_name: Option<String>,
    utilization: Option<f64>,
    percent: Option<f64>,
    resets_at: Option<String>,
}

#[derive(Deserialize)]
struct ClaudeScopedLimit {
    kind: Option<String>,
    percent: Option<f64>,
    utilization: Option<f64>,
    resets_at: Option<String>,
    scope: Option<ClaudeLimitScope>,
}

#[derive(Deserialize)]
struct ClaudeLimitScope {
    model: Option<ClaudeLimitModel>,
}

#[derive(Deserialize)]
struct ClaudeLimitModel {
    display_name: Option<String>,
}

#[derive(Clone, Deserialize)]
struct ClaudeUsageWindow {
    utilization: Option<f64>,
    resets_at: Option<String>,
}

#[derive(Deserialize)]
struct OAuthTokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    expires_at: Option<u64>,
}

fn fetch_claude_limits(claude_dir: &Path) -> Result<ProviderUsage, FetchErrorKind> {
    let mut creds = read_claude_credentials(claude_dir).ok_or(FetchErrorKind::AuthMissing)?;
    if claude_token_expired(&creds) {
        let _ = refresh_claude_credentials(claude_dir, &mut creds);
    }
    match claude_usage_request(&creds.access_token, creds.plan.as_deref()) {
        Ok(usage) => return Ok(usage),
        Err(FetchErrorKind::AuthMissing) => {}
        Err(error) => {
            if !refresh_claude_credentials(claude_dir, &mut creds) {
                return Err(error);
            }
            return claude_usage_request(&creds.access_token, creds.plan.as_deref());
        }
    }
    if refresh_claude_credentials(claude_dir, &mut creds) {
        claude_usage_request(&creds.access_token, creds.plan.as_deref())
    } else {
        Err(FetchErrorKind::AuthMissing)
    }
}

fn claude_usage_request(token: &str, plan: Option<&str>) -> Result<ProviderUsage, FetchErrorKind> {
    let headers = bearer_headers(token, "anthropic-beta: oauth-2025-04-20\r\n")
        .ok_or(FetchErrorKind::AuthMissing)?;
    let (status, body) = super::update::https_get(
        "api.anthropic.com",
        "/api/oauth/usage",
        &headers,
        16 * 1_024,
    )
    .map_err(|_| FetchErrorKind::Transport)?;
    if status != 200 {
        return Err(FetchErrorKind::HttpStatus);
    }
    let text = String::from_utf8(body).map_err(|_| FetchErrorKind::Parse)?;
    parse_claude_usage_response(&text, plan).ok_or(FetchErrorKind::Parse)
}

fn refresh_claude_credentials(claude_dir: &Path, creds: &mut ClaudeCreds) -> bool {
    let Some(refresh_token) = creds.refresh_token.as_deref() else {
        return false;
    };
    if !is_safe_header_value(refresh_token) {
        return false;
    }
    let payload = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": CLAUDE_OAUTH_CLIENT_ID,
    })
    .to_string();
    let headers = "Content-Type: application/json\r\n";
    for host in ["platform.claude.com", "console.anthropic.com"] {
        let Ok((status, body)) = super::update::https_post(
            host,
            "/v1/oauth/token",
            headers,
            payload.as_bytes(),
            8 * 1_024,
        ) else {
            continue;
        };
        if status == 404 || status == 405 {
            continue;
        }
        if status != 200 {
            return false;
        }
        let Ok(text) = String::from_utf8(body) else {
            return false;
        };
        let Ok(parsed) = serde_json::from_str::<OAuthTokenResponse>(&text) else {
            return false;
        };
        let Some(access) = parsed.access_token.filter(|token| !token.is_empty()) else {
            return false;
        };
        if !is_safe_header_value(&access) {
            return false;
        }
        creds.access_token = access;
        if let Some(next) = parsed.refresh_token.filter(|token| !token.is_empty()) {
            if !is_safe_header_value(&next) {
                return false;
            }
            creds.refresh_token = Some(next);
        }
        creds.expires_at_ms = Some(parsed.expires_at.map_or_else(
            || {
                unix_now_ms()
                    .saturating_add(parsed.expires_in.unwrap_or(3_600).saturating_mul(1_000))
            },
            normalize_expiry_ms,
        ));
        return persist_claude_credentials(claude_dir, creds);
    }
    false
}

#[derive(Deserialize)]
struct CodexAuthFile {
    tokens: Option<CodexAuthTokens>,
}

#[derive(Deserialize)]
struct CodexAuthTokens {
    access_token: Option<String>,
    account_id: Option<String>,
}

#[derive(Deserialize)]
struct WhamUsageResponse {
    plan_type: Option<String>,
    rate_limit: Option<WhamRateLimit>,
    rate_limit_reset_credits: Option<ResetCredits>,
}

#[derive(Deserialize)]
struct ResetCredits {
    available_count: Option<i64>,
}

#[derive(Deserialize)]
struct WhamRateLimit {
    primary_window: Option<WhamWindow>,
    secondary_window: Option<WhamWindow>,
}

#[derive(Deserialize)]
struct WhamWindow {
    used_percent: Option<f64>,
    limit_window_seconds: Option<u64>,
    reset_at: Option<f64>,
}

fn fetch_codex_wham_limits(codex_home: &Path) -> Result<ProviderUsage, FetchErrorKind> {
    let raw =
        read_regular_file(&codex_home.join("auth.json")).ok_or(FetchErrorKind::AuthMissing)?;
    let file: CodexAuthFile = serde_json::from_str(&raw).map_err(|_| FetchErrorKind::Parse)?;
    let tokens = file.tokens.ok_or(FetchErrorKind::AuthMissing)?;
    let access = tokens
        .access_token
        .filter(|token| !token.is_empty())
        .ok_or(FetchErrorKind::AuthMissing)?;
    let account = tokens
        .account_id
        .filter(|id| !id.is_empty())
        .ok_or(FetchErrorKind::AuthMissing)?;
    if !is_safe_header_value(&access) || !is_safe_header_value(&account) {
        return Err(FetchErrorKind::AuthMissing);
    }
    let (status, body) = super::update::https_get(
        "chatgpt.com",
        "/backend-api/wham/usage",
        &format!(
            "Authorization: Bearer {access}\r\nChatGPT-Account-Id: {account}\r\nAccept: application/json\r\n"
        ),
        16 * 1_024,
    )
    .map_err(|_| FetchErrorKind::Transport)?;
    if status != 200 {
        return Err(FetchErrorKind::HttpStatus);
    }
    let text = String::from_utf8(body).map_err(|_| FetchErrorKind::Parse)?;
    parse_wham_usage_response(&text).ok_or(FetchErrorKind::Parse)
}

pub fn parse_wham_usage_response(body: &str) -> Option<ProviderUsage> {
    let parsed: WhamUsageResponse = serde_json::from_str(body).ok()?;
    let mut usage = ProviderUsage {
        banked_reset_available: reset_credit_count(parsed.rate_limit_reset_credits),
        ..ProviderUsage::default()
    };
    if let Some(rate) = parsed.rate_limit {
        usage.primary = rate.primary_window.and_then(wham_window);
        usage.secondary = rate.secondary_window.and_then(wham_window);
    }
    if let Some(plan) = parsed.plan_type {
        usage.set_chatgpt_plan(&plan);
    }
    if usage.primary.is_none()
        && usage.secondary.is_none()
        && usage.banked_reset_available.is_none()
    {
        None
    } else {
        Some(usage)
    }
}

fn reset_credit_count(credits: Option<ResetCredits>) -> Option<u16> {
    let count = credits?.available_count?;
    u16::try_from(count).ok()
}

fn wham_window(window: WhamWindow) -> Option<LimitWindow> {
    let used = window.used_percent?;
    Some(LimitWindow {
        used_tenths: (used * 10.0).round().clamp(0.0, 1000.0) as u16,
        resets_at_ms: window
            .reset_at
            .map(|seconds| (seconds * 1000.0) as u64)
            .unwrap_or(0),
        window_minutes: window
            .limit_window_seconds
            .map(|seconds| (seconds / 60) as u16)
            .unwrap_or(0),
    })
}

fn apply_provider_limits(target: &mut ProviderUsage, limits: ProviderUsage) {
    target.primary = limits.primary;
    target.secondary = limits.secondary;
    target.fable = limits.fable;
    target.banked_reset_available = limits.banked_reset_available;
    if limits.plan_len != 0 {
        target.plan = limits.plan;
        target.plan_len = limits.plan_len;
    }
}

fn is_subscription_limits(usage: &ProviderUsage) -> bool {
    usage.secondary.is_some()
        || usage
            .primary
            .is_some_and(|window| window.window_minutes > 0 && window.window_minutes <= 300)
}

pub fn parse_claude_usage_response(body: &str, plan: Option<&str>) -> Option<ProviderUsage> {
    let parsed: ClaudeUsageResponse = serde_json::from_str(body).ok()?;
    let fable = extract_fable_window(&parsed);
    let mut usage = ProviderUsage {
        primary: claude_window(parsed.five_hour, 300),
        secondary: claude_window(parsed.seven_day, 10_080),
        fable,
        ..ProviderUsage::default()
    };
    if let Some(plan) = plan {
        usage.set_plan(plan);
    }
    if usage.primary.is_none() && usage.secondary.is_none() && usage.fable.is_none() {
        None
    } else {
        Some(usage)
    }
}

fn extract_fable_window(parsed: &ClaudeUsageResponse) -> Option<LimitWindow> {
    if let Some(window) = claude_window(parsed.seven_day_fable.clone(), 10_080) {
        return Some(window);
    }
    for scoped in &parsed.model_scoped {
        if !is_fable_label(scoped.display_name.as_deref()) {
            continue;
        }
        if let Some(window) = claude_scoped_window(
            scoped.utilization,
            scoped.percent,
            scoped.resets_at.as_deref(),
        ) {
            return Some(window);
        }
    }
    for limit in &parsed.limits {
        if !limit
            .kind
            .as_deref()
            .is_some_and(|kind| kind.eq_ignore_ascii_case("weekly_scoped"))
        {
            continue;
        }
        let name = limit
            .scope
            .as_ref()
            .and_then(|scope| scope.model.as_ref())
            .and_then(|model| model.display_name.as_deref());
        if !is_fable_label(name) {
            continue;
        }
        if let Some(window) =
            claude_scoped_window(limit.utilization, limit.percent, limit.resets_at.as_deref())
        {
            return Some(window);
        }
    }
    None
}

fn is_fable_label(name: Option<&str>) -> bool {
    name.is_some_and(|label| label.to_ascii_lowercase().contains("fable"))
}

fn claude_scoped_window(
    utilization: Option<f64>,
    percent: Option<f64>,
    resets_at: Option<&str>,
) -> Option<LimitWindow> {
    claude_window(
        Some(ClaudeUsageWindow {
            utilization: utilization.or(percent),
            resets_at: resets_at.map(str::to_owned),
        }),
        10_080,
    )
}

fn claude_window(window: Option<ClaudeUsageWindow>, minutes: u16) -> Option<LimitWindow> {
    let window = window?;
    let used = window.utilization?;
    Some(LimitWindow {
        used_tenths: (used * 10.0).round().clamp(0.0, 1000.0) as u16,
        resets_at_ms: window
            .resets_at
            .as_deref()
            .and_then(parse_timestamp)
            .unwrap_or(0),
        window_minutes: minutes,
    })
}

fn parse_timestamp(value: &str) -> Option<u64> {
    parse_rfc3339_ms(value)
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub(super) fn timezone_bias_minutes() -> i32 {
    let mut info = TIME_ZONE_INFORMATION::default();
    let zone_id = unsafe { GetTimeZoneInformation(&mut info) };
    windows_tz_bias_minutes(info.Bias, info.StandardBias, info.DaylightBias, zone_id)
}

fn day_window(now_ms: u64) -> DayWindow {
    let bias = timezone_bias_minutes();
    let (year, month, day) = local_ymd(now_ms, bias);
    DayWindow {
        bias_minutes: bias,
        today: ymd_key(year, month, day),
        month_start: ymd_key(year, month, 1),
    }
}

fn ymd_key_from_unix(unix_ms: u64, bias_minutes: i32) -> u32 {
    let (year, month, day) = local_ymd(unix_ms, bias_minutes);
    ymd_key(year, month, day)
}

fn previous_month_start(window: DayWindow) -> u32 {
    let year = (window.month_start / 10_000) as i32;
    let month = ((window.month_start / 100) % 100) as u8;
    let (year, month) = previous_month(year, month);
    ymd_key(year, month, 1)
}

fn previous_month(year: i32, month: u8) -> (i32, u8) {
    if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    }
}

fn codex_month_dir(home: &Path, year: i32, month: u8) -> PathBuf {
    home.join("sessions")
        .join(year.to_string())
        .join(format!("{month:02}"))
}

/// Enqueue Codex `sessions/{year}/{month}` dirs touched within `hot_age_ms`.
/// Walks the current and previous calendar years only — metadata, not a
/// full-history JSONL scan.
fn queue_recent_codex_month_dirs(
    discover: &mut VecDeque<PathBuf>,
    home: &Path,
    year: i32,
    now_ms: u64,
    hot_age_ms: u64,
) {
    for walk_year in [year, year - 1] {
        let year_dir = home.join("sessions").join(walk_year.to_string());
        let Ok(entries) = fs::read_dir(&year_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let Some(mtime_ms) = path_mtime_ms(&path) else {
                continue;
            };
            if now_ms.saturating_sub(mtime_ms) > hot_age_ms {
                continue;
            }
            if !discover.contains(&path) {
                discover.push_back(path);
            }
        }
    }
}

fn checkpoint_disk_path(claude_dir: &Path, codex_home: &Path, key: &FileCheckpointKey) -> PathBuf {
    let (root, logical_id) = match key {
        FileCheckpointKey::Claude(id) => (claude_dir, id.as_str()),
        FileCheckpointKey::Codex(id) => (codex_home, id.as_str()),
    };
    let mut path = root.to_path_buf();
    for part in logical_id.split(['/', '\\']) {
        if part.is_empty() || part == "." || part == ".." || part.contains(':') {
            continue;
        }
        path.push(part);
    }
    path
}

fn file_logical_id(
    path: &Path,
    claude_dir: &Path,
    codex_home: &Path,
    kind: SourceKind,
) -> Option<String> {
    match file_checkpoint_key(path, claude_dir, codex_home, kind)? {
        FileCheckpointKey::Claude(id) | FileCheckpointKey::Codex(id) => Some(id),
    }
}

fn file_checkpoint_key(
    path: &Path,
    claude_dir: &Path,
    codex_home: &Path,
    kind: SourceKind,
) -> Option<FileCheckpointKey> {
    let relative = match kind {
        SourceKind::Claude => path.strip_prefix(claude_dir).ok()?,
        SourceKind::Codex => path.strip_prefix(codex_home).ok()?,
    };
    let normalized = relative.to_string_lossy().replace('\\', "/");
    Some(match kind {
        SourceKind::Claude => FileCheckpointKey::Claude(normalized),
        SourceKind::Codex => FileCheckpointKey::Codex(normalized),
    })
}

fn restored_cursor_offset(
    checkpoint: &HashMap<FileCheckpointKey, UsageCursor>,
    path: &Path,
    claude_dir: &Path,
    codex_home: &Path,
    kind: SourceKind,
    size: u64,
    prefix: Option<u64>,
) -> (u64, Option<CursorRebuildReason>) {
    let Some(key) = file_checkpoint_key(path, claude_dir, codex_home, kind) else {
        return (0, None);
    };
    let Some(cursor) = checkpoint.get(&key) else {
        return (0, None);
    };
    if size < cursor.size || size < cursor.offset {
        return (0, Some(CursorRebuildReason::SizeShrunk));
    }
    if let (Some(stored), Some(current)) = (cursor.prefix, prefix) {
        if stored != current {
            return (0, Some(CursorRebuildReason::PrefixChanged));
        }
    }
    (cursor.offset.min(size), None)
}

fn cursor_rebuild_reason(
    cursor: &FileCursor,
    size: u64,
    mtime_ms: u64,
    prefix: Option<u64>,
    file_id: Option<FileId>,
) -> Option<CursorRebuildReason> {
    if size < cursor.offset {
        return Some(CursorRebuildReason::SizeShrunk);
    }
    let prefix_changed = matches!((cursor.last_prefix, prefix), (Some(previous), Some(current)) if previous != current);
    let file_id_changed = matches!((cursor.last_file_id, file_id), (Some(previous), Some(current)) if previous != current);
    if file_id_changed && prefix_changed {
        return Some(CursorRebuildReason::FileIdAndPrefixChanged);
    }
    if prefix_changed {
        return Some(CursorRebuildReason::PrefixChanged);
    }
    // Same size + mtime move is a hint, not a complete rewrite detector.
    // Prefix/FileId can miss an in-place rewrite past the first 64 bytes.
    if size == cursor.offset && size == cursor.size && mtime_ms != cursor.mtime_ms {
        return Some(CursorRebuildReason::SameSizeRewriteHint);
    }
    None
}

fn file_prefix_fingerprint(path: &Path) -> Option<u64> {
    let mut file = File::open(path).ok()?;
    let mut buf = [0_u8; 64];
    let n = file.read(&mut buf).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    n.hash(&mut hasher);
    buf[..n].hash(&mut hasher);
    Some(hasher.finish())
}

fn jsonl_file_id(path: &Path) -> Option<FileId> {
    let owned = OwnedHandle(open_existing_without_following_reparse(path)?);
    regular_file_id(owned.0)
}

fn path_mtime_ms(path: &Path) -> Option<u64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    Some(modified.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64)
}

fn path_size_mtime(path: &Path) -> Option<(u64, u64)> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    Some((meta.len(), mtime))
}

#[cfg(test)]
mod tests {
    use super::{
        is_current_month, is_safe_header_value, is_subscription_limits, parse_claude_line,
        parse_claude_usage_response, parse_codex_limits_line, parse_codex_model,
        parse_codex_usage_line, parse_timestamp, parse_wham_usage_response,
        persist_claude_credentials, read_claude_credentials, read_regular_file, unix_now_ms,
        UsageCollector, UsageTick,
    };
    use crate::core::{local_hms, local_ymd, CursorRebuildReason};
    use std::{
        fs,
        path::{Path, PathBuf},
        ptr,
    };

    impl UsageCollector {
        fn test_file_offset(&self, path: &Path) -> Option<u64> {
            self.files.get(path).map(|cursor| cursor.offset)
        }

        fn test_force_restat(&mut self) {
            for cursor in self.files.values_mut() {
                cursor.last_stat_ms = 0;
            }
        }

        fn test_store_root(&self) -> Option<PathBuf> {
            self.store.as_ref().map(|store| store.root().to_path_buf())
        }

        fn test_last_model(&self, path: &Path) -> Option<String> {
            self.files
                .get(path)
                .and_then(|cursor| cursor.last_model.clone())
        }

        fn test_codex_total(&self, path: &Path) -> Option<(u64, u64, u64)> {
            self.files.get(path).and_then(|cursor| {
                cursor
                    .last_codex_total
                    .map(|total| (total.input, total.cached, total.output))
            })
        }

        fn test_mark_file_cold(&mut self, path: &Path) {
            if let Some(cursor) = self.files.get_mut(path) {
                cursor.mtime_ms = 1;
                cursor.last_stat_ms = 0;
            }
        }

        fn test_complete_fetch(
            &mut self,
            kind: crate::core::ProviderFetchKind,
            usage: Option<crate::core::ProviderUsage>,
            error: Option<crate::core::FetchErrorKind>,
        ) -> bool {
            let now = unix_now_ms();
            let slot = match kind {
                crate::core::ProviderFetchKind::Claude => &mut self.claude_fetch,
                crate::core::ProviderFetchKind::Codex => &mut self.codex_fetch,
            };
            let generation = if slot.state.request_generation == 0 || !slot.state.in_flight {
                crate::core::start_fetch(&mut slot.state, now)
            } else {
                slot.state.request_generation
            };
            self.apply_remote_outcome(
                kind,
                super::SlotOutcome {
                    generation,
                    usage,
                    error,
                    observed_at_ms: now,
                },
                now,
            )
        }

        fn test_fetch_freshness(
            &self,
            kind: crate::core::ProviderFetchKind,
        ) -> crate::core::LimitsFreshness {
            match kind {
                crate::core::ProviderFetchKind::Claude => self.claude_fetch.state.freshness,
                crate::core::ProviderFetchKind::Codex => self.codex_fetch.state.freshness,
            }
        }

        fn test_mark_spawn_failure(&mut self, kind: crate::core::ProviderFetchKind) {
            let now = unix_now_ms();
            let slot = match kind {
                crate::core::ProviderFetchKind::Claude => &mut self.claude_fetch,
                crate::core::ProviderFetchKind::Codex => &mut self.codex_fetch,
            };
            let _ = crate::core::start_fetch(&mut slot.state, now);
            crate::core::record_spawn_failure(&mut slot.state, now);
        }

        fn test_remember_codex(&mut self, logical_id: &str) {
            self.file_checkpoint.insert(
                crate::core::FileCheckpointKey::Codex(logical_id.to_owned()),
                crate::core::UsageCursor {
                    kind: crate::core::CursorKind::Codex,
                    logical_id: logical_id.to_owned(),
                    offset: 0,
                    size: 0,
                    prefix: None,
                    last_model: None,
                    last_codex_total: None,
                },
            );
        }
    }

    fn sample_limit_usage(used_tenths: u16) -> crate::core::ProviderUsage {
        crate::core::ProviderUsage {
            primary: Some(crate::core::LimitWindow {
                used_tenths,
                resets_at_ms: unix_now_ms().saturating_add(3_600_000),
                window_minutes: 300,
            }),
            ..crate::core::ProviderUsage::default()
        }
    }

    fn current_stamp() -> String {
        let now = unix_now_ms();
        let (year, month, day) = local_ymd(now, 0);
        let (hour, minute) = local_hms(now, 0);
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:00Z")
    }

    fn claude_usage_line(id: &str, stamp: &str) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{stamp}","requestId":"{id}","message":{{"id":"{id}","model":"claude-opus-5","usage":{{"input_tokens":1000000,"output_tokens":0}}}}}}"#
        )
    }

    fn claude_usage_line_with_len(id: &str, stamp: &str, target: usize) -> String {
        let prefix = format!(
            r#"{{"type":"assistant","timestamp":"{stamp}","requestId":"{id}","message":{{"id":"{id}","model":"claude-opus-5","usage":{{"input_tokens":1000000,"output_tokens":0}}}},"pad":""#
        );
        let suffix = "\"}";
        let pad = target.saturating_sub(prefix.len() + suffix.len());
        format!("{prefix}{}{suffix}", "x".repeat(pad))
    }

    fn tick_idle(collector: &mut UsageCollector) -> UsageTick {
        let mut last = UsageTick::MoreWork;
        for _ in 0..64 {
            last = collector.tick(ptr::null_mut());
            if last == UsageTick::Idle && !collector.catch_up {
                break;
            }
        }
        last
    }

    fn new_claude_collector(root: &Path) -> UsageCollector {
        UsageCollector::with_dirs(root.join("claude"), root.join("codex"), false)
    }

    fn new_persisted_collector(root: &Path, store_root: PathBuf) -> UsageCollector {
        UsageCollector::with_dirs_and_store(
            root.join("claude"),
            root.join("codex"),
            Some(super::FileUsageStore::at(store_root)),
        )
    }

    const TURN_CONTEXT: &str = r#"{"type":"turn_context","payload":{"model":"gpt-5.4"}}"#;

    fn current_codex_session(root: &Path, name: &str) -> PathBuf {
        let now = unix_now_ms();
        let (year, month, _) = local_ymd(now, 0);
        let dir = root
            .join("codex")
            .join("sessions")
            .join(year.to_string())
            .join(format!("{month:02}"));
        fs::create_dir_all(&dir).expect("codex month");
        dir.join(name)
    }

    fn write_codex_session(root: &Path, name: &str, lines: &[String]) -> PathBuf {
        write_codex_session_with_model(root, name, "gpt-5.4", lines)
    }

    fn write_codex_session_with_model(
        root: &Path,
        name: &str,
        model: &str,
        lines: &[String],
    ) -> PathBuf {
        let session = current_codex_session(root, name);
        let mut body = String::new();
        body.push_str(&format!(
            r#"{{"type":"turn_context","payload":{{"model":"{model}"}}}}"#
        ));
        body.push('\n');
        for line in lines {
            body.push_str(line);
            body.push('\n');
        }
        fs::write(&session, body).expect("jsonl");
        session
    }

    fn append_codex_lines(session: &Path, lines: &[String]) {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(session)
            .expect("append");
        for line in lines {
            writeln!(file, "{line}").expect("line");
        }
    }

    fn token_count_line(
        stamp: &str,
        last_in: u64,
        last_cached: u64,
        last_out: u64,
        total_in: u64,
        total_cached: u64,
        total_out: u64,
    ) -> String {
        format!(
            r#"{{"type":"event_msg","timestamp":"{stamp}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{last_in},"cached_input_tokens":{last_cached},"output_tokens":{last_out}}},"total_token_usage":{{"input_tokens":{total_in},"cached_input_tokens":{total_cached},"output_tokens":{total_out}}}}}}}}}"#
        )
    }

    fn token_count_line_no_total(
        stamp: &str,
        last_in: u64,
        last_cached: u64,
        last_out: u64,
    ) -> String {
        format!(
            r#"{{"type":"event_msg","timestamp":"{stamp}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{last_in},"cached_input_tokens":{last_cached},"output_tokens":{last_out}}}}}}}}}"#
        )
    }

    fn token_count_with_limits(
        stamp: &str,
        last_in: u64,
        last_cached: u64,
        last_out: u64,
        total_in: u64,
        total_cached: u64,
        total_out: u64,
    ) -> String {
        format!(
            r#"{{"type":"event_msg","timestamp":"{stamp}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{last_in},"cached_input_tokens":{last_cached},"output_tokens":{last_out}}},"total_token_usage":{{"input_tokens":{total_in},"cached_input_tokens":{total_cached},"output_tokens":{total_out}}}}},"rate_limits":{{"plan_type":"pro","primary":{{"used_percent":5.0,"resets_at":1780000000,"window_minutes":300}},"secondary":{{"used_percent":21.5,"resets_at":1780500000,"window_minutes":10080}}}}}}}}"#
        )
    }

    fn collect_codex(root: &Path) -> crate::core::ProviderUsage {
        let mut collector = new_claude_collector(root);
        assert_eq!(tick_idle(&mut collector), UsageTick::Idle);
        collector.snapshot().codex
    }

    fn year_outside_walk_window() -> i32 {
        let (year, _, _) = local_ymd(unix_now_ms(), 0);
        year - 3
    }

    fn write_old_year_codex_session(
        root: &Path,
        year: i32,
        name: &str,
        lines: &[String],
    ) -> PathBuf {
        let dir = super::codex_month_dir(&root.join("codex"), year, 1);
        fs::create_dir_all(&dir).expect("old year");
        let session = dir.join(name);
        let mut body = String::new();
        body.push_str(TURN_CONTEXT);
        body.push('\n');
        for line in lines {
            body.push_str(line);
            body.push('\n');
        }
        fs::write(&session, body).expect("jsonl");
        session
    }

    fn claude_session(root: &Path) -> PathBuf {
        let project = root.join("claude").join("projects").join("p1");
        fs::create_dir_all(&project).expect("project");
        project.join("session.jsonl")
    }

    fn read_chunk(path: &Path, offset: u64, max_bytes: u64) -> super::AppendedChunk {
        let size = fs::metadata(path).expect("meta").len();
        let mut buf = Vec::new();
        super::read_appended(
            path,
            offset,
            size,
            super::SourceKind::Claude,
            None,
            max_bytes,
            &mut buf,
        )
    }

    #[test]
    fn component_usage_read_intervals_are_at_least_one_minute() {
        const {
            assert!(super::USAGE_IDLE_INTERVAL_MS >= 60_000);
            assert!(super::USAGE_CONTINUE_INTERVAL_MS >= 60_000);
            assert!(super::REDISCOVER_MS == super::USAGE_IDLE_INTERVAL_MS as u64);
            assert!(super::STAT_COOLDOWN_MS >= 60_000);
            assert!(super::CLAUDE_LIMITS_PERIOD_MS >= 60_000);
            assert!(super::CODEX_LIMITS_PERIOD_MS >= 60_000);
        }
    }

    #[test]
    fn component_idle_collector_rediscovers_a_new_session_next_interval() {
        let root = std::env::temp_dir().join(format!(
            "rundog-rediscover-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let mut collector = new_claude_collector(&root);
        assert_eq!(tick_idle(&mut collector), UsageTick::Idle);

        let session = claude_session(&root);
        let line = claude_usage_line("new-session", &current_stamp());
        fs::write(session, format!("{line}\n")).unwrap();
        collector.last_discover_ms = unix_now_ms().saturating_sub(super::REDISCOVER_MS + 1);

        assert_eq!(tick_idle(&mut collector), UsageTick::Idle);
        assert_eq!(collector.snapshot().claude.month_input_tokens, 1_000_000);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn component_claude_assistant_line_extracts_tokens_and_fast_sku() {
        let line = r#"{"type":"assistant","timestamp":"2026-08-16T01:02:03Z","requestId":"r1","message":{"id":"m1","model":"claude-opus-5","usage":{"input_tokens":10,"output_tokens":4,"cache_read_input_tokens":2,"speed":"fast"}}}"#;
        let (event, _) = parse_claude_line(line).expect("assistant usage line");
        assert_eq!(event.model, "claude-opus-5-fast");
        assert_eq!(event.usage.input, 10);
        assert_eq!(event.usage.output, 4);
        assert_eq!(event.usage.cache_read, 2);
    }

    #[test]
    fn c2_header_values_reject_crlf_and_empty_tokens() {
        assert!(is_safe_header_value("eyJhbGciOiJIUzI1NiJ9.payload.sig"));
        assert!(!is_safe_header_value(""));
        assert!(!is_safe_header_value("abc\r\nX-Injected: 1"));
        assert!(!is_safe_header_value("abc\n"));
        assert!(!is_safe_header_value("token with space"));
    }

    fn sample_claude_credentials() -> String {
        r#"{"claudeAiOauth":{"accessToken":"sk-ant-test","expiresAt":4102444800000,"subscriptionType":"pro"}}"#.to_owned()
    }

    #[test]
    fn component_regular_credentials_file_is_read_from_the_opened_handle() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-creds-regular-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        fs::create_dir_all(&root).expect("temp dir");
        fs::write(root.join(".credentials.json"), sample_claude_credentials())
            .expect("credentials");
        let creds = read_claude_credentials(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(
            creds.map(|value| value.access_token),
            Some("sk-ant-test".to_owned())
        );
    }

    #[test]
    fn component_symlink_credentials_are_not_followed() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-creds-link-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        fs::create_dir_all(&root).expect("temp dir");
        let target = root.join("elsewhere.json");
        fs::write(&target, sample_claude_credentials()).expect("target");
        let link = root.join(".credentials.json");
        let linked = std::os::windows::fs::symlink_file(&target, &link).is_ok();
        let creds = read_claude_credentials(&root);
        let through_link = read_regular_file(&link);
        let _ = fs::remove_dir_all(&root);
        if !linked {
            return;
        }
        assert!(creds.is_none());
        assert!(through_link.is_none());
    }

    #[test]
    fn component_credential_replace_updates_regular_file_and_refuses_symlinks() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-creds-replace-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        fs::create_dir_all(&root).expect("temp dir");
        fs::write(root.join(".credentials.json"), sample_claude_credentials())
            .expect("credentials");
        let mut creds = read_claude_credentials(&root).expect("readable");
        creds.access_token = "sk-ant-refreshed".to_owned();
        creds.refresh_token = Some("refresh-token".to_owned());
        creds.expires_at_ms = Some(4102444800000);
        assert!(persist_claude_credentials(&root, &creds));
        let stored = read_claude_credentials(&root).expect("replaced");
        assert_eq!(stored.access_token, "sk-ant-refreshed");
        assert_eq!(stored.refresh_token.as_deref(), Some("refresh-token"));

        let linked_root = root.join("linked");
        fs::create_dir_all(&linked_root).expect("linked dir");
        let target = linked_root.join("elsewhere.json");
        fs::write(&target, sample_claude_credentials()).expect("target");
        let link = linked_root.join(".credentials.json");
        let linked = std::os::windows::fs::symlink_file(&target, &link).is_ok();
        let persist_link = persist_claude_credentials(&linked_root, &creds);
        let target_after = fs::read_to_string(&target).ok();
        let _ = fs::remove_dir_all(&root);
        if linked {
            assert!(!persist_link);
            assert_eq!(
                target_after.as_deref(),
                Some(sample_claude_credentials().as_str())
            );
        }
    }

    #[test]
    fn component_codex_turn_then_token_count_uses_last_token_usage() {
        assert_eq!(
            parse_codex_model(r#"{"type":"turn_context","payload":{"model":"gpt-5.4"}}"#)
                .as_deref(),
            Some("gpt-5.4")
        );
        let event = parse_codex_usage_line(
            r#"{"type":"event_msg","timestamp":"2026-08-16T01:02:03Z","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":20,"cached_input_tokens":5,"output_tokens":3}}}}"#,
            Some("gpt-5.4"),
        )
        .expect("token_count");
        assert_eq!(event.usage.input, 15);
        assert_eq!(event.usage.cached_input, 5);
        assert_eq!(event.usage.output, 3);

        let auto = parse_codex_usage_line(
            r#"{"type":"event_msg","timestamp":"2026-08-16T01:02:03Z","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":20,"cached_input_tokens":0,"output_tokens":3}}}}"#,
            Some("codex-auto-review"),
        )
        .expect("auto-review");
        assert_eq!(auto.model, "gpt-5.4");
    }

    #[test]
    fn component_codex_reasoning_tokens_are_not_added_on_top_of_output() {
        // Upstream last_token_usage: total_tokens = input + output.
        // reasoning_output_tokens is a breakdown of output, not an extra bucket.
        // Public numbers from openai/codex#5276 (no secrets).
        let event = parse_codex_usage_line(
            r#"{"type":"event_msg","timestamp":"2025-10-17T05:54:20.209Z","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":6245,"cached_input_tokens":5376,"output_tokens":407,"reasoning_output_tokens":320,"total_tokens":6652}}}}"#,
            Some("gpt-5.4"),
        )
        .expect("token_count");
        assert_eq!(event.usage.input, 869);
        assert_eq!(event.usage.cached_input, 5376);
        assert_eq!(event.usage.output, 407);
        assert_eq!(event.usage.processed_output_tokens(), 407);
    }

    #[test]
    fn component_long_context_classification_does_not_inflate_measured_tokens() {
        let event = parse_codex_usage_line(
            r#"{"type":"event_msg","timestamp":"2026-08-16T01:02:03Z","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":272001,"cached_input_tokens":1,"output_tokens":10}}}}"#,
            Some("gpt-5.4"),
        )
        .expect("token_count");
        assert_eq!(event.usage.input, 272_000);
        assert_eq!(event.usage.cached_input, 1);
        assert_eq!(event.usage.long_context_input, 272_000);
        assert_eq!(event.usage.processed_input_tokens(), 272_001);
        assert_eq!(event.usage.processed_output_tokens(), 10);
    }

    proptest::proptest! {
        #[test]
        fn pbt_codex_output_equals_output_tokens_not_output_plus_reasoning(
            output in 0u64..1_000_000u64,
            reasoning in 0u64..1_000_000u64,
        ) {
            let line = format!(
                r#"{{"type":"event_msg","timestamp":"2026-08-16T01:02:03Z","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":20,"cached_input_tokens":0,"output_tokens":{output},"reasoning_output_tokens":{reasoning}}}}}}}}}"#
            );
            let event = parse_codex_usage_line(&line, Some("gpt-5.4")).expect("token_count");
            proptest::prop_assert_eq!(event.usage.output, output);
        }
    }

    #[test]
    fn component_codex_gpt6_astra_today_cost_is_priced() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-astra-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let stamp = current_stamp();
        write_codex_session_with_model(
            &root,
            "rollout.jsonl",
            "gpt-6-astra",
            &[token_count_line(
                &stamp, 100_000, 0, 100_000, 100_000, 0, 100_000,
            )],
        );
        let usage = collect_codex(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.today_cents, 600);
        assert_eq!(usage.month_cents, 600);
        assert_eq!(usage.month_output_tokens, 100_000);
        assert_eq!(usage.month_input_tokens, 100_000);
    }

    #[test]
    fn component_codex_split_cost_rounds_once_across_restart() {
        let root = std::env::temp_dir().join(format!(
            "rundog-codex-precise-cost-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let store_root = root.join("store");
        let stamp = current_stamp();
        let first_lines = (1..=10_000)
            .map(|total| token_count_line(&stamp, 1, 0, 0, total, 0, 0))
            .collect::<Vec<_>>();
        let session = write_codex_session(&root, "split-cost.jsonl", &first_lines);

        let mut first = new_persisted_collector(&root, store_root.clone());
        assert_eq!(tick_idle(&mut first), UsageTick::Idle);
        assert_eq!(first.snapshot().codex.today_cost_nanos, 25_000_000);
        assert_eq!(first.snapshot().codex.today_cents, 3);

        let second_lines = (10_001..=20_000)
            .map(|total| token_count_line(&stamp, 1, 0, 0, total, 0, 0))
            .collect::<Vec<_>>();
        append_codex_lines(&session, &second_lines);
        let mut restored = new_persisted_collector(&root, store_root);
        assert_eq!(restored.snapshot().codex.today_cost_nanos, 25_000_000);
        assert_eq!(tick_idle(&mut restored), UsageTick::Idle);

        let usage = restored.snapshot().codex;
        fs::remove_dir_all(root).unwrap();
        assert_eq!(usage.today_cost_nanos, 50_000_000);
        assert_eq!(usage.month_cost_nanos, 50_000_000);
        assert_eq!(usage.today_cents, 5);
        assert_eq!(usage.month_cents, 5);
    }

    #[test]
    fn component_codex_unpriced_model_still_counts_tokens() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-unpriced-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let stamp = current_stamp();
        write_codex_session_with_model(
            &root,
            "rollout.jsonl",
            "mystery-model",
            &[token_count_line(&stamp, 80, 20, 5, 80, 20, 5)],
        );
        let usage = collect_codex(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.today_cents, 0);
        assert_eq!(usage.month_cents, 0);
        assert_eq!(usage.month_input_tokens, 80);
        assert_eq!(usage.month_output_tokens, 5);
        assert!(usage.has_month_activity());
    }

    #[test]
    fn component_codex_identical_snapshot_is_not_double_counted() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-snap-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let stamp = current_stamp();
        let turn = token_count_line(&stamp, 80, 20, 5, 80, 20, 5);
        write_codex_session(&root, "rollout.jsonl", &[turn.clone(), turn]);
        let usage = collect_codex(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.month_input_tokens, 80);
        assert_eq!(usage.month_output_tokens, 5);
    }

    #[test]
    fn component_codex_rate_limit_only_renotify_is_not_counted() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-limits-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let stamp = current_stamp();
        write_codex_session(
            &root,
            "rollout.jsonl",
            &[
                token_count_line(&stamp, 80, 20, 5, 80, 20, 5),
                token_count_with_limits(&stamp, 80, 20, 5, 80, 20, 5),
            ],
        );
        let usage = collect_codex(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.month_input_tokens, 80);
        assert_eq!(usage.month_output_tokens, 5);
    }

    #[test]
    fn component_codex_cumulative_usage_counts_last_turn_only() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-cum-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let stamp = current_stamp();
        write_codex_session(
            &root,
            "rollout.jsonl",
            &[
                token_count_line(&stamp, 80, 20, 5, 80, 20, 5),
                token_count_line(&stamp, 40, 10, 2, 120, 30, 7),
            ],
        );
        let usage = collect_codex(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.month_input_tokens, 120);
        assert_eq!(usage.month_output_tokens, 7);
    }

    #[test]
    fn component_codex_replay_after_idle_does_not_add_tokens() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-replay-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let stamp = current_stamp();
        let session = write_codex_session(
            &root,
            "rollout.jsonl",
            &[token_count_line(&stamp, 80, 20, 5, 80, 20, 5)],
        );
        let mut collector = new_claude_collector(&root);
        assert_eq!(tick_idle(&mut collector), UsageTick::Idle);
        assert_eq!(collector.snapshot().codex.month_input_tokens, 80);
        append_codex_lines(&session, &[token_count_line(&stamp, 80, 20, 5, 80, 20, 5)]);
        collector.test_force_restat();
        assert_eq!(tick_idle(&mut collector), UsageTick::Idle);
        let usage = collector.snapshot().codex;
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.month_input_tokens, 80);
        assert_eq!(usage.month_output_tokens, 5);
    }

    #[test]
    fn component_codex_fork_inherited_snapshot_is_baseline_only() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-fork-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let stamp = current_stamp();
        write_codex_session(
            &root,
            "rollout.jsonl",
            &[
                token_count_line(&stamp, 80, 20, 5, 160, 40, 10),
                token_count_line(&stamp, 40, 0, 3, 200, 40, 13),
            ],
        );
        let usage = collect_codex(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.month_input_tokens, 40);
        assert_eq!(usage.month_output_tokens, 3);
    }

    #[test]
    fn component_codex_resume_renotify_is_replay() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-resume-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let stamp = current_stamp();
        write_codex_session(
            &root,
            "rollout.jsonl",
            &[
                token_count_line(&stamp, 80, 20, 5, 80, 20, 5),
                token_count_line(&stamp, 80, 20, 5, 80, 20, 5),
            ],
        );
        let usage = collect_codex(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.month_input_tokens, 80);
        assert_eq!(usage.month_output_tokens, 5);
    }

    #[test]
    fn component_codex_process_restart_keeps_watermark() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-restart-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let store_root = root.join("store");
        let stamp = current_stamp();
        let session = write_codex_session(
            &root,
            "rollout.jsonl",
            &[token_count_line(&stamp, 80, 20, 5, 80, 20, 5)],
        );
        let mut first = new_persisted_collector(&root, store_root.clone());
        assert_eq!(tick_idle(&mut first), UsageTick::Idle);
        assert_eq!(first.snapshot().codex.month_input_tokens, 80);
        assert_eq!(first.test_last_model(&session).as_deref(), Some("gpt-5.4"));
        assert_eq!(first.test_codex_total(&session), Some((80, 20, 5)));
        append_codex_lines(&session, &[token_count_line(&stamp, 40, 10, 2, 120, 30, 7)]);
        let mut restored = new_persisted_collector(&root, store_root);
        assert_eq!(restored.snapshot().codex.month_input_tokens, 80);
        assert_eq!(tick_idle(&mut restored), UsageTick::Idle);
        let usage = restored.snapshot().codex;
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.month_input_tokens, 120);
        assert_eq!(usage.month_output_tokens, 7);
    }

    #[test]
    fn component_codex_restart_between_turn_context_and_token_count_keeps_model() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-gap-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let store_root = root.join("store");
        let session = current_codex_session(&root, "rollout.jsonl");
        fs::write(&session, format!("{TURN_CONTEXT}\n")).expect("turn only");
        let mut first = new_persisted_collector(&root, store_root.clone());
        assert_eq!(tick_idle(&mut first), UsageTick::Idle);
        assert_eq!(first.snapshot().codex.month_input_tokens, 0);
        assert_eq!(first.test_last_model(&session).as_deref(), Some("gpt-5.4"));
        append_codex_lines(
            &session,
            &[token_count_line(&current_stamp(), 80, 20, 5, 80, 20, 5)],
        );
        let mut restored = new_persisted_collector(&root, store_root);
        assert_eq!(tick_idle(&mut restored), UsageTick::Idle);
        let usage = restored.snapshot().codex;
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.month_input_tokens, 80);
        assert_eq!(usage.month_output_tokens, 5);
    }

    #[test]
    fn component_codex_out_of_order_snapshot_is_treated_as_reset_not_a_defect() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-ooo-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let stamp = current_stamp();
        write_codex_session(
            &root,
            "rollout.jsonl",
            &[
                token_count_line(&stamp, 80, 20, 5, 80, 20, 5),
                token_count_line(&stamp, 40, 10, 2, 120, 30, 7),
                token_count_line(&stamp, 80, 20, 5, 80, 20, 5),
            ],
        );
        let usage = collect_codex(&root);
        let _ = fs::remove_dir_all(&root);
        // Smaller total after a larger one cannot be distinguished from a reset.
        assert_eq!(usage.month_input_tokens, 200);
        assert_eq!(usage.month_output_tokens, 12);
    }

    #[test]
    fn component_codex_counter_reset_counts_new_epoch() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-reset-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let stamp = current_stamp();
        write_codex_session(
            &root,
            "rollout.jsonl",
            &[
                token_count_line(&stamp, 80, 0, 5, 80, 0, 5),
                token_count_line(&stamp, 10, 0, 1, 10, 0, 1),
            ],
        );
        let usage = collect_codex(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.month_input_tokens, 90);
        assert_eq!(usage.month_output_tokens, 6);
    }

    #[test]
    fn component_codex_same_tuple_different_request_counts_twice() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-tuple-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let stamp = current_stamp();
        write_codex_session(
            &root,
            "rollout.jsonl",
            &[
                token_count_line(&stamp, 80, 20, 5, 80, 20, 5),
                token_count_line(&stamp, 80, 20, 5, 160, 40, 10),
            ],
        );
        let usage = collect_codex(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.month_input_tokens, 160);
        assert_eq!(usage.month_output_tokens, 10);
    }

    #[test]
    fn component_codex_missing_total_counts_each_last_without_replay_claim() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-nototal-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let stamp = current_stamp();
        write_codex_session(
            &root,
            "rollout.jsonl",
            &[
                token_count_line_no_total(&stamp, 80, 20, 5),
                token_count_line_no_total(&stamp, 80, 20, 5),
            ],
        );
        let usage = collect_codex(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.month_input_tokens, 160);
        assert_eq!(usage.month_output_tokens, 10);
    }

    #[test]
    fn component_codex_old_month_dir_with_today_event_is_still_counted() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-oldmonth-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let now = unix_now_ms();
        let window = super::day_window(now);
        let (year, month, day) = local_ymd(now, window.bias_minutes);
        let (hour, minute) = local_hms(now, window.bias_minutes);
        let mut old_year = year;
        let mut old_month = month;
        for _ in 0..3 {
            let (next_year, next_month) = super::previous_month(old_year, old_month);
            old_year = next_year;
            old_month = next_month;
        }
        let dir = super::codex_month_dir(&root.join("codex"), old_year, old_month);
        fs::create_dir_all(&dir).expect("old month");
        let stamp = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:00Z");
        fs::write(
            dir.join("rollout.jsonl"),
            format!(
                "{TURN_CONTEXT}\n{}\n",
                token_count_line(&stamp, 80, 20, 5, 80, 20, 5)
            ),
        )
        .expect("jsonl");
        let usage = collect_codex(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.month_input_tokens, 80);
        assert_eq!(usage.month_output_tokens, 5);
    }

    #[test]
    fn component_codex_cold_month_dir_is_not_queued_for_history_scan() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-cold-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let dir = super::codex_month_dir(&root, 2024, 1);
        fs::create_dir_all(&dir).expect("cold month");
        let mtime = super::path_mtime_ms(&dir).expect("mtime");
        let mut discover = std::collections::VecDeque::new();
        super::queue_recent_codex_month_dirs(
            &mut discover,
            &root,
            2024,
            mtime.saturating_add(super::HOT_AGE_MS + 1),
            super::HOT_AGE_MS,
        );
        let _ = fs::remove_dir_all(&root);
        assert!(!discover.iter().any(|path| path == &dir));
    }

    #[test]
    fn component_codex_known_cold_file_append_is_restatted() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-coldfile-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let stamp = current_stamp();
        let session = write_codex_session(
            &root,
            "rollout.jsonl",
            &[token_count_line(&stamp, 80, 20, 5, 80, 20, 5)],
        );
        let mut collector = new_claude_collector(&root);
        assert_eq!(tick_idle(&mut collector), UsageTick::Idle);
        assert_eq!(collector.snapshot().codex.month_input_tokens, 80);
        collector.test_mark_file_cold(&session);
        append_codex_lines(&session, &[token_count_line(&stamp, 40, 10, 2, 120, 30, 7)]);
        assert_eq!(tick_idle(&mut collector), UsageTick::Idle);
        let usage = collector.snapshot().codex;
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.month_input_tokens, 120);
        assert_eq!(usage.month_output_tokens, 7);
    }

    #[test]
    fn component_checkpoint_path_rejects_parent_segments() {
        let root = Path::new(r"C:\tmp\codex");
        let escaped = super::checkpoint_disk_path(
            Path::new(r"C:\tmp\claude"),
            root,
            &crate::core::FileCheckpointKey::Codex("../secret.jsonl".to_owned()),
        );
        assert_eq!(escaped, root.join("secret.jsonl"));
        let slash_escape = super::checkpoint_disk_path(
            Path::new(r"C:\tmp\claude"),
            root,
            &crate::core::FileCheckpointKey::Codex(r"..\..\Windows\secret.jsonl".to_owned()),
        );
        assert_eq!(slash_escape, root.join("Windows").join("secret.jsonl"));
    }

    #[test]
    fn component_codex_year_before_walk_window_is_not_scanned() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-oldyear-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let stamp = current_stamp();
        write_old_year_codex_session(
            &root,
            year_outside_walk_window(),
            "rollout.jsonl",
            &[token_count_line(&stamp, 80, 20, 5, 80, 20, 5)],
        );
        let usage = collect_codex(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.month_input_tokens, 0);
        assert_eq!(usage.month_output_tokens, 0);
    }

    #[test]
    fn component_codex_known_file_outside_walk_window_is_reconciled() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-knownold-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let stamp = current_stamp();
        let year = year_outside_walk_window();
        write_old_year_codex_session(
            &root,
            year,
            "rollout.jsonl",
            &[token_count_line(&stamp, 80, 20, 5, 80, 20, 5)],
        );
        let mut collector = new_claude_collector(&root);
        collector.test_remember_codex(&format!("sessions/{year}/01/rollout.jsonl"));
        assert_eq!(tick_idle(&mut collector), UsageTick::Idle);
        let usage = collector.snapshot().codex;
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.month_input_tokens, 80);
        assert_eq!(usage.month_output_tokens, 5);
    }

    #[test]
    fn component_codex_known_old_session_restart_sees_append() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-codex-oldrestart-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let store_root = root.join("store");
        let stamp = current_stamp();
        let year = year_outside_walk_window();
        let session = write_old_year_codex_session(
            &root,
            year,
            "rollout.jsonl",
            &[token_count_line(&stamp, 80, 20, 5, 80, 20, 5)],
        );
        let mut first = new_persisted_collector(&root, store_root.clone());
        first.test_remember_codex(&format!("sessions/{year}/01/rollout.jsonl"));
        assert_eq!(tick_idle(&mut first), UsageTick::Idle);
        assert_eq!(first.snapshot().codex.month_input_tokens, 80);
        append_codex_lines(&session, &[token_count_line(&stamp, 40, 10, 2, 120, 30, 7)]);
        let mut restored = new_persisted_collector(&root, store_root);
        assert_eq!(tick_idle(&mut restored), UsageTick::Idle);
        let usage = restored.snapshot().codex;
        let _ = fs::remove_dir_all(&root);
        assert_eq!(usage.month_input_tokens, 120);
        assert_eq!(usage.month_output_tokens, 7);
    }

    #[test]
    fn component_claude_failure_does_not_block_codex_apply() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-fetch-isol-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let mut collector = new_claude_collector(&root);
        assert!(collector.test_complete_fetch(
            crate::core::ProviderFetchKind::Codex,
            Some(sample_limit_usage(210)),
            None,
        ));
        assert!(!collector.test_complete_fetch(
            crate::core::ProviderFetchKind::Claude,
            None,
            Some(crate::core::FetchErrorKind::HttpStatus),
        ));
        let snap = collector.snapshot();
        let _ = fs::remove_dir_all(&root);
        assert_eq!(snap.codex.primary.unwrap().used_tenths, 210);
        assert!(snap.claude.primary.is_none());
        assert_eq!(
            collector.test_fetch_freshness(crate::core::ProviderFetchKind::Claude),
            crate::core::LimitsFreshness::Failed
        );
        assert_eq!(
            collector.test_fetch_freshness(crate::core::ProviderFetchKind::Codex),
            crate::core::LimitsFreshness::Current
        );
    }

    #[test]
    fn component_local_jsonl_limits_keep_expired_percent_for_presentation() {
        let expired = crate::core::LimitWindow {
            used_tenths: 280,
            resets_at_ms: 1_000,
            window_minutes: 300,
        };
        assert_eq!(expired.effective(2_000).used_tenths, 0);
        assert_eq!(expired.used_tenths, 280);
        assert!(!expired.is_current(2_000));
        assert_eq!(
            crate::core::format_limit_label("5h", Some(expired), 2_000),
            "5h: —"
        );
        assert_ne!(
            crate::core::format_limit_label("5h", Some(expired), 2_000),
            "5h: 0%"
        );
    }

    #[test]
    fn component_failed_fetch_does_not_write_zero_percent() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-fetch-zero-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let mut collector = new_claude_collector(&root);
        assert!(collector.test_complete_fetch(
            crate::core::ProviderFetchKind::Claude,
            Some(sample_limit_usage(410)),
            None,
        ));
        assert!(!collector.test_complete_fetch(
            crate::core::ProviderFetchKind::Claude,
            None,
            Some(crate::core::FetchErrorKind::Transport),
        ));
        let window = collector.snapshot().claude.primary.unwrap();
        let _ = fs::remove_dir_all(&root);
        assert_eq!(window.used_tenths, 410);
        assert_ne!(window.used_tenths, 0);
        assert_eq!(
            collector.test_fetch_freshness(crate::core::ProviderFetchKind::Claude),
            crate::core::LimitsFreshness::Stale
        );
    }

    #[test]
    fn component_late_fetch_after_cancel_is_rejected() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-fetch-late-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let mut collector = new_claude_collector(&root);
        let now = unix_now_ms();
        let generation = crate::core::start_fetch(&mut collector.claude_fetch.state, now);
        collector.cancel_remote_fetches();
        let applied = collector.apply_remote_outcome(
            crate::core::ProviderFetchKind::Claude,
            super::SlotOutcome {
                generation,
                usage: Some(sample_limit_usage(990)),
                error: None,
                observed_at_ms: now,
            },
            now,
        );
        let _ = fs::remove_dir_all(&root);
        assert!(!applied);
        assert!(collector.snapshot().claude.primary.is_none());
    }

    #[test]
    fn component_spawn_failure_clears_inflight() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-fetch-spawn-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let mut collector = new_claude_collector(&root);
        collector.test_mark_spawn_failure(crate::core::ProviderFetchKind::Codex);
        let _ = fs::remove_dir_all(&root);
        assert!(!collector.codex_fetch.state.in_flight);
        assert_eq!(
            collector.codex_fetch.state.last_error,
            Some(crate::core::FetchErrorKind::SpawnFailed)
        );
        assert!(crate::core::should_start_fetch(
            &collector.codex_fetch.state,
            unix_now_ms() + crate::core::FETCH_BACKOFF_INITIAL_MS,
            super::CODEX_LIMITS_PERIOD_MS,
        ));
    }

    #[test]
    fn component_diagnostics_are_bounded_and_secret_free() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-diag-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let store_root = root.join("store");
        write_codex_session(
            &root,
            "rollout.jsonl",
            &[token_count_line(&current_stamp(), 80, 20, 5, 80, 20, 5)],
        );
        let mut collector = new_persisted_collector(&root, store_root);
        assert_eq!(tick_idle(&mut collector), UsageTick::Idle);
        collector.test_complete_fetch(
            crate::core::ProviderFetchKind::Claude,
            None,
            Some(crate::core::FetchErrorKind::AuthMissing),
        );
        let snapshot = collector.diagnostics();
        let rendered = format!("{snapshot:?}");
        let _ = fs::remove_dir_all(&root);
        assert!(snapshot.known_files >= 1);
        assert!(snapshot.usage_parse_bytes > 0);
        assert_eq!(
            snapshot.checkpoint_schema,
            u64::from(crate::core::USAGE_STATE_SCHEMA_VERSION)
        );
        assert_eq!(snapshot.startup_mode, crate::core::StartupMode::Test as u64);
        assert!(!rendered.contains("Bearer"));
        assert!(!rendered.contains("Authorization"));
        assert!(!rendered.contains("sk-"));
        assert!(!rendered.contains("prompt"));
        assert!(!rendered.contains("refresh_token"));
    }

    #[test]
    fn component_codex_and_claude_limit_payloads_round_trip() {
        let codex = parse_codex_limits_line(
            r#"{"timestamp":"2026-08-16T01:02:03Z","payload":{"rate_limits":{"plan_type":"pro","primary":{"used_percent":5.0,"resets_at":1780000000,"window_minutes":300},"secondary":{"used_percent":21.5,"resets_at":1780500000,"window_minutes":10080}}}}"#,
        )
        .expect("codex limits");
        assert_eq!(codex.plan_label().as_deref(), Some("ChatGPT Pro"));
        assert_eq!(codex.primary.unwrap().used_tenths, 50);
        assert_eq!(codex.secondary.unwrap().window_minutes, 10_080);

        let claude = parse_claude_usage_response(
            r#"{"five_hour":{"utilization":28.4,"resets_at":"2026-08-16T07:39:00Z"},"seven_day":{"utilization":13,"resets_at":"2026-08-18T00:00:00Z"}}"#,
            Some("max"),
        )
        .expect("claude limits");
        assert_eq!(claude.plan_label().as_deref(), Some("Max"));
        assert_eq!(claude.primary.unwrap().used_tenths, 284);
        assert_eq!(claude.secondary.unwrap().window_minutes, 10_080);

        let max_20x = parse_claude_usage_response(
            r#"{"five_hour":{"utilization":1.0},"seven_day":{"utilization":34.0}}"#,
            Some("default_claude_max_20x"),
        )
        .expect("claude max 20x");
        assert_eq!(max_20x.plan_label().as_deref(), Some("Max 20x"));
        assert!(max_20x.fable.is_none());
    }

    #[test]
    fn component_claude_fable_model_scoped_is_not_the_weekly_all_models_window() {
        let usage = parse_claude_usage_response(
            r#"{"five_hour":{"utilization":61.0,"resets_at":"2026-08-16T07:39:00Z"},"seven_day":{"utilization":40.0,"resets_at":"2026-08-18T00:00:00Z"},"model_scoped":[{"display_name":"Fable","utilization":41.0,"resets_at":"2026-08-18T00:00:00Z"}]}"#,
            Some("max"),
        )
        .expect("claude with fable");
        assert_eq!(usage.secondary.unwrap().used_tenths, 400);
        assert_eq!(usage.fable.unwrap().used_tenths, 410);
        assert_eq!(usage.fable.unwrap().window_minutes, 10_080);
        assert_eq!(
            usage.fable.unwrap().resets_at_ms,
            usage.secondary.unwrap().resets_at_ms
        );
    }

    #[test]
    fn component_claude_fable_weekly_scoped_limit_uses_percent() {
        let usage = parse_claude_usage_response(
            r#"{"five_hour":{"utilization":8.0},"seven_day":{"utilization":2.0},"limits":[{"kind":"weekly_scoped","percent":65.0,"resets_at":"2026-07-31T23:59:59Z","scope":{"model":{"display_name":"Fable"}},"is_active":true}]}"#,
            None,
        )
        .expect("weekly_scoped fable");
        assert_eq!(usage.fable.unwrap().used_tenths, 650);
        assert_eq!(usage.secondary.unwrap().used_tenths, 20);
    }

    #[test]
    fn component_claude_absent_fable_is_not_invented() {
        let usage = parse_claude_usage_response(
            r#"{"five_hour":{"utilization":10.0},"seven_day":{"utilization":20.0},"model_scoped":[{"display_name":"Opus","utilization":90.0}],"limits":[{"kind":"weekly_scoped","percent":12.0,"scope":{"model":{"display_name":"Sonnet"}}}]}"#,
            None,
        )
        .expect("no fable");
        assert!(usage.fable.is_none());
        assert_eq!(usage.secondary.unwrap().used_tenths, 200);
    }

    #[test]
    fn component_claude_seven_day_fable_field_is_accepted_when_present() {
        let usage = parse_claude_usage_response(
            r#"{"five_hour":{"utilization":1.0},"seven_day":{"utilization":2.0},"seven_day_fable":{"utilization":55.0,"resets_at":"2026-08-18T00:00:00Z"}}"#,
            None,
        )
        .expect("seven_day_fable");
        assert_eq!(usage.fable.unwrap().used_tenths, 550);
    }

    #[test]
    fn component_codex_spark_extra_limit_is_not_the_subscription_window() {
        let spark = parse_codex_limits_line(
            r#"{"payload":{"rate_limits":{"limit_name":"GPT-5.3-Codex-Spark","plan_type":"pro","primary":{"used_percent":0.0,"window_minutes":10080,"resets_at":1787381403},"secondary":null}}}"#,
        )
        .expect("spark extra limit");
        assert!(!is_subscription_limits(&spark));
        assert!(is_subscription_limits(&parse_codex_limits_line(
            r#"{"payload":{"rate_limits":{"plan_type":"pro","primary":{"used_percent":5.0,"window_minutes":300,"resets_at":1780000000},"secondary":{"used_percent":21.5,"window_minutes":10080,"resets_at":1780500000}}}}"#,
        )
        .expect("subscription windows")));
    }

    #[test]
    fn component_codex_jsonl_banked_reset_count_is_optional() {
        let with_credits = parse_codex_limits_line(
            r#"{"payload":{"rate_limits":{"plan_type":"plus","primary":{"used_percent":5.0,"window_minutes":300,"resets_at":1780000000},"secondary":{"used_percent":4.0,"window_minutes":10080,"resets_at":1780500000},"rate_limit_reset_credits":{"available_count":3}}}}"#,
        )
        .expect("jsonl banked");
        assert_eq!(with_credits.banked_reset_available, Some(3));
        let without = parse_codex_limits_line(
            r#"{"payload":{"rate_limits":{"plan_type":"plus","primary":{"used_percent":5.0,"window_minutes":300,"resets_at":1780000000}}}}"#,
        )
        .expect("jsonl windows");
        assert!(without.banked_reset_available.is_none());
    }

    #[test]
    fn component_chatgpt_wham_usage_maps_primary_and_weekly_windows() {
        let usage = parse_wham_usage_response(
            r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":34,"limit_window_seconds":18000,"reset_at":1778091218},"secondary_window":{"used_percent":37,"limit_window_seconds":604800,"reset_at":1778605571}}}"#,
        )
        .expect("wham usage");
        assert_eq!(usage.plan_label().as_deref(), Some("ChatGPT Pro"));
        assert_eq!(usage.primary.unwrap().window_minutes, 300);
        assert_eq!(usage.primary.unwrap().used_tenths, 340);
        assert_eq!(usage.secondary.unwrap().window_minutes, 10_080);
        assert_eq!(usage.secondary.unwrap().used_tenths, 370);
        assert!(usage.banked_reset_available.is_none());
    }

    #[test]
    fn component_chatgpt_wham_usage_reads_banked_reset_count() {
        let usage = parse_wham_usage_response(
            r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":27,"limit_window_seconds":18000,"reset_at":1782770922},"secondary_window":{"used_percent":4,"limit_window_seconds":604800,"reset_at":1783357722}},"rate_limit_reset_credits":{"available_count":2}}"#,
        )
        .expect("wham with banked resets");
        assert_eq!(usage.plan_label().as_deref(), Some("ChatGPT Plus"));
        assert_eq!(usage.banked_reset_available, Some(2));
        assert_eq!(usage.primary.unwrap().used_tenths, 270);
        assert_eq!(usage.secondary.unwrap().used_tenths, 40);
    }

    #[test]
    fn component_chatgpt_wham_usage_does_not_invent_banked_reset() {
        assert_eq!(
            parse_wham_usage_response(
                r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":27,"limit_window_seconds":18000,"reset_at":1782770922}}}"#,
            )
            .expect("windows only")
            .banked_reset_available,
            None
        );
        assert_eq!(
            parse_wham_usage_response(r#"{"rate_limit_reset_credits":{"available_count":0}}"#)
                .expect("zero banked")
                .banked_reset_available,
            Some(0)
        );
    }

    #[test]
    fn component_catch_up_window_keeps_current_month_and_drops_epoch() {
        let window = super::day_window(unix_now_ms());
        assert!(is_current_month(unix_now_ms(), window));
        assert!(!is_current_month(0, window));
    }

    #[test]
    fn component_deleted_pending_log_releases_catch_up_and_can_resume() {
        let root = std::env::temp_dir().join(format!(
            "rundog-deleted-pending-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let session = claude_session(&root);
        let stamp = current_stamp();
        let body: String = (0..8)
            .map(|n| {
                format!(
                    "{}\n",
                    claude_usage_line_with_len(&format!("event-{n}"), &stamp, 200_000)
                )
            })
            .collect();
        fs::write(&session, &body).unwrap();
        let mut collector = new_claude_collector(&root);
        assert_eq!(collector.tick(ptr::null_mut()), UsageTick::MoreWork);
        let collected = collector.snapshot().claude.month_input_tokens;
        let offset = collector.test_file_offset(&session).unwrap();
        assert!(offset > 0 && offset < body.len() as u64);
        fs::remove_file(&session).unwrap();
        let _ = collector.tick(ptr::null_mut());
        assert!(!collector.snapshot().month_scan_in_progress);
        assert!(collector.pending.is_empty());
        assert_eq!(collector.test_file_offset(&session), Some(offset));
        assert_eq!(collector.snapshot().claude.month_input_tokens, collected);
        // Reappearance must resume without counting the already collected IDs.
        fs::write(&session, body).unwrap();
        for _ in 0..32 {
            collector.scan_file(
                &session,
                super::day_window(unix_now_ms()),
                unix_now_ms() + super::COLD_RESTAT_MS,
            );
        }
        assert_eq!(collector.snapshot().claude.month_input_tokens, 8_000_000);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn component_empty_snapshot_catch_up_reads_current_month_jsonl() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-usage-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let project = root.join("claude").join("projects").join("p1");
        fs::create_dir_all(&project).expect("temp project");
        let now = unix_now_ms();
        let (year, month, day) = local_ymd(now, 0);
        let (hour, minute) = local_hms(now, 0);
        let line = format!(
            r#"{{"type":"assistant","timestamp":"{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:00Z","requestId":"r1","message":{{"id":"m1","model":"claude-opus-5","usage":{{"input_tokens":1000000,"output_tokens":0}}}}}}"#
        );
        fs::write(project.join("session.jsonl"), format!("{line}\n")).expect("jsonl");

        let mut collector =
            UsageCollector::with_dirs(root.join("claude"), root.join("codex"), false);
        assert_eq!(collector.snapshot().claude.month_cents, 0);
        let mut last = UsageTick::MoreWork;
        for _ in 0..16 {
            last = collector.tick(ptr::null_mut());
            if last == UsageTick::Idle && collector.snapshot().claude.month_cents > 0 {
                break;
            }
        }
        let _ = fs::remove_dir_all(&root);
        assert_eq!(last, UsageTick::Idle);
        assert_eq!(collector.snapshot().claude.month_cents, 500);
        assert_eq!(collector.snapshot().claude.month_input_tokens, 1_000_000);
        assert_eq!(collector.snapshot().claude.month_output_tokens, 0);
        assert!(!collector.catch_up);
    }

    #[test]
    fn component_oversized_jsonl_line_does_not_block_later_usage() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-usage-long-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let project = root.join("claude").join("projects").join("p1");
        fs::create_dir_all(&project).expect("temp project");
        let now = unix_now_ms();
        let (year, month, day) = local_ymd(now, 0);
        let (hour, minute) = local_hms(now, 0);
        let timestamp = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:00Z");
        let event = |id: &str| {
            format!(
                r#"{{"type":"assistant","timestamp":"{timestamp}","requestId":"{id}","message":{{"id":"{id}","model":"claude-opus-5","usage":{{"input_tokens":1000000,"output_tokens":0}}}}}}"#
            )
        };
        let blob = "x".repeat(600_000);
        let jsonl = format!(
            "{}\n{{\"type\":\"user\",\"blob\":\"{blob}\"}}\n{}\n",
            event("a"),
            event("b")
        );
        fs::write(project.join("session.jsonl"), jsonl).expect("jsonl");

        let mut collector =
            UsageCollector::with_dirs(root.join("claude"), root.join("codex"), false);
        let mut last = UsageTick::MoreWork;
        for _ in 0..64 {
            last = collector.tick(ptr::null_mut());
            if last == UsageTick::Idle && collector.snapshot().claude.month_cents == 1_000 {
                break;
            }
        }
        let _ = fs::remove_dir_all(&root);
        assert_eq!(last, UsageTick::Idle);
        assert_eq!(collector.snapshot().claude.month_cents, 1_000);
        assert!(!collector.catch_up);
    }

    #[test]
    fn component_checkpoint_restore_reads_only_appended_jsonl() {
        use super::{
            day_window, file_checkpoint_key, parse_timestamp, unix_now_ms, SourceKind,
            UsageCollector, UsageTick,
        };
        use crate::core::{FileCheckpointCursor, UsageCheckpoint};
        use std::collections::HashMap;

        let root = std::env::temp_dir().join(format!(
            "run-dog-usage-checkpoint-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let claude_dir = root.join("claude");
        let codex_home = root.join("codex");
        let project = claude_dir.join("projects").join("p1");
        fs::create_dir_all(&project).expect("temp project");
        let session = project.join("session.jsonl");
        let now = unix_now_ms();
        let (year, month, day) = local_ymd(now, 0);
        let (hour, minute) = local_hms(now, 0);
        let timestamp = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:00Z");
        let timestamp_ms = parse_timestamp(&timestamp).expect("timestamp");
        let event = |id: &str| {
            format!(
                r#"{{"type":"assistant","timestamp":"{timestamp}","requestId":"{id}","message":{{"id":"{id}","model":"claude-opus-5","usage":{{"input_tokens":1000000,"output_tokens":0}}}}}}"#
            )
        };
        fs::write(&session, format!("{}\n", event("a"))).expect("jsonl");

        let mut collector =
            UsageCollector::with_dirs(claude_dir.clone(), codex_home.clone(), false);
        let mut last = UsageTick::MoreWork;
        for _ in 0..16 {
            last = collector.tick(ptr::null_mut());
            if last == UsageTick::Idle && collector.snapshot().claude.month_cents > 0 {
                break;
            }
        }
        assert_eq!(last, UsageTick::Idle);
        assert_eq!(collector.snapshot().claude.month_cents, 500);
        assert_eq!(collector.snapshot().claude.month_input_tokens, 1_000_000);

        let window = day_window(now);
        let file_size = fs::metadata(&session).expect("session").len();
        let key = file_checkpoint_key(&session, &claude_dir, &codex_home, SourceKind::Claude)
            .expect("checkpoint key");
        let checkpoint = UsageCheckpoint {
            month_start: window.month_start,
            today: window.today,
            last_collected_ms: timestamp_ms,
            catch_up_done: true,
            snapshot: collector.snapshot(),
            files: HashMap::from([(
                key,
                FileCheckpointCursor {
                    offset: file_size,
                    size: file_size,
                },
            )]),
        };

        fs::OpenOptions::new()
            .append(true)
            .open(&session)
            .and_then(|mut file| {
                use std::io::Write;
                file.write_all(format!("{}\n", event("b")).as_bytes())
            })
            .expect("append");

        let mut restored = UsageCollector::with_dirs(claude_dir.clone(), codex_home.clone(), false);
        restored.apply_checkpoint(window, checkpoint);
        last = UsageTick::MoreWork;
        for _ in 0..16 {
            last = restored.tick(ptr::null_mut());
            if last == UsageTick::Idle && restored.snapshot().claude.month_cents == 1_000 {
                break;
            }
        }
        let _ = fs::remove_dir_all(&root);
        assert_eq!(last, UsageTick::Idle);
        assert_eq!(restored.snapshot().claude.month_cents, 1_000);
        assert_eq!(restored.snapshot().claude.month_input_tokens, 2_000_000);
        assert!(!restored.catch_up);
    }

    #[test]
    fn component_rescan_current_month_rebuilds_totals_from_jsonl() {
        use super::{unix_now_ms, UsageCollector, UsageTick};
        use crate::core::local_ymd;

        let root = std::env::temp_dir().join(format!(
            "run-dog-usage-rescan-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let claude_dir = root.join("claude");
        let project = claude_dir.join("projects").join("p1");
        fs::create_dir_all(&project).expect("temp project");
        let now = unix_now_ms();
        let (year, month, day) = local_ymd(now, 0);
        let timestamp = format!("{year:04}-{month:02}-{day:02}T12:00:00Z");
        let line = format!(
            r#"{{"type":"assistant","timestamp":"{timestamp}","requestId":"a","message":{{"id":"a","model":"claude-opus-5","usage":{{"input_tokens":1000000,"output_tokens":0}}}}}}"#
        );
        fs::write(project.join("session.jsonl"), format!("{line}\n")).expect("jsonl");

        let mut collector =
            UsageCollector::with_dirs(claude_dir.clone(), root.join("codex"), false);
        for _ in 0..16 {
            if collector.tick(ptr::null_mut()) == UsageTick::Idle
                && collector.snapshot().claude.month_cents == 500
            {
                break;
            }
        }
        assert_eq!(collector.snapshot().claude.month_cents, 500);

        collector.snapshot.claude.month_cents = 12;
        collector.snapshot.claude.month_input_tokens = 0;
        collector.catch_up = false;
        collector.rescan_current_month();
        assert!(collector.catch_up);
        assert_eq!(collector.snapshot().claude.month_cents, 0);

        let mut last = UsageTick::MoreWork;
        for _ in 0..16 {
            last = collector.tick(ptr::null_mut());
            if last == UsageTick::Idle && collector.snapshot().claude.month_cents == 500 {
                break;
            }
        }
        let _ = fs::remove_dir_all(&root);
        assert_eq!(last, UsageTick::Idle);
        assert_eq!(collector.snapshot().claude.month_cents, 500);
        assert_eq!(collector.snapshot().claude.month_input_tokens, 1_000_000);
    }

    #[test]
    fn component_older_jsonl_still_counts_after_newer_file() {
        use super::{unix_now_ms, UsageCollector, UsageTick};
        use crate::core::local_ymd;

        let root = std::env::temp_dir().join(format!(
            "run-dog-usage-order-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let claude_dir = root.join("claude");
        let project = claude_dir.join("projects").join("p1");
        fs::create_dir_all(&project).expect("temp project");
        let now = unix_now_ms();
        let (year, month, day) = local_ymd(now, 0);
        let older_day = if day > 1 { day - 1 } else { day };
        let older_stamp = format!("{year:04}-{month:02}-{older_day:02}T01:00:00Z");
        let newer_stamp = format!("{year:04}-{month:02}-{day:02}T23:00:00Z");
        let event = |id: &str, stamp: &str| {
            format!(
                r#"{{"type":"assistant","timestamp":"{stamp}","requestId":"{id}","message":{{"id":"{id}","model":"claude-opus-5","usage":{{"input_tokens":1000000,"output_tokens":0}}}}}}"#
            )
        };
        fs::write(
            project.join("older.jsonl"),
            format!("{}\n", event("old", &older_stamp)),
        )
        .expect("older jsonl");
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(
            project.join("newer.jsonl"),
            format!("{}\n", event("new", &newer_stamp)),
        )
        .expect("newer jsonl");

        let mut collector = UsageCollector::with_dirs(claude_dir, root.join("codex"), false);
        let mut last = UsageTick::MoreWork;
        for _ in 0..32 {
            last = collector.tick(ptr::null_mut());
            if last == UsageTick::Idle && collector.snapshot().claude.month_cents == 1_000 {
                break;
            }
        }
        let cents = collector.snapshot().claude.month_cents;
        let _ = fs::remove_dir_all(&root);
        assert_eq!(last, UsageTick::Idle);
        assert_eq!(
            cents, 1_000,
            "newer-first scan must still count older files"
        );
    }

    #[test]
    #[ignore = "manual: scans installed Claude/Codex JSONL on this machine"]
    fn live_installed_jsonl_full_scan_round_trips_month_totals() {
        use std::path::PathBuf;

        let home = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_default();
        let claude_dir = std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".claude"));
        let codex_home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        if !claude_dir.join("projects").is_dir() && !codex_home.is_dir() {
            eprintln!("skip: no Claude/Codex directories");
            return;
        }

        let mut collector = UsageCollector::with_dirs(claude_dir, codex_home, false);
        let baseline = run_until_scan_idle(&mut collector, 20_000);
        eprintln!(
            "baseline: claude_month={}c codex_month={}c scan={}",
            baseline.claude.month_cents,
            baseline.codex.month_cents,
            baseline.month_scan_in_progress
        );

        collector.rescan_current_month();
        let reset = collector.snapshot();
        assert!(
            reset.month_scan_in_progress,
            "expected catch_up after rescan"
        );
        assert_eq!(reset.claude.month_cents, 0, "claude month should reset");
        assert_eq!(reset.codex.month_cents, 0, "codex month should reset");

        let rebuilt = run_until_scan_idle(&mut collector, 20_000);
        eprintln!(
            "rebuilt: claude_month={}c codex_month={}c claude_in={} claude_out={}",
            rebuilt.claude.month_cents,
            rebuilt.codex.month_cents,
            rebuilt.claude.month_input_tokens,
            rebuilt.claude.month_output_tokens,
        );
        assert!(
            !rebuilt.month_scan_in_progress,
            "scan did not finish within tick budget"
        );
        assert_eq!(
            rebuilt.claude.month_cents, baseline.claude.month_cents,
            "claude month should match after full rescan"
        );
        assert_eq!(
            rebuilt.codex.month_cents, baseline.codex.month_cents,
            "codex month should match after full rescan"
        );
    }

    #[test]
    fn component_reader_one_byte_at_a_time_does_not_commit_incomplete() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-jsonl-byte-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let session = claude_session(&root);
        let line = claude_usage_line("a", &current_stamp());
        let committed = 0_u64;
        for end in 1..=line.len() {
            fs::write(&session, &line[..end]).expect("prefix");
            let chunk = read_chunk(&session, committed, super::MAX_BYTES_PER_TICK);
            assert!(chunk.events.is_empty());
            assert_eq!(chunk.new_offset, committed);
        }
        fs::write(&session, format!("{line}\n")).expect("complete");
        let chunk = read_chunk(&session, 0, super::MAX_BYTES_PER_TICK);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(chunk.events.len(), 1);
        assert!(chunk.new_offset > 0);
    }

    #[test]
    fn component_reader_mid_json_and_utf8_do_not_advance() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-jsonl-mid-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let session = claude_session(&root);
        let line = claude_usage_line("a", &current_stamp());
        fs::write(&session, &line[..line.len() / 2]).expect("mid json");
        let mid = read_chunk(&session, 0, super::MAX_BYTES_PER_TICK);
        assert_eq!(mid.new_offset, 0);
        let dog = "犬";
        fs::write(&session, &dog.as_bytes()[..1]).expect("mid utf8");
        let utf8 = read_chunk(&session, 0, super::MAX_BYTES_PER_TICK);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(utf8.new_offset, 0);
        assert!(utf8.events.is_empty());
    }

    #[test]
    fn component_reader_record_over_tick_budget_is_still_counted() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-jsonl-budget-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let session = claude_session(&root);
        let line = claude_usage_line_with_len("a", &current_stamp(), 200_000);
        assert!(line.len() as u64 > super::MAX_BYTES_PER_TICK);
        assert!(line.len() <= super::MAX_PARSE_LINE);
        fs::write(&session, format!("{line}\n")).expect("long");
        let chunk = read_chunk(&session, 0, super::MAX_BYTES_PER_TICK);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(chunk.events.len(), 1);
    }

    #[test]
    fn component_reader_just_under_max_parse_line_is_counted() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-jsonl-undermax-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let session = claude_session(&root);
        let line = claude_usage_line_with_len("a", &current_stamp(), super::MAX_PARSE_LINE);
        assert_eq!(line.len(), super::MAX_PARSE_LINE);
        fs::write(&session, format!("{line}\n")).expect("under max");
        let chunk = read_chunk(&session, 0, super::MAX_BYTES_PER_TICK);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(chunk.events.len(), 1);
    }

    #[test]
    fn component_reader_oversize_then_good_line_is_counted() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-jsonl-oversize-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let session = claude_session(&root);
        let stamp = current_stamp();
        let huge = claude_usage_line_with_len("skip", &stamp, super::MAX_PARSE_LINE + 8);
        let good = claude_usage_line("b", &stamp);
        fs::write(&session, format!("{huge}\n{good}\n")).expect("oversize+good");
        let chunk = read_chunk(&session, 0, super::CATCH_UP_BYTES_PER_TICK);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(chunk.events.len(), 1);
        assert_eq!(chunk.events[0].usage.input, 1_000_000);
    }

    #[test]
    fn component_reader_malformed_and_crlf_keep_later_good_lines() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-jsonl-mixed-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let session = claude_session(&root);
        let stamp = current_stamp();
        let lf = claude_usage_line("lf", &stamp);
        let crlf = claude_usage_line("crlf", &stamp);
        fs::write(&session, format!("not-json\n{lf}\n{crlf}\r\n")).expect("mixed");
        let chunk = read_chunk(&session, 0, super::MAX_BYTES_PER_TICK);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(chunk.events.len(), 2);
    }

    #[test]
    fn component_reader_restart_during_partial_does_not_checkpoint_past_record() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-jsonl-restart-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let store_root = root.join("store");
        let session = claude_session(&root);
        let line = claude_usage_line("a", &current_stamp());
        fs::write(&session, &line[..line.len() / 2]).expect("partial");
        let mut first = new_persisted_collector(&root, store_root.clone());
        assert_eq!(tick_idle(&mut first), UsageTick::Idle);
        assert_eq!(first.snapshot().claude.month_cents, 0);
        assert_eq!(first.test_file_offset(&session), Some(0));
        fs::write(&session, format!("{line}\n")).expect("complete");
        let mut restored = new_persisted_collector(&root, store_root);
        assert_eq!(tick_idle(&mut restored), UsageTick::Idle);
        let cents = restored.snapshot().claude.month_cents;
        let _ = fs::remove_dir_all(&root);
        assert_eq!(cents, 500);
    }

    #[test]
    fn component_reader_truncate_rereads_without_in_process_double_count() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-jsonl-trunc-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let session = claude_session(&root);
        let stamp = current_stamp();
        fs::write(&session, format!("{}\n", claude_usage_line("a", &stamp))).expect("a");
        let mut collector = new_claude_collector(&root);
        assert_eq!(tick_idle(&mut collector), UsageTick::Idle);
        assert_eq!(collector.snapshot().claude.month_cents, 500);
        fs::write(&session, "").expect("truncate");
        collector.test_force_restat();
        assert_eq!(tick_idle(&mut collector), UsageTick::Idle);
        assert_eq!(
            collector.last_rebuild_reason,
            Some(CursorRebuildReason::SizeShrunk)
        );
        fs::write(&session, format!("{}\n", claude_usage_line("b", &stamp))).expect("b");
        collector.test_force_restat();
        assert_eq!(tick_idle(&mut collector), UsageTick::Idle);
        let cents = collector.snapshot().claude.month_cents;
        let _ = fs::remove_dir_all(&root);
        assert_eq!(cents, 1_000);
    }

    #[test]
    fn component_reader_same_path_replace_counts_new_ids() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-jsonl-replace-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let session = claude_session(&root);
        let stamp = current_stamp();
        let first = claude_usage_line_with_len("a", &stamp, 512);
        let second = claude_usage_line_with_len("b", &stamp, 512);
        assert_eq!(first.len(), second.len());
        fs::write(&session, format!("{first}\n")).expect("a");
        let mut collector = new_claude_collector(&root);
        assert_eq!(tick_idle(&mut collector), UsageTick::Idle);
        assert_eq!(collector.snapshot().claude.month_cents, 500);
        fs::write(&session, format!("{second}\n")).expect("same-size b");
        collector.test_force_restat();
        assert_eq!(tick_idle(&mut collector), UsageTick::Idle);
        let cents = collector.snapshot().claude.month_cents;
        let _ = fs::remove_dir_all(&root);
        assert_eq!(cents, 1_000);
    }

    #[test]
    fn component_reader_rename_recreate_does_not_double_same_id() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-jsonl-rename-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let session = claude_session(&root);
        let stamp = current_stamp();
        fs::write(&session, format!("{}\n", claude_usage_line("a", &stamp))).expect("a");
        let mut collector = new_claude_collector(&root);
        assert_eq!(tick_idle(&mut collector), UsageTick::Idle);
        let archived = session.with_file_name("archived.jsonl");
        fs::rename(&session, &archived).expect("rename");
        fs::write(&session, format!("{}\n", claude_usage_line("a", &stamp))).expect("recreate a");
        collector.test_force_restat();
        assert_eq!(tick_idle(&mut collector), UsageTick::Idle);
        let cents = collector.snapshot().claude.month_cents;
        let _ = fs::remove_dir_all(&root);
        assert_eq!(cents, 500);
    }

    #[test]
    fn component_store_restart_does_not_double_complete_record() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-jsonl-store-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let store_root = root.join("store");
        let session = claude_session(&root);
        fs::write(
            &session,
            format!("{}\n", claude_usage_line("a", &current_stamp())),
        )
        .expect("a");
        let mut first = new_persisted_collector(&root, store_root.clone());
        assert_eq!(tick_idle(&mut first), UsageTick::Idle);
        assert_eq!(first.snapshot().claude.month_cents, 500);
        assert!(first.test_store_root().is_some());
        let mut restored = new_persisted_collector(&root, store_root);
        assert_eq!(tick_idle(&mut restored), UsageTick::Idle);
        let cents = restored.snapshot().claude.month_cents;
        let _ = fs::remove_dir_all(&root);
        assert_eq!(cents, 500);
        assert!(!restored.catch_up);
    }

    fn store_blob_text(store_root: &Path) -> String {
        let entry = fs::read_dir(store_root)
            .expect("store")
            .filter_map(Result::ok)
            .find(|entry| entry.file_name().to_string_lossy().ends_with(".state"))
            .expect("blob");
        fs::read_to_string(entry.path()).expect("blob text")
    }

    #[test]
    fn component_store_restart_rename_same_id_does_not_double() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-jsonl-dedupe-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let store_root = root.join("store");
        let session = claude_session(&root);
        let stamp = current_stamp();
        fs::write(&session, format!("{}\n", claude_usage_line("a", &stamp))).expect("a");
        let mut first = new_persisted_collector(&root, store_root.clone());
        assert_eq!(tick_idle(&mut first), UsageTick::Idle);
        assert_eq!(first.snapshot().claude.month_cents, 500);
        let blob = store_blob_text(&store_root);
        assert!(blob.contains("dkey="));
        assert!(!blob.contains("ckey="));
        assert!(!blob.contains("requestId"));
        assert!(!blob.contains("message\":"));
        let archived = session.with_file_name("archived.jsonl");
        fs::rename(&session, &archived).expect("rename");
        fs::write(&session, format!("{}\n", claude_usage_line("a", &stamp))).expect("recreate");
        let mut restored = new_persisted_collector(&root, store_root);
        assert_eq!(tick_idle(&mut restored), UsageTick::Idle);
        let cents = restored.snapshot().claude.month_cents;
        let _ = fs::remove_dir_all(&root);
        assert_eq!(cents, 500);
    }

    #[test]
    fn component_store_resume_mid_catch_up_does_not_double() {
        let root = std::env::temp_dir().join(format!(
            "run-dog-jsonl-mid-{}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let store_root = root.join("store");
        let session = claude_session(&root);
        let stamp = current_stamp();
        let line_len = 4_096;
        let count = ((super::CATCH_UP_BYTES_PER_TICK as usize / line_len) + 8) as u32;
        let mut body = String::new();
        for index in 0..count {
            body.push_str(&claude_usage_line_with_len(
                &format!("m{index}"),
                &stamp,
                line_len,
            ));
            body.push('\n');
        }
        fs::write(&session, body).expect("many lines");
        let expected = count.saturating_mul(500);
        let mut first = new_persisted_collector(&root, store_root.clone());
        assert_eq!(first.tick(ptr::null_mut()), UsageTick::MoreWork);
        assert!(first.catch_up);
        let partial = first.snapshot().claude.month_cents;
        assert!(partial > 0);
        assert!(partial < expected);
        let mut restored = new_persisted_collector(&root, store_root);
        assert!(restored.catch_up);
        assert_eq!(restored.snapshot().claude.month_cents, partial);
        assert_eq!(tick_idle(&mut restored), UsageTick::Idle);
        let cents = restored.snapshot().claude.month_cents;
        let _ = fs::remove_dir_all(&root);
        assert_eq!(cents, expected);
        assert!(!restored.catch_up);
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig {
            cases: 64,
            rng_seed: proptest::test_runner::RngSeed::Fixed(0x5EED_2026_0905_0002),
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::Direct(
                    "verification/evidence/usage-ingest-pbt.regressions",
                ),
            )),
            ..proptest::prelude::ProptestConfig::default()
        })]
        #[test]
        fn pbt_reader_prefix_without_newline_never_advances(cut in 1usize..80) {
            let root = std::env::temp_dir().join(format!(
                "run-dog-jsonl-pbt-{}-{}-{cut}",
                std::process::id(),
                unix_now_ms()
            ));
            let session = claude_session(&root);
            let line = claude_usage_line("pbt", &current_stamp());
            let end = cut.min(line.len().saturating_sub(1)).max(1);
            fs::write(&session, &line[..end]).expect("prefix");
            let chunk = read_chunk(&session, 0, super::MAX_BYTES_PER_TICK);
            let _ = fs::remove_dir_all(&root);
            proptest::prop_assert_eq!(chunk.new_offset, 0);
            proptest::prop_assert!(chunk.events.is_empty());
        }
    }

    #[test]
    fn component_bounded_jsonl_timestamp_fuzz_never_passes_safe_record() {
        use crate::core::{
            contains_forbidden_secret, fuzz_case_count, last_safe_complete_record_offset, mutate,
            seed_corpus, XorShift, FUZZ_SEED,
        };

        let corpus = seed_corpus();
        let mut rng = XorShift::new(FUZZ_SEED ^ 0x51);
        for _ in 0..fuzz_case_count() {
            let seed = &corpus[rng.below(corpus.len())];
            let input = mutate(&mut rng, seed);
            let safe = last_safe_complete_record_offset(&input);
            assert!(safe <= input.len() as u64);
            if let Ok(text) = std::str::from_utf8(&input) {
                assert!(!contains_forbidden_secret(text));
                let _ = parse_timestamp(text.trim());
                for line in text.split('\n') {
                    let trimmed = line.trim_end_matches('\r');
                    if trimmed.is_empty() {
                        continue;
                    }
                    let _ = parse_claude_line(trimmed);
                    let _ = parse_codex_model(trimmed);
                    let _ = parse_codex_usage_line(trimmed, Some("gpt-5.4"));
                    let _ = parse_codex_limits_line(trimmed);
                }
            }
            if !input.contains(&b'\n') {
                assert_eq!(safe, 0);
            }
        }
    }

    fn run_until_scan_idle(
        collector: &mut UsageCollector,
        max_ticks: usize,
    ) -> crate::core::UsageSnapshot {
        let mut last = collector.snapshot();
        for tick in 0..max_ticks {
            let more = collector.tick(ptr::null_mut());
            last = collector.snapshot();
            if tick % 100 == 0 {
                eprintln!(
                    "tick {tick}: catch_up={} claude_month={}c codex_month={}c",
                    last.month_scan_in_progress, last.claude.month_cents, last.codex.month_cents,
                );
            }
            if more == UsageTick::Idle && !last.month_scan_in_progress {
                return last;
            }
        }
        last
    }
}
