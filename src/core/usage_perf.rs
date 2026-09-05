//! Performance scenario catalog and warm-restart oracles.
//!
//! Process counters live in `scripts/measure.ps1`. Usage work is split so
//! integrity / limits-tail / checkpoint bytes are never treated as "already
//! read usage was re-aggregated."

use super::DiagnosticSnapshot;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PerfScenario {
    CleanFirstLaunch,
    WarmRestart,
    Jsonl1000,
    LargeCorpus,
    Append4KiB,
    MonthRollover,
    CatchUpRestart,
    Truncate,
    Replace,
    Rebuild,
    Soak8h,
    VendorApiUnavailable,
    NoClaudeOrCodex,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScenarioRunStatus {
    Defined,
    Smoke,
    NotRun,
}

impl PerfScenario {
    pub const ALL: [Self; 13] = [
        Self::CleanFirstLaunch,
        Self::WarmRestart,
        Self::Jsonl1000,
        Self::LargeCorpus,
        Self::Append4KiB,
        Self::MonthRollover,
        Self::CatchUpRestart,
        Self::Truncate,
        Self::Replace,
        Self::Rebuild,
        Self::Soak8h,
        Self::VendorApiUnavailable,
        Self::NoClaudeOrCodex,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CleanFirstLaunch => "clean-first-launch",
            Self::WarmRestart => "warm-restart",
            Self::Jsonl1000 => "1000-jsonl",
            Self::LargeCorpus => "large-corpus",
            Self::Append4KiB => "4kib-append",
            Self::MonthRollover => "month-rollover",
            Self::CatchUpRestart => "catch-up-restart",
            Self::Truncate => "truncate",
            Self::Replace => "replace",
            Self::Rebuild => "rebuild",
            Self::Soak8h => "8h-soak",
            Self::VendorApiUnavailable => "vendor-api-unavailable",
            Self::NoClaudeOrCodex => "no-claude-codex",
        }
    }

    /// This crate run only smoke-checks the catalog. 8h soak is never executed here.
    #[must_use]
    pub const fn default_status(self) -> ScenarioRunStatus {
        match self {
            Self::WarmRestart | Self::Append4KiB | Self::Truncate | Self::Replace => {
                ScenarioRunStatus::Smoke
            }
            Self::Soak8h => ScenarioRunStatus::NotRun,
            _ => ScenarioRunStatus::Defined,
        }
    }
}

/// Split usage-work counters so a warm restart is not judged by one byte total.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UsageWorkCounters {
    pub usage_parse_bytes: u64,
    pub integrity_probe_bytes: u64,
    pub limits_tail_bytes: u64,
    pub checkpoint_bytes: u64,
    pub checkpoint_write_bytes: u64,
}

impl UsageWorkCounters {
    #[must_use]
    pub fn from_snapshot(snapshot: &DiagnosticSnapshot) -> Self {
        Self {
            usage_parse_bytes: snapshot.usage_parse_bytes,
            integrity_probe_bytes: snapshot.integrity_probe_bytes,
            limits_tail_bytes: snapshot.limits_tail_bytes,
            checkpoint_bytes: snapshot.checkpoint_bytes,
            checkpoint_write_bytes: snapshot.checkpoint_write_bytes,
        }
    }
}

/// Unchanged files after a warm restart must not re-parse already-read JSONL.
/// Integrity probes and limits-tail reads may still do I/O.
#[must_use]
pub fn warm_restart_does_not_reaggregate(counters: &UsageWorkCounters) -> bool {
    counters.usage_parse_bytes == 0
}

#[must_use]
pub fn warm_restart_allows_auxiliary_io(counters: &UsageWorkCounters) -> bool {
    counters.integrity_probe_bytes > 0 || counters.limits_tail_bytes > 0
}

#[cfg(test)]
mod tests {
    use super::{
        warm_restart_allows_auxiliary_io, warm_restart_does_not_reaggregate, PerfScenario,
        ScenarioRunStatus, UsageWorkCounters,
    };

    #[test]
    fn component_catalog_marks_eight_hour_soak_not_run() {
        assert_eq!(
            PerfScenario::Soak8h.default_status(),
            ScenarioRunStatus::NotRun
        );
        assert_eq!(PerfScenario::ALL.len(), 13);
        assert!(PerfScenario::ALL
            .iter()
            .any(|scenario| scenario.name() == "warm-restart"));
    }

    #[test]
    fn component_warm_restart_oracle_ignores_integrity_and_limits_tail() {
        let unchanged = UsageWorkCounters {
            usage_parse_bytes: 0,
            integrity_probe_bytes: 4_096,
            limits_tail_bytes: 256,
            checkpoint_bytes: 128,
            checkpoint_write_bytes: 0,
        };
        assert!(warm_restart_does_not_reaggregate(&unchanged));
        assert!(warm_restart_allows_auxiliary_io(&unchanged));

        let reaggregated = UsageWorkCounters {
            usage_parse_bytes: 12,
            ..unchanged
        };
        assert!(!warm_restart_does_not_reaggregate(&reaggregated));
    }
}
