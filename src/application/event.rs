use crate::core::{AppSettings, FpsLimit, ResolvedTheme, SystemTimes, ThemePreference};

use super::commit_protocol::CommitStatus;

/// An input to the application state machine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    CpuSample(SystemTimes),
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
    ExitRequested,
}
