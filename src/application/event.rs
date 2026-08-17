use crate::core::{
    AppSettings, FpsLimit, MemoryStatus, ProcessStatus, ResolvedTheme, StorageStatus, SystemTimes,
    ThemePreference, UsageSnapshot,
};

use super::commit_protocol::CommitStatus;

/// An input to the application state machine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    CpuSample {
        times: SystemTimes,
        memory: Option<MemoryStatus>,
        storage: Option<StorageStatus>,
        process: Option<ProcessStatus>,
    },
    AnimationTimerElapsed,
    SystemThemeChanged(ResolvedTheme),
    SelectTheme(ThemePreference),
    SelectFpsLimit(FpsLimit),
    ToggleStartup,
    SettingsCommitFinished {
        settings: AppSettings,
        status: CommitStatus,
        new_generation: u64,
        last_operation_id: u64,
    },
    TrayActivated,
    TaskbarRecreated,
    UsageSample(UsageSnapshot),
    ExitRequested,
}

impl Event {
    #[must_use]
    pub const fn cpu_sample(times: SystemTimes) -> Self {
        Self::CpuSample {
            times,
            memory: None,
            storage: None,
            process: None,
        }
    }
}
