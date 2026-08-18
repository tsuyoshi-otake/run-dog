use crate::core::{
    AppSettings, CpuBreakdown, FpsLimit, GpuStatus, MemoryStatus, ProcessStatus, ResolvedTheme,
    Sparkline, StorageStatus, ThemePreference, UsageSnapshot,
};

/// Timer identities are stable values, so the Windows adapter never needs to
/// derive a timer ID from a pointer or allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimerKind {
    CpuSampling,
    Animation,
}

/// Complete visual state for a shell tray notification.
#[derive(Clone, Debug, PartialEq)]
pub struct TrayIcon {
    pub theme: ResolvedTheme,
    pub frame: usize,
    pub tooltip: String,
    pub cpu_sparkline: Sparkline,
    pub memory_sparkline: Sparkline,
    pub gpu_sparkline: Sparkline,
    pub cpu_breakdown: Option<CpuBreakdown>,
    pub memory: Option<MemoryStatus>,
    pub storage: Option<StorageStatus>,
    pub gpu: Option<GpuStatus>,
    pub usage: UsageSnapshot,
    pub process: Option<ProcessStatus>,
}

/// Default wall-clock budget for one settings/Run commit saga.
pub const COMMIT_DEADLINE_MS: u64 = 5_000;

/// An external action requested by [`crate::application::App`].
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    AddTray(TrayIcon),
    ModifyTray(TrayIcon),
    RemoveTray,
    SetTimer {
        kind: TimerKind,
        interval_ms: u32,
    },
    KillTimer(TimerKind),
    /// Persist `settings` as one generation under a unique operation ID.
    CommitSettings {
        operation_id: u64,
        settings: AppSettings,
        previous: AppSettings,
        expected_generation: u64,
        sync_run_entry: bool,
        deadline_millis: u64,
    },
    /// Best-effort persistence during exit; failures are not surfaced.
    SaveSettings(AppSettings),
    /// Marks the in-flight commit operation cancelled so late completions are
    /// ignored.
    CancelCommit {
        operation_id: u64,
    },
    SetThemeMenu(ThemePreference),
    SetFpsMenu(FpsLimit),
    SetStartupMenu(bool),
    LaunchTaskManager,
    Quit,
}
