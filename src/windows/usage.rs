//! Incremental Claude Code / Codex CLI usage collector.
//!
//! Designed so antivirus realtime scanners barely see it:
//! - never spawns `claude` / `codex` / Node
//! - never writes into those home directories
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
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Deserialize;
use windows_sys::Win32::{
    Foundation::HWND,
    System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION},
    UI::WindowsAndMessaging::PostMessageW,
};

use crate::core::{
    cost_cents, local_ymd, ymd_key, LimitWindow, ProviderUsage, TokenUsage, UsageSnapshot,
};

pub const USAGE_TIMER_ID: usize = 4;
pub const USAGE_READY_MESSAGE: u32 = 0x8000 + 3;
pub const USAGE_FIRST_INTERVAL_MS: u32 = 8_000;
pub const USAGE_IDLE_INTERVAL_MS: u32 = 60_000;
pub const USAGE_CONTINUE_INTERVAL_MS: u32 = 400;

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
const STAT_COOLDOWN_MS: u64 = 30 * 1_000;
const HOT_AGE_MS: u64 = 48 * 60 * 60 * 1_000;
const REDISCOVER_MS: u64 = 30 * 60 * 1_000;
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
    kind: SourceKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    claude_keys: HashSet<u64>,
    pending: VecDeque<PathBuf>,
    snapshot: UsageSnapshot,
    month_key: u32,
    last_discover_ms: u64,
    last_claude_limits_ms: u64,
    last_codex_limits_ms: u64,
    restat_skip: usize,
    catch_up: bool,
    deferred: VecDeque<PathBuf>,
    read_buf: Vec<u8>,
    codex_from_remote: bool,
    remote_fetch: RemoteLimitsFetch,
}

struct RemoteLimits {
    claude: Option<ProviderUsage>,
    codex: Option<ProviderUsage>,
}

struct RemoteLimitsFetch {
    in_flight: Arc<AtomicBool>,
    latest: Arc<Mutex<Option<RemoteLimits>>>,
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
        Self::with_dirs(claude_dir, codex_home)
    }

    #[must_use]
    fn with_dirs(claude_dir: PathBuf, codex_home: PathBuf) -> Self {
        Self {
            claude_dir,
            codex_home,
            files: HashMap::new(),
            dirs: HashMap::new(),
            discover: VecDeque::new(),
            claude_keys: HashSet::new(),
            pending: VecDeque::new(),
            snapshot: UsageSnapshot::default(),
            month_key: 0,
            last_discover_ms: 0,
            last_claude_limits_ms: 0,
            last_codex_limits_ms: 0,
            restat_skip: 0,
            catch_up: true,
            deferred: VecDeque::new(),
            read_buf: Vec::new(),
            codex_from_remote: false,
            remote_fetch: RemoteLimitsFetch {
                in_flight: Arc::new(AtomicBool::new(false)),
                latest: Arc::new(Mutex::new(None)),
            },
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> UsageSnapshot {
        self.snapshot
    }

    pub fn take_claude_limits(&mut self) -> bool {
        let Some(remote) = self
            .remote_fetch
            .latest
            .lock()
            .ok()
            .and_then(|mut guard| guard.take())
        else {
            return false;
        };
        let mut changed = false;
        if let Some(limits) = remote.claude {
            apply_provider_limits(&mut self.snapshot.claude, limits);
            changed = true;
        }
        if let Some(limits) = remote.codex {
            apply_provider_limits(&mut self.snapshot.codex, limits);
            self.codex_from_remote = true;
            changed = true;
        }
        changed
    }

    #[must_use]
    pub fn tick(&mut self, hwnd: HWND) -> UsageTick {
        let now_ms = unix_now_ms();
        let window = day_window(now_ms);
        self.reset_if_month_changed(window);
        let _ = self.take_claude_limits();

        if self.last_discover_ms == 0
            || now_ms.saturating_sub(self.last_discover_ms) >= REDISCOVER_MS
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

        if self.catch_up && self.discover.is_empty() && !unread_remaining {
            self.finish_catch_up();
        }

        if self.discover.is_empty() && !unread_remaining && opens < self.file_budget() {
            self.release_scratch();
            UsageTick::Idle
        } else {
            UsageTick::MoreWork
        }
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
        self.files.clear();
        self.dirs.clear();
        self.claude_keys.clear();
        self.pending.clear();
        self.deferred.clear();
        self.catch_up = true;
        self.snapshot.claude.today_cents = 0;
        self.snapshot.claude.month_cents = 0;
        self.snapshot.codex.today_cents = 0;
        self.snapshot.codex.month_cents = 0;
        self.queue_roots();
    }

    fn queue_roots(&mut self) {
        self.discover.clear();
        self.discover.push_back(self.claude_dir.join("projects"));
        let (year, month, _) = local_ymd(unix_now_ms(), day_window(unix_now_ms()).bias_minutes);
        self.discover
            .push_back(codex_month_dir(&self.codex_home, year, month));
        let (prev_year, prev_month) = previous_month(year, month);
        self.discover
            .push_back(codex_month_dir(&self.codex_home, prev_year, prev_month));
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
        let mut files = Vec::new();
        let mut dirs = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                dirs.push(path);
            } else if file_type.is_file() && is_jsonl(&path) {
                files.push(path);
            }
        }
        for child in &dirs {
            self.discover.push_back(child.clone());
        }
        for path in &files {
            let Some((size, mtime_ms)) = path_size_mtime(path) else {
                continue;
            };
            if ymd_key_from_unix(mtime_ms, window.bias_minutes) < previous_month_start(window) {
                continue;
            }
            let kind = if path.starts_with(&self.codex_home) {
                SourceKind::Codex
            } else {
                SourceKind::Claude
            };
            if let std::collections::hash_map::Entry::Vacant(entry) = self.files.entry(path.clone())
            {
                entry.insert(FileCursor {
                    size,
                    mtime_ms,
                    offset: 0,
                    last_stat_ms: 0,
                    last_model: None,
                    kind,
                });
                if size > 0 {
                    self.enqueue_scan(path.clone(), mtime_ms, window);
                }
            }
        }
        self.dirs
            .insert(dir.to_path_buf(), DirListing { mtime_ms, dirs });
    }

    fn scan_file(&mut self, path: &Path, window: DayWindow, now_ms: u64) -> u64 {
        let Some((size, mtime_ms)) = path_size_mtime(path) else {
            return 0;
        };
        let max_bytes = self.byte_budget();
        let (kind, offset, previous_model) = {
            let Some(cursor) = self.files.get_mut(path) else {
                return 0;
            };
            cursor.last_stat_ms = now_ms;
            if size < cursor.offset {
                cursor.offset = 0;
                cursor.last_model = None;
            }
            if size == cursor.offset {
                cursor.size = size;
                cursor.mtime_ms = mtime_ms;
                return 0;
            }
            (cursor.kind, cursor.offset, cursor.last_model.clone())
        };
        let chunk = read_appended(
            path,
            offset,
            size,
            kind,
            previous_model.as_deref(),
            max_bytes,
            &mut self.read_buf,
        );
        let Some(cursor) = self.files.get_mut(path) else {
            return chunk.consumed;
        };
        cursor.offset = chunk.new_offset;
        cursor.size = size;
        cursor.mtime_ms = mtime_ms;
        cursor.last_model = chunk.last_model;
        if let Some(limits) = chunk.limits {
            if kind == SourceKind::Codex
                && !self.codex_from_remote
                && is_subscription_limits(&limits)
            {
                apply_provider_limits(&mut self.snapshot.codex, limits);
            }
        }
        for (event, key) in chunk.events.into_iter().zip(chunk.dedupe) {
            if let Some(key) = key {
                if !self.claude_keys.insert(key) {
                    continue;
                }
            }
            let Some(cents) = cost_cents(&event.model, event.usage) else {
                continue;
            };
            let day = ymd_key_from_unix(event.timestamp_ms, window.bias_minutes);
            let target = match kind {
                SourceKind::Claude => &mut self.snapshot.claude,
                SourceKind::Codex => &mut self.snapshot.codex,
            };
            if day == window.today {
                target.today_cents = target.today_cents.saturating_add(cents);
            }
            if day >= window.month_start {
                target.month_cents = target.month_cents.saturating_add(cents);
            }
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
            if self
                .files
                .get(&path)
                .is_some_and(|cursor| cursor.size != cursor.offset)
            {
                self.pending.push_back(path);
            }
        }

        if !self.catch_up {
            let mut skipped = 0_usize;
            let mut stated = 0_usize;
            let mut restat = Vec::new();
            for (path, cursor) in &self.files {
                if cursor.size != cursor.offset {
                    continue;
                }
                if !is_hot(cursor, now_ms) {
                    continue;
                }
                if now_ms.saturating_sub(cursor.last_stat_ms) < STAT_COOLDOWN_MS {
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
                    if self
                        .files
                        .get(&path)
                        .is_some_and(|cursor| cursor.size != cursor.offset)
                    {
                        self.pending.push_back(path);
                    }
                }
            }
        }

        let unread_remaining = self.pending.iter().any(|path| {
            self.files
                .get(path)
                .is_some_and(|cursor| cursor.size != cursor.offset)
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

    fn finish_catch_up(&mut self) {
        self.catch_up = false;
        self.pending.extend(self.deferred.drain(..));
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
            if let Some(limits) = read_codex_limits_tail(path, size) {
                if is_subscription_limits(&limits) {
                    self.snapshot.codex.primary =
                        limits.primary.map(|window| window.effective(now_ms));
                    self.snapshot.codex.secondary =
                        limits.secondary.map(|window| window.effective(now_ms));
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
        if hwnd.is_null() {
            return;
        }
        if now_ms.saturating_sub(self.last_claude_limits_ms) < CLAUDE_LIMITS_PERIOD_MS {
            return;
        }
        if self
            .remote_fetch
            .in_flight
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        self.last_claude_limits_ms = now_ms;
        let claude_dir = self.claude_dir.clone();
        let codex_home = self.codex_home.clone();
        let latest = Arc::clone(&self.remote_fetch.latest);
        let in_flight = Arc::clone(&self.remote_fetch.in_flight);
        let hwnd = hwnd as isize;
        thread::spawn(move || {
            let remote = RemoteLimits {
                claude: fetch_claude_limits(&claude_dir),
                codex: fetch_codex_wham_limits(&codex_home),
            };
            if remote.claude.is_some() || remote.codex.is_some() {
                if let Ok(mut guard) = latest.lock() {
                    *guard = Some(remote);
                }
                let _ = unsafe { PostMessageW(hwnd as HWND, USAGE_READY_MESSAGE, 0, 0) };
            }
            in_flight.store(false, Ordering::SeqCst);
        });
    }
}

struct ParsedEvent {
    model: String,
    timestamp_ms: u64,
    usage: TokenUsage,
}

fn is_hot(cursor: &FileCursor, now_ms: u64) -> bool {
    now_ms.saturating_sub(cursor.mtime_ms) <= HOT_AGE_MS || cursor.size != cursor.offset
}

fn is_jsonl(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "jsonl")
}

fn is_current_month(mtime_ms: u64, window: DayWindow) -> bool {
    ymd_key_from_unix(mtime_ms, window.bias_minutes) >= window.month_start
}

struct AppendedChunk {
    new_offset: u64,
    consumed: u64,
    events: Vec<ParsedEvent>,
    dedupe: Vec<Option<u64>>,
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
    let empty = AppendedChunk {
        new_offset: offset,
        consumed: 0,
        events: Vec::new(),
        dedupe: Vec::new(),
        limits: None,
        last_model: last_model.map(str::to_owned),
    };
    let budget = (size - offset).min(max_bytes.max(1));
    let Ok(mut file) = File::open(path) else {
        return empty;
    };
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return empty;
    }
    buf.clear();
    buf.resize(budget as usize, 0);
    let Ok(read) = file.read(buf) else {
        return empty;
    };
    buf.truncate(read);
    let complete = buf.iter().rposition(|byte| *byte == b'\n');
    let Some(end) = complete else {
        return skip_incomplete_line(&mut file, offset, size, read as u64, last_model);
    };
    let consumed = (end + 1) as u64;
    let text = String::from_utf8_lossy(&buf[..=end]);
    let mut events = Vec::new();
    let mut keys = Vec::new();
    let mut limits = None;
    let mut model = last_model.map(str::to_owned);
    for line in text.lines() {
        if line.is_empty() || line.len() > MAX_PARSE_LINE {
            continue;
        }
        match kind {
            SourceKind::Claude => {
                if let Some((event, key)) = parse_claude_line(line) {
                    events.push(event);
                    keys.push(Some(key));
                }
            }
            SourceKind::Codex => {
                if let Some(next_model) = parse_codex_model(line) {
                    model = Some(next_model);
                    continue;
                }
                if let Some(found) = parse_codex_limits_line(line) {
                    limits = Some(found);
                }
                if let Some(event) = parse_codex_usage_line(line, model.as_deref()) {
                    events.push(event);
                    keys.push(None);
                }
            }
        }
    }
    AppendedChunk {
        new_offset: offset + consumed,
        consumed,
        events,
        dedupe: keys,
        limits,
        last_model: model,
    }
}

fn skip_incomplete_line(
    file: &mut File,
    offset: u64,
    size: u64,
    already_read: u64,
    last_model: Option<&str>,
) -> AppendedChunk {
    let last_model = last_model.map(str::to_owned);
    let mut scanned = already_read;
    let limit = already_read.saturating_add(MAX_SKIP_PER_READ);
    let mut tmp = [0_u8; 65_536];
    let tmp_len = tmp.len();
    while offset + scanned < size && scanned < limit {
        let remaining = (size - offset - scanned) as usize;
        let n = match file.read(&mut tmp[..remaining.min(tmp_len)]) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        if let Some(i) = tmp[..n].iter().position(|byte| *byte == b'\n') {
            let new_offset = offset + scanned + i as u64 + 1;
            return AppendedChunk {
                new_offset,
                consumed: new_offset - offset,
                events: Vec::new(),
                dedupe: Vec::new(),
                limits: None,
                last_model,
            };
        }
        scanned += n as u64;
    }
    AppendedChunk {
        new_offset: offset + scanned,
        consumed: scanned,
        events: Vec::new(),
        dedupe: Vec::new(),
        limits: None,
        last_model,
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

fn parse_claude_line(line: &str) -> Option<(ParsedEvent, u64)> {
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
    let key = hash_key(
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
}

#[derive(Deserialize)]
struct CodexTokens {
    input_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct CodexRateLimits {
    primary: Option<CodexWindow>,
    secondary: Option<CodexWindow>,
    plan_type: Option<String>,
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
    let model = model?.to_owned();
    let last = payload.info?.last_token_usage?;
    let raw_input = last.input_tokens.unwrap_or(0);
    let cached = last.cached_input_tokens.unwrap_or(0).min(raw_input);
    Some(ParsedEvent {
        model,
        timestamp_ms: parse_timestamp(rec.timestamp.as_deref()?)?,
        usage: TokenUsage {
            input: raw_input - cached,
            cached_input: cached,
            output: last.output_tokens.unwrap_or(0),
            ..TokenUsage::default()
        },
    })
}

fn parse_codex_limits_line(line: &str) -> Option<ProviderUsage> {
    let rec: CodexLine = serde_json::from_str(line).ok()?;
    let limits = rec.payload?.rate_limits?;
    let mut usage = ProviderUsage {
        primary: limits.primary.and_then(codex_window),
        secondary: limits.secondary.and_then(codex_window),
        ..ProviderUsage::default()
    };
    if let Some(plan) = limits.plan_type {
        usage.set_plan(&plan);
    }
    if usage.primary.is_none() && usage.secondary.is_none() {
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
    let raw = fs::read_to_string(claude_dir.join(".credentials.json")).ok()?;
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
}

#[derive(Deserialize)]
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

fn fetch_claude_limits(claude_dir: &Path) -> Option<ProviderUsage> {
    let mut creds = read_claude_credentials(claude_dir)?;
    if claude_token_expired(&creds) {
        let _ = refresh_claude_credentials(claude_dir, &mut creds);
    }
    if let Some(usage) = claude_usage_request(&creds.access_token, creds.plan.as_deref()) {
        return Some(usage);
    }
    if refresh_claude_credentials(claude_dir, &mut creds) {
        claude_usage_request(&creds.access_token, creds.plan.as_deref())
    } else {
        None
    }
}

fn claude_usage_request(token: &str, plan: Option<&str>) -> Option<ProviderUsage> {
    let (status, body) = super::update::https_get(
        "api.anthropic.com",
        "/api/oauth/usage",
        &format!("Authorization: Bearer {token}\r\nanthropic-beta: oauth-2025-04-20\r\n"),
        16 * 1_024,
    )
    .ok()?;
    if status != 200 {
        return None;
    }
    let text = String::from_utf8(body).ok()?;
    parse_claude_usage_response(&text, plan)
}

fn refresh_claude_credentials(claude_dir: &Path, creds: &mut ClaudeCreds) -> bool {
    let Some(refresh_token) = creds.refresh_token.as_deref() else {
        return false;
    };
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
        creds.access_token = access;
        if let Some(next) = parsed.refresh_token.filter(|token| !token.is_empty()) {
            creds.refresh_token = Some(next);
        }
        creds.expires_at_ms = Some(parsed.expires_at.map_or_else(
            || {
                unix_now_ms()
                    .saturating_add(parsed.expires_in.unwrap_or(3_600).saturating_mul(1_000))
            },
            normalize_expiry_ms,
        ));
        persist_claude_credentials(claude_dir, creds);
        return true;
    }
    false
}

fn persist_claude_credentials(claude_dir: &Path, creds: &ClaudeCreds) {
    let path = claude_dir.join(".credentials.json");
    let Ok(meta) = fs::metadata(&path) else {
        return;
    };
    let Ok(mtime) = meta.modified() else {
        return;
    };
    let Ok(raw) = fs::read_to_string(&path) else {
        return;
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    let Some(oauth) = value.get_mut("claudeAiOauth") else {
        return;
    };
    oauth["accessToken"] = serde_json::Value::String(creds.access_token.clone());
    if let Some(refresh) = &creds.refresh_token {
        oauth["refreshToken"] = serde_json::Value::String(refresh.clone());
    }
    if let Some(expires) = creds.expires_at_ms {
        oauth["expiresAt"] = serde_json::Value::from(expires);
    }
    let Ok(encoded) = serde_json::to_vec(&value) else {
        return;
    };
    if fs::metadata(&path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        != Some(mtime)
    {
        return;
    }
    let _ = fs::write(path, encoded);
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

fn fetch_codex_wham_limits(codex_home: &Path) -> Option<ProviderUsage> {
    let raw = fs::read_to_string(codex_home.join("auth.json")).ok()?;
    let file: CodexAuthFile = serde_json::from_str(&raw).ok()?;
    let tokens = file.tokens?;
    let access = tokens.access_token.filter(|token| !token.is_empty())?;
    let account = tokens.account_id.filter(|id| !id.is_empty())?;
    let (status, body) = super::update::https_get(
        "chatgpt.com",
        "/backend-api/wham/usage",
        &format!(
            "Authorization: Bearer {access}\r\nChatGPT-Account-Id: {account}\r\nAccept: application/json\r\n"
        ),
        16 * 1_024,
    )
    .ok()?;
    if status != 200 {
        return None;
    }
    parse_wham_usage_response(&String::from_utf8(body).ok()?)
}

pub fn parse_wham_usage_response(body: &str) -> Option<ProviderUsage> {
    let parsed: WhamUsageResponse = serde_json::from_str(body).ok()?;
    let rate = parsed.rate_limit?;
    let mut usage = ProviderUsage {
        primary: rate.primary_window.and_then(wham_window),
        secondary: rate.secondary_window.and_then(wham_window),
        ..ProviderUsage::default()
    };
    if let Some(plan) = parsed.plan_type {
        usage.set_plan(&plan);
    }
    if usage.primary.is_none() && usage.secondary.is_none() {
        None
    } else {
        Some(usage)
    }
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
    let mut usage = ProviderUsage {
        primary: claude_window(parsed.five_hour, 300),
        secondary: claude_window(parsed.seven_day, 10_080),
        ..ProviderUsage::default()
    };
    if let Some(plan) = plan {
        usage.set_plan(plan);
    }
    if usage.primary.is_none() && usage.secondary.is_none() {
        None
    } else {
        Some(usage)
    }
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
    // RFC3339-like `2026-08-16T10:15:30Z` or with offset. Enough for local logs.
    if value.len() < 20 {
        return None;
    }
    let year: i32 = value.get(0..4)?.parse().ok()?;
    let month: u8 = value.get(5..7)?.parse().ok()?;
    let day: u8 = value.get(8..10)?.parse().ok()?;
    let hour: u8 = value.get(11..13)?.parse().ok()?;
    let minute: u8 = value.get(14..16)?.parse().ok()?;
    let second: u8 = value.get(17..19)?.parse().ok()?;
    let days = ymd_to_days(year, month, day)?;
    Some(
        (days * 86_400 + u64::from(hour) * 3_600 + u64::from(minute) * 60 + u64::from(second))
            * 1_000,
    )
}

fn ymd_to_days(year: i32, month: u8, day: u8) -> Option<u64> {
    if !(1..=12).contains(&month) || day == 0 {
        return None;
    }
    let (y, m) = if month <= 2 {
        (year - 1, i32::from(month) + 9)
    } else {
        (year, i32::from(month) - 3)
    };
    let era = y.div_euclid(400);
    let yoe = (y - era * 400) as u64;
    let doy = (153 * m as u64 + 2) / 5 + u64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era as i64 * 146_097 + doe as i64 - 719_468) as u64)
}

fn hash_key(message_id: &str, request_id: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    message_id.hash(&mut hasher);
    request_id.hash(&mut hasher);
    hasher.finish()
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub(super) fn timezone_bias_minutes() -> i32 {
    let mut info = TIME_ZONE_INFORMATION::default();
    let _ = unsafe { GetTimeZoneInformation(&mut info) };
    info.Bias
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
        is_current_month, is_subscription_limits, parse_claude_line, parse_claude_usage_response,
        parse_codex_limits_line, parse_codex_model, parse_codex_usage_line,
        parse_wham_usage_response, unix_now_ms, UsageCollector, UsageTick,
    };
    use crate::core::{local_hms, local_ymd};
    use std::{fs, ptr};

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
    }

    #[test]
    fn component_codex_and_claude_limit_payloads_round_trip() {
        let codex = parse_codex_limits_line(
            r#"{"timestamp":"2026-08-16T01:02:03Z","payload":{"rate_limits":{"plan_type":"pro","primary":{"used_percent":5.0,"resets_at":1780000000,"window_minutes":300},"secondary":{"used_percent":21.5,"resets_at":1780500000,"window_minutes":10080}}}}"#,
        )
        .expect("codex limits");
        assert_eq!(codex.plan_label().as_deref(), Some("Pro 20x"));
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
    fn component_chatgpt_wham_usage_maps_primary_and_weekly_windows() {
        let usage = parse_wham_usage_response(
            r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":34,"limit_window_seconds":18000,"reset_at":1778091218},"secondary_window":{"used_percent":37,"limit_window_seconds":604800,"reset_at":1778605571}}}"#,
        )
        .expect("wham usage");
        assert_eq!(usage.plan_label().as_deref(), Some("Pro 20x"));
        assert_eq!(usage.primary.unwrap().window_minutes, 300);
        assert_eq!(usage.primary.unwrap().used_tenths, 340);
        assert_eq!(usage.secondary.unwrap().window_minutes, 10_080);
        assert_eq!(usage.secondary.unwrap().used_tenths, 370);
    }

    #[test]
    fn component_catch_up_window_keeps_current_month_and_drops_epoch() {
        let window = super::day_window(unix_now_ms());
        assert!(is_current_month(unix_now_ms(), window));
        assert!(!is_current_month(0, window));
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

        let mut collector = UsageCollector::with_dirs(root.join("claude"), root.join("codex"));
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

        let mut collector = UsageCollector::with_dirs(root.join("claude"), root.join("codex"));
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
}
