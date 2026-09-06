use crate::core::{
    AppSettings, CpuBreakdown, FpsLimit, GpuStatus, MemoryStatus, ProcessStatus, ResolvedTheme,
    Sparkline, StorageStatus, ThemePreference, TrayDisplayMode, TrayMetrics, UsageSnapshot,
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
    pub display_mode: TrayDisplayMode,
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

impl TrayIcon {
    #[must_use]
    pub fn metrics(&self) -> TrayMetrics {
        TrayMetrics {
            cpu_percent: self.cpu_breakdown.map(|breakdown| breakdown.total.value()),
            memory_percent: self.memory.and_then(MemoryStatus::usage_percent),
            gpu_percent: self.gpu.and_then(GpuStatus::utilization_percent),
            claude_session: self.usage.claude.session_window(),
            claude_week: self.usage.claude.weekly_window(),
            claude_fable: self.usage.claude.fable,
            codex_session: self.usage.codex.session_window(),
            codex_week: self.usage.codex.weekly_window(),
        }
    }
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
    SetDisplayMenu(TrayDisplayMode),
    SetStartupMenu(bool),
    /// User-visible confirmation after a successful Launch-at-startup toggle.
    NotifyStartupChanged(bool),
    LaunchTaskManager,
    Quit,
}

#[cfg(test)]
mod tests {
    use super::TrayIcon;
    use crate::core::{
        CpuBreakdown, CpuLoad, GpuStatus, LimitWindow, MemoryStatus, ProviderUsage, ResolvedTheme,
        Sparkline, TrayDisplayMode, UsageSnapshot,
    };

    #[test]
    fn component_tray_metrics_read_cpu_memory_gpu_and_codex_week() {
        let icon = TrayIcon {
            theme: ResolvedTheme::Dark,
            frame: 0,
            display_mode: TrayDisplayMode::Cpu,
            tooltip: String::new(),
            cpu_sparkline: Sparkline::new(),
            memory_sparkline: Sparkline::new(),
            gpu_sparkline: Sparkline::new(),
            cpu_breakdown: Some(CpuBreakdown {
                total: CpuLoad::percent(41.6),
                system: CpuLoad::percent(10.0),
                user: CpuLoad::percent(31.6),
                idle: CpuLoad::percent(58.4),
            }),
            memory: Some(MemoryStatus::new(8_u64 << 30, 2_u64 << 30)),
            storage: None,
            gpu: Some(
                GpuStatus::new(8_u64 << 30, 1_u64 << 30, 16_u64 << 30, 0)
                    .with_utilization(Some(9.2)),
            ),
            usage: UsageSnapshot {
                claude: ProviderUsage {
                    primary: Some(LimitWindow {
                        used_tenths: 180,
                        resets_at_ms: 9_000,
                        window_minutes: 300,
                    }),
                    secondary: Some(LimitWindow {
                        used_tenths: 620,
                        resets_at_ms: 9_000,
                        window_minutes: 10_080,
                    }),
                    fable: Some(LimitWindow {
                        used_tenths: 275,
                        resets_at_ms: 9_000,
                        window_minutes: 10_080,
                    }),
                    ..ProviderUsage::default()
                },
                codex: ProviderUsage {
                    primary: Some(LimitWindow {
                        used_tenths: 90,
                        resets_at_ms: 9_000,
                        window_minutes: 300,
                    }),
                    secondary: Some(LimitWindow {
                        used_tenths: 255,
                        resets_at_ms: 9_000,
                        window_minutes: 10_080,
                    }),
                    ..ProviderUsage::default()
                },
                ..UsageSnapshot::default()
            },
            process: None,
        };
        let metrics = icon.metrics();
        assert_eq!(metrics.cpu_percent, Some(41.6));
        assert_eq!(metrics.memory_percent, Some(75.0));
        assert_eq!(metrics.gpu_percent, Some(9.2));
        assert_eq!(
            metrics.claude_session.map(|window| window.used_tenths),
            Some(180)
        );
        assert_eq!(
            metrics.claude_week.map(|window| window.used_tenths),
            Some(620)
        );
        assert_eq!(
            metrics.claude_fable.map(|window| window.used_tenths),
            Some(275)
        );
        assert_eq!(
            metrics.codex_session.map(|window| window.used_tenths),
            Some(90)
        );
        assert_eq!(
            metrics.codex_week.map(|window| window.used_tenths),
            Some(255)
        );
    }
}
