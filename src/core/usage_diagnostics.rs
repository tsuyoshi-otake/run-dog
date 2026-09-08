//! Bounded usage diagnostic ring. Numbers and small enums only.
//!
//! Forbidden in events: raw paths, prompts, responses, JSONL bodies,
//! access/refresh tokens, Authorization headers, account identifiers.

use super::{CursorRebuildReason, FetchErrorKind, LimitsFreshness, PersistStatus};

pub const DIAGNOSTIC_RING_CAP: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum DiagnosticKind {
    StartupMode = 1,
    CheckpointSource = 2,
    CheckpointSchema = 3,
    RestoreResult = 4,
    CheckpointBytes = 5,
    KnownFiles = 6,
    DirsEnumerated = 7,
    FilesStated = 8,
    FilesOpened = 9,
    UsageParseBytes = 10,
    IntegrityProbeBytes = 11,
    LimitsTailBytes = 12,
    CursorResetCount = 13,
    CursorResetReason = 14,
    RescanReason = 15,
    CatchUpState = 16,
    RebuildState = 17,
    CheckpointWriteBytes = 18,
    CheckpointSaveResult = 19,
    ClaudeFetch = 20,
    CodexFetch = 21,
    PendingFiles = 22,
    OldestPendingAge = 23,
}

impl DiagnosticKind {
    pub const ALL: [Self; 23] = [
        Self::StartupMode,
        Self::CheckpointSource,
        Self::CheckpointSchema,
        Self::RestoreResult,
        Self::CheckpointBytes,
        Self::KnownFiles,
        Self::DirsEnumerated,
        Self::FilesStated,
        Self::FilesOpened,
        Self::UsageParseBytes,
        Self::IntegrityProbeBytes,
        Self::LimitsTailBytes,
        Self::CursorResetCount,
        Self::CursorResetReason,
        Self::RescanReason,
        Self::CatchUpState,
        Self::RebuildState,
        Self::CheckpointWriteBytes,
        Self::CheckpointSaveResult,
        Self::ClaudeFetch,
        Self::CodexFetch,
        Self::PendingFiles,
        Self::OldestPendingAge,
    ];

    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self as u32
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum StartupMode {
    Test = 1,
    Production = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum CheckpointSource {
    Missing = 0,
    FileStore = 1,
    Registry = 2,
    RecoveredPrior = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RestoreResult {
    Missing = 0,
    Loaded = 1,
    Recovered = 2,
    Refused = 3,
    Rebuilding = 4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RescanReason {
    None = 0,
    User = 1,
    LegacyIdentity = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RebuildState {
    Idle = 0,
    CatchUp = 1,
    Rescan = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiagnosticEvent {
    pub kind: DiagnosticKind,
    pub value: u64,
    pub detail: u32,
    pub at_ms: u64,
}

impl DiagnosticEvent {
    #[must_use]
    pub const fn new(kind: DiagnosticKind, value: u64, detail: u32, at_ms: u64) -> Self {
        Self {
            kind,
            value,
            detail,
            at_ms,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DiagnosticRing {
    events: [Option<DiagnosticEvent>; DIAGNOSTIC_RING_CAP],
    next: usize,
    len: usize,
}

impl Default for DiagnosticRing {
    fn default() -> Self {
        Self {
            events: [None; DIAGNOSTIC_RING_CAP],
            next: 0,
            len: 0,
        }
    }
}

impl DiagnosticRing {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, event: DiagnosticEvent) {
        self.events[self.next] = Some(event);
        self.next = (self.next + 1) % DIAGNOSTIC_RING_CAP;
        self.len = (self.len + 1).min(DIAGNOSTIC_RING_CAP);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[must_use]
    pub fn latest(&self, kind: DiagnosticKind) -> Option<DiagnosticEvent> {
        self.iter().rev().find(|event| event.kind == kind)
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = DiagnosticEvent> + '_ {
        let start = if self.len == DIAGNOSTIC_RING_CAP {
            self.next
        } else {
            0
        };
        (0..self.len).filter_map(move |offset| {
            let index = (start + offset) % DIAGNOSTIC_RING_CAP;
            self.events[index]
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiagnosticSnapshot {
    pub startup_mode: u64,
    pub checkpoint_source: u64,
    pub checkpoint_schema: u64,
    pub restore_result: u64,
    pub checkpoint_bytes: u64,
    pub known_files: u64,
    pub dirs_enumerated: u64,
    pub files_stated: u64,
    pub files_opened: u64,
    pub usage_parse_bytes: u64,
    pub integrity_probe_bytes: u64,
    pub limits_tail_bytes: u64,
    pub cursor_reset_count: u64,
    pub cursor_reset_reason: u64,
    pub rescan_reason: u64,
    pub catch_up: u64,
    pub rebuild_state: u64,
    pub checkpoint_write_bytes: u64,
    pub checkpoint_save_result: u64,
    pub claude_fetch: u64,
    pub codex_fetch: u64,
    pub pending_files: u64,
    pub oldest_pending_age_ms: u64,
}

impl DiagnosticSnapshot {
    pub fn record(&mut self, event: DiagnosticEvent) {
        match event.kind {
            DiagnosticKind::StartupMode => self.startup_mode = event.value,
            DiagnosticKind::CheckpointSource => self.checkpoint_source = event.value,
            DiagnosticKind::CheckpointSchema => self.checkpoint_schema = event.value,
            DiagnosticKind::RestoreResult => self.restore_result = event.value,
            DiagnosticKind::CheckpointBytes => self.checkpoint_bytes = event.value,
            DiagnosticKind::KnownFiles => self.known_files = event.value,
            DiagnosticKind::DirsEnumerated => self.dirs_enumerated = event.value,
            DiagnosticKind::FilesStated => self.files_stated = event.value,
            DiagnosticKind::FilesOpened => self.files_opened = event.value,
            DiagnosticKind::UsageParseBytes => self.usage_parse_bytes = event.value,
            DiagnosticKind::IntegrityProbeBytes => self.integrity_probe_bytes = event.value,
            DiagnosticKind::LimitsTailBytes => self.limits_tail_bytes = event.value,
            DiagnosticKind::CursorResetCount => self.cursor_reset_count = event.value,
            DiagnosticKind::CursorResetReason => self.cursor_reset_reason = event.value,
            DiagnosticKind::RescanReason => self.rescan_reason = event.value,
            DiagnosticKind::CatchUpState => self.catch_up = event.value,
            DiagnosticKind::RebuildState => self.rebuild_state = event.value,
            DiagnosticKind::CheckpointWriteBytes => self.checkpoint_write_bytes = event.value,
            DiagnosticKind::CheckpointSaveResult => self.checkpoint_save_result = event.value,
            DiagnosticKind::ClaudeFetch => self.claude_fetch = event.value,
            DiagnosticKind::CodexFetch => self.codex_fetch = event.value,
            DiagnosticKind::PendingFiles => self.pending_files = event.value,
            DiagnosticKind::OldestPendingAge => self.oldest_pending_age_ms = event.value,
        }
    }

    #[must_use]
    pub fn from_ring(ring: &DiagnosticRing) -> Self {
        let mut snapshot = Self::default();
        for event in ring.iter() {
            snapshot.record(event);
        }
        snapshot
    }
}

#[must_use]
pub fn persist_result_code(status: PersistStatus) -> u64 {
    match status {
        PersistStatus::Applied { generation } => generation,
        PersistStatus::Failed => 0,
    }
}

#[must_use]
pub fn persist_result_detail(status: PersistStatus) -> u32 {
    match status {
        PersistStatus::Applied { .. } => 1,
        PersistStatus::Failed => 2,
    }
}

#[must_use]
pub fn rebuild_reason_code(reason: CursorRebuildReason) -> u64 {
    match reason {
        CursorRebuildReason::SizeShrunk => 1,
        CursorRebuildReason::PrefixChanged => 2,
        CursorRebuildReason::FileIdAndPrefixChanged => 3,
        CursorRebuildReason::FileIdChanged => 5,
        CursorRebuildReason::SameSizeRewriteHint => 4,
    }
}

#[must_use]
pub fn fetch_result_code(error: Option<FetchErrorKind>, freshness: LimitsFreshness) -> u64 {
    match error {
        Some(kind) => u64::from(kind.as_u32()),
        None => u64::from(freshness.as_u32()) << 8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_ring_is_bounded_and_overwrites() {
        let mut ring = DiagnosticRing::new();
        for index in 0..(DIAGNOSTIC_RING_CAP + 3) {
            ring.push(DiagnosticEvent::new(
                DiagnosticKind::KnownFiles,
                index as u64,
                0,
                index as u64,
            ));
        }
        assert_eq!(ring.len(), DIAGNOSTIC_RING_CAP);
        let values: Vec<u64> = ring.iter().map(|event| event.value).collect();
        assert_eq!(values.first().copied(), Some(3));
        assert_eq!(
            values.last().copied(),
            Some((DIAGNOSTIC_RING_CAP + 2) as u64)
        );
    }

    #[test]
    fn component_required_kinds_are_all_named() {
        assert_eq!(DiagnosticKind::ALL.len(), 23);
    }

    #[test]
    fn component_events_are_numeric_only() {
        let event = DiagnosticEvent::new(
            DiagnosticKind::ClaudeFetch,
            u64::from(FetchErrorKind::HttpStatus.as_u32()),
            401,
            1,
        );
        let rendered = format!("{event:?}");
        assert!(!rendered.contains("Bearer"));
        assert!(!rendered.contains("Authorization"));
        assert!(!rendered.contains("sk-"));
        assert!(!rendered.contains("C:\\Users"));
        assert!(!rendered.contains("prompt"));
    }

    #[test]
    fn component_snapshot_tracks_latest_per_kind() {
        let mut ring = DiagnosticRing::new();
        ring.push(DiagnosticEvent::new(
            DiagnosticKind::RestoreResult,
            RestoreResult::Loaded as u64,
            0,
            1,
        ));
        ring.push(DiagnosticEvent::new(
            DiagnosticKind::RestoreResult,
            RestoreResult::Recovered as u64,
            0,
            2,
        ));
        let snapshot = DiagnosticSnapshot::from_ring(&ring);
        assert_eq!(snapshot.restore_result, RestoreResult::Recovered as u64);
    }
}
