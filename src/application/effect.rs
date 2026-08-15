use crate::core::{AppSettings, FpsLimit, ResolvedTheme, ThemePreference};

/// Timer identities are stable values, so the Windows adapter never needs to
/// derive a timer ID from a pointer or allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimerKind {
    CpuSampling,
    Animation,
}

/// Complete visual state for a shell tray notification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrayIcon {
    pub theme: ResolvedTheme,
    pub frame: usize,
    pub tooltip: String,
}

/// An external action requested by [`crate::application::App`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    AddTray(TrayIcon),
    ModifyTray(TrayIcon),
    RemoveTray,
    SetTimer { kind: TimerKind, interval_ms: u32 },
    KillTimer(TimerKind),
    SaveSettings(AppSettings),
    SetThemeMenu(ThemePreference),
    SetFpsMenu(FpsLimit),
    SetStartupMenu(bool),
    RequestStartup(bool),
    LaunchTaskManager,
    Quit,
}
