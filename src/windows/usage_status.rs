//! Small, read-only diagnostic record for the usage values sent to the tray.
//! This is separate from the durable collector checkpoint: the checkpoint
//! intentionally excludes the otak-usage display floor.

use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

use crate::core::{display_cents, ProviderUsage, UsageSnapshot};

use super::{
    otak_snapshot::ProviderSnapshot,
    usage::{UsageDiagnosticSource, UsageTick},
    usage_store_path::PinnedDirectory,
};

const STATUS_NAME: &str = "usage-status.json";
const STATUS_TEMP_NAME: &str = "usage-status.json.tmp";
const CATCH_UP_STATUS_INTERVAL_MS: u64 = 5_000;

#[derive(Serialize)]
struct ProviderStatus {
    local_today_cents: u32,
    cache_today_cents: Option<u32>,
    displayed_today_cents: u32,
    local_month_cents: u32,
    cache_month_cents: Option<u32>,
    displayed_month_cents: u32,
}

impl ProviderStatus {
    fn from_parts(
        local: ProviderUsage,
        cache: Option<ProviderSnapshot>,
        displayed: ProviderUsage,
    ) -> Self {
        Self {
            local_today_cents: local.today_cents,
            cache_today_cents: cache.map(|value| display_cents(value.today_cost_nanos)),
            displayed_today_cents: displayed.today_cents,
            local_month_cents: local.month_cents,
            cache_month_cents: cache.map(|value| display_cents(value.month_cost_nanos)),
            displayed_month_cents: displayed.month_cents,
        }
    }
}

#[derive(Serialize)]
struct LiveUsageStatus {
    schema: u8,
    pid: u32,
    captured_at_ms: u64,
    day: u32,
    month: u32,
    last_collected_event_ms: u64,
    pending_files: usize,
    month_scan_in_progress: bool,
    cache_snapshot_at_ms: Option<u64>,
    claude: ProviderStatus,
    codex: ProviderStatus,
}

impl LiveUsageStatus {
    fn new(source: UsageDiagnosticSource, displayed: UsageSnapshot, captured_at_ms: u64) -> Self {
        Self {
            schema: 1,
            pid: std::process::id(),
            captured_at_ms,
            day: source.day,
            month: source.month,
            last_collected_event_ms: source.last_collected_ms,
            pending_files: source.pending_files,
            month_scan_in_progress: displayed.month_scan_in_progress,
            cache_snapshot_at_ms: source.cache_floor.map(|cache| cache.updated_at_ms),
            claude: ProviderStatus::from_parts(
                source.local.claude,
                source.cache_floor.and_then(|cache| cache.claude),
                displayed.claude,
            ),
            codex: ProviderStatus::from_parts(
                source.local.codex,
                source.cache_floor.and_then(|cache| cache.codex),
                displayed.codex,
            ),
        }
    }
}

pub(super) struct UsageStatusWriter {
    root: Option<PathBuf>,
    last_attempt_ms: u64,
}

impl UsageStatusWriter {
    pub(super) fn production() -> Self {
        Self {
            root: std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .map(|local| local.join("RunDog").join("diagnostics")),
            last_attempt_ms: 0,
        }
    }

    /// At most one write per five seconds during catch-up; always record an
    /// idle tick. A failed write remains visible as a missing or stale record.
    pub(super) fn record(
        &mut self,
        source: UsageDiagnosticSource,
        displayed: UsageSnapshot,
        tick: UsageTick,
    ) {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis() as u64);
        if tick == UsageTick::MoreWork
            && self.last_attempt_ms != 0
            && now_ms.saturating_sub(self.last_attempt_ms) < CATCH_UP_STATUS_INTERVAL_MS
        {
            return;
        }
        self.last_attempt_ms = now_ms;
        let Some(root) = self.root.as_deref() else {
            return;
        };
        let status = LiveUsageStatus::new(source, displayed, now_ms);
        let Ok(bytes) = serde_json::to_vec(&status) else {
            return;
        };
        let Some(directory) = PinnedDirectory::open(root, true, &[], false) else {
            return;
        };
        let _ = directory.write_atomically(
            &root.join(STATUS_TEMP_NAME),
            &root.join(STATUS_NAME),
            &bytes,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::LiveUsageStatus;
    use crate::core::UsageSnapshot;
    use crate::windows::{
        otak_snapshot::{OtakSnapshot, ProviderSnapshot},
        usage::UsageDiagnosticSource,
    };

    #[test]
    fn status_distinguishes_local_cache_and_exact_tray_values() {
        let mut local = UsageSnapshot::default();
        local.claude.add_today_nanos(12_340_000_000);
        local.codex.add_today_nanos(5_670_000_000);
        let mut displayed = local;
        displayed.claude.add_today_nanos(77_670_000_000);
        displayed.codex.add_today_nanos(228_890_000_000);
        let source = UsageDiagnosticSource {
            local,
            cache_floor: Some(OtakSnapshot {
                updated_at_ms: 1_791_460_000_000,
                claude: Some(ProviderSnapshot {
                    today_cost_nanos: 90_010_000_000,
                    ..ProviderSnapshot::default()
                }),
                codex: Some(ProviderSnapshot {
                    today_cost_nanos: 234_560_000_000,
                    ..ProviderSnapshot::default()
                }),
            }),
            day: 20261008,
            month: 20261001,
            last_collected_ms: 1_791_440_000_000,
            pending_files: 992,
        };
        let value =
            serde_json::to_value(LiveUsageStatus::new(source, displayed, 1_791_460_001_000))
                .expect("small status JSON");
        assert_eq!(value["claude"]["local_today_cents"], 1234);
        assert_eq!(value["claude"]["cache_today_cents"], 9001);
        assert_eq!(value["claude"]["displayed_today_cents"], 9001);
        assert_eq!(value["codex"]["local_today_cents"], 567);
        assert_eq!(value["codex"]["cache_today_cents"], 23456);
        assert_eq!(value["codex"]["displayed_today_cents"], 23456);
        assert_eq!(value["pending_files"], 992);
        assert_eq!(value["cache_snapshot_at_ms"], 1_791_460_000_000_u64);
        assert!(!value.to_string().contains(".jsonl"));
    }
}
