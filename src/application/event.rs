use crate::core::{FpsLimit, ResolvedTheme, SystemTimes, ThemePreference};

/// An input to the application state machine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    CpuSample(SystemTimes),
    AnimationTimerElapsed,
    SystemThemeChanged(ResolvedTheme),
    SelectTheme(ThemePreference),
    SelectFpsLimit(FpsLimit),
    ToggleStartup,
    StartupChangeFinished { enabled: bool, succeeded: bool },
    TrayActivated,
    TaskbarRecreated,
    ExitRequested,
}
