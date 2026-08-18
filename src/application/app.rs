use crate::core::{
    AnimationController, AppSettings, CpuBreakdown, CpuSampler, FpsLimit, FrameCursor, GpuStatus,
    MemoryStatus, ProcessStatus, ResolvedTheme, Sparkline, StorageStatus, SystemTimes,
    ThemePreference, UsageSnapshot,
};

use super::{
    commit_protocol::CommitStatus, effect::COMMIT_DEADLINE_MS, Effect, Event, TimerKind, TrayIcon,
};

pub const CPU_SAMPLE_INTERVAL_MS: u32 = 2_000;
pub const ANIMATION_FRAME_COUNT: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingCommit {
    operation_id: u64,
    settings: AppSettings,
    previous: AppSettings,
    sync_run_entry: bool,
}

/// Pure application state. All operating-system calls are expressed as
/// [`Effect`] values returned by [`Self::start`] or [`Self::dispatch`].
#[derive(Clone, Debug)]
pub struct App {
    settings: AppSettings,
    settings_generation: u64,
    last_operation_id: u64,
    next_operation_id: u64,
    system_theme: ResolvedTheme,
    resolved_theme: ResolvedTheme,
    sampler: CpuSampler,
    animation: AnimationController,
    frames: FrameCursor,
    memory: Option<MemoryStatus>,
    storage: Option<StorageStatus>,
    gpu: Option<GpuStatus>,
    cpu_sparkline: Sparkline,
    memory_sparkline: Sparkline,
    gpu_sparkline: Sparkline,
    usage: UsageSnapshot,
    process: Option<ProcessStatus>,
    tooltip: String,
    running: bool,
    pending_commit: Option<PendingCommit>,
    clock_millis: u64,
}

/// Observable state used by non-live integration tests and diagnostics.
#[derive(Clone, Debug, PartialEq)]
pub struct AppSnapshot {
    pub settings: AppSettings,
    pub settings_generation: u64,
    pub last_operation_id: u64,
    pub system_theme: ResolvedTheme,
    pub resolved_theme: ResolvedTheme,
    pub frame: usize,
    pub animation_fps: u16,
    pub tooltip: String,
    pub cpu_sparkline: Sparkline,
    pub memory_sparkline: Sparkline,
    pub gpu_sparkline: Sparkline,
    pub cpu_breakdown: Option<CpuBreakdown>,
    pub storage: Option<StorageStatus>,
    pub gpu: Option<GpuStatus>,
    pub usage: UsageSnapshot,
    pub process: Option<ProcessStatus>,
    pub running: bool,
    pub pending_commit: Option<AppSettings>,
}

impl App {
    #[must_use]
    pub fn new(settings: AppSettings, system_theme: ResolvedTheme) -> Self {
        Self::with_persistence(settings, 0, 0, system_theme)
    }

    #[must_use]
    pub fn with_generation(
        settings: AppSettings,
        settings_generation: u64,
        system_theme: ResolvedTheme,
    ) -> Self {
        Self::with_persistence(settings, settings_generation, 0, system_theme)
    }

    #[must_use]
    pub fn with_persistence(
        settings: AppSettings,
        settings_generation: u64,
        last_operation_id: u64,
        system_theme: ResolvedTheme,
    ) -> Self {
        let resolved_theme = settings.theme.resolve(system_theme);
        Self {
            animation: AnimationController::new(settings.fps_limit),
            settings,
            settings_generation,
            last_operation_id,
            next_operation_id: last_operation_id.saturating_add(1),
            system_theme,
            resolved_theme,
            sampler: CpuSampler::default(),
            frames: FrameCursor::new(ANIMATION_FRAME_COUNT)
                .expect("RunDog embeds a fixed non-empty frame set"),
            memory: None,
            storage: None,
            gpu: None,
            cpu_sparkline: Sparkline::new(),
            memory_sparkline: Sparkline::new(),
            gpu_sparkline: Sparkline::new(),
            usage: UsageSnapshot::default(),
            process: None,
            tooltip: format_tooltip(None, None, None),
            running: false,
            pending_commit: None,
            clock_millis: 0,
        }
    }

    pub fn set_clock_millis(&mut self, clock_millis: u64) {
        self.clock_millis = clock_millis;
    }

    #[must_use]
    pub fn snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            settings: self.settings,
            settings_generation: self.settings_generation,
            last_operation_id: self.last_operation_id,
            system_theme: self.system_theme,
            resolved_theme: self.resolved_theme,
            frame: self.frames.current(),
            animation_fps: self.animation.current_fps(),
            tooltip: self.tooltip.clone(),
            cpu_sparkline: self.cpu_sparkline,
            memory_sparkline: self.memory_sparkline,
            gpu_sparkline: self.gpu_sparkline,
            cpu_breakdown: self.sampler.latest_breakdown(),
            storage: self.storage,
            gpu: self.gpu,
            usage: self.usage,
            process: self.process,
            running: self.running,
            pending_commit: self.pending_commit.map(|pending| pending.settings),
        }
    }

    /// Starts the application once. A duplicate start is a no-op, which makes
    /// the window bootstrap idempotent.
    #[must_use]
    pub fn start(&mut self) -> Vec<Effect> {
        if self.running {
            return Vec::new();
        }
        self.running = true;
        vec![
            Effect::AddTray(self.tray_icon()),
            Effect::SetTimer {
                kind: TimerKind::CpuSampling,
                interval_ms: CPU_SAMPLE_INTERVAL_MS,
            },
            Effect::SetTimer {
                kind: TimerKind::Animation,
                interval_ms: self.animation.current_interval_ms(),
            },
            Effect::SetThemeMenu(self.settings.theme),
            Effect::SetFpsMenu(self.settings.fps_limit),
            Effect::SetStartupMenu(self.settings.launch_at_startup),
        ]
    }

    /// Applies one input event and returns the required platform actions.
    #[must_use]
    pub fn dispatch(&mut self, event: Event) -> Vec<Effect> {
        if !self.running {
            return Vec::new();
        }

        match event {
            Event::CpuSample {
                times,
                memory,
                storage,
                gpu,
                process,
            } => self.handle_cpu_sample(times, memory, storage, gpu, process),
            Event::AnimationTimerElapsed => {
                self.frames.advance();
                vec![Effect::ModifyTray(self.tray_icon())]
            }
            Event::SystemThemeChanged(theme) => self.handle_system_theme_change(theme),
            Event::SelectTheme(theme) => self.handle_theme_selection(theme),
            Event::SelectFpsLimit(limit) => self.handle_fps_selection(limit),
            Event::ToggleStartup => self.handle_startup_toggle(),
            Event::SettingsCommitFinished {
                settings,
                status,
                new_generation,
                last_operation_id,
            } => self.handle_settings_commit_finished(
                settings,
                status,
                new_generation,
                last_operation_id,
            ),
            Event::TrayActivated => vec![Effect::LaunchTaskManager],
            Event::TaskbarRecreated => vec![Effect::AddTray(self.tray_icon())],
            Event::UsageSample(usage) => {
                if self.usage == usage {
                    Vec::new()
                } else {
                    self.usage = usage;
                    vec![Effect::ModifyTray(self.tray_icon())]
                }
            }
            Event::ExitRequested => self.handle_exit(),
        }
    }

    fn handle_cpu_sample(
        &mut self,
        sample: SystemTimes,
        memory: Option<MemoryStatus>,
        storage: Option<StorageStatus>,
        gpu: Option<GpuStatus>,
        process: Option<ProcessStatus>,
    ) -> Vec<Effect> {
        let cpu_load = self.sampler.push(sample);
        if memory.is_some_and(|status| status.usage_percent().is_some()) {
            self.memory = memory;
        }
        if storage.is_some_and(|status| status.used_percent().is_some()) {
            self.storage = storage;
        }
        if gpu.is_some() {
            self.gpu = gpu;
        }
        let process_changed = process.is_some_and(|status| self.process != Some(status));
        if let Some(status) = process {
            self.process = Some(status);
        }

        if let Some(_load) = cpu_load {
            let spark = self
                .sampler
                .latest_breakdown()
                .map(|breakdown| breakdown.total.value())
                .unwrap_or(0.0);
            self.cpu_sparkline.push(spark);
            if let Some(percent) = self.memory.and_then(MemoryStatus::usage_percent) {
                self.memory_sparkline.push(percent);
            }
            if let Some(percent) = self.gpu.and_then(GpuStatus::utilization_percent) {
                self.gpu_sparkline.push(percent);
            }
        }

        let next_tooltip = format_tooltip(
            self.sampler.latest().map(|load| load.value()),
            self.memory,
            self.gpu,
        );
        let tooltip_changed = self.tooltip != next_tooltip;
        self.tooltip = next_tooltip;

        let mut effects = Vec::with_capacity(2);
        if tooltip_changed || cpu_load.is_some() || process_changed {
            effects.push(Effect::ModifyTray(self.tray_icon()));
        }
        if let Some(load) = cpu_load {
            if let Some(change) = self.animation.update(load) {
                effects.push(Effect::SetTimer {
                    kind: TimerKind::Animation,
                    interval_ms: change.interval_ms,
                });
            }
        }
        effects
    }

    fn handle_system_theme_change(&mut self, theme: ResolvedTheme) -> Vec<Effect> {
        self.system_theme = theme;
        let resolved_theme = self.settings.theme.resolve(theme);
        if self.resolved_theme == resolved_theme {
            return Vec::new();
        }
        self.resolved_theme = resolved_theme;
        vec![Effect::ModifyTray(self.tray_icon())]
    }

    fn handle_theme_selection(&mut self, theme: ThemePreference) -> Vec<Effect> {
        if self.settings.theme == theme || self.pending_commit.is_some() {
            return Vec::new();
        }
        let previous = self.settings;
        let mut settings = self.settings;
        settings.theme = theme;
        self.begin_commit(settings, previous, false)
    }

    fn handle_fps_selection(&mut self, limit: FpsLimit) -> Vec<Effect> {
        if self.settings.fps_limit == limit || self.pending_commit.is_some() {
            return Vec::new();
        }
        let previous = self.settings;
        let mut settings = self.settings;
        settings.fps_limit = limit;
        self.begin_commit(settings, previous, false)
    }

    fn handle_startup_toggle(&mut self) -> Vec<Effect> {
        if self.pending_commit.is_some() {
            return Vec::new();
        }
        let previous = self.settings;
        let mut settings = self.settings;
        settings.launch_at_startup = !settings.launch_at_startup;
        let mut effects = self.begin_commit(settings, previous, true);
        effects.insert(0, Effect::SetStartupMenu(settings.launch_at_startup));
        effects
    }

    fn begin_commit(
        &mut self,
        settings: AppSettings,
        previous: AppSettings,
        sync_run_entry: bool,
    ) -> Vec<Effect> {
        let operation_id = self.next_operation_id;
        self.next_operation_id = self.next_operation_id.saturating_add(1);
        self.pending_commit = Some(PendingCommit {
            operation_id,
            settings,
            previous,
            sync_run_entry,
        });
        vec![Effect::CommitSettings {
            operation_id,
            settings,
            previous,
            expected_generation: self.settings_generation,
            sync_run_entry,
            deadline_millis: self.clock_millis.saturating_add(COMMIT_DEADLINE_MS),
        }]
    }

    fn handle_settings_commit_finished(
        &mut self,
        settings: AppSettings,
        status: CommitStatus,
        new_generation: u64,
        last_operation_id: u64,
    ) -> Vec<Effect> {
        let Some(pending) = self.pending_commit.take() else {
            return Vec::new();
        };
        if pending.settings != settings {
            self.pending_commit = Some(pending);
            return Vec::new();
        }

        self.settings_generation = new_generation;
        self.last_operation_id = last_operation_id.max(self.last_operation_id);
        self.next_operation_id = self
            .next_operation_id
            .max(self.last_operation_id.saturating_add(1));

        if !matches!(status, CommitStatus::Applied | CommitStatus::Duplicate) {
            return vec![
                Effect::SetThemeMenu(self.settings.theme),
                Effect::SetFpsMenu(self.settings.fps_limit),
                Effect::SetStartupMenu(self.settings.launch_at_startup),
            ];
        }

        if status == CommitStatus::Duplicate {
            // Durable state already contains this logical operation.
            return vec![
                Effect::SetThemeMenu(self.settings.theme),
                Effect::SetFpsMenu(self.settings.fps_limit),
                Effect::SetStartupMenu(self.settings.launch_at_startup),
            ];
        }

        let previous_resolved_theme = self.resolved_theme;
        let previous_fps_limit = self.settings.fps_limit;
        self.settings = settings;
        self.resolved_theme = settings.theme.resolve(self.system_theme);

        let mut effects = vec![
            Effect::SetThemeMenu(self.settings.theme),
            Effect::SetFpsMenu(self.settings.fps_limit),
            Effect::SetStartupMenu(self.settings.launch_at_startup),
        ];
        if previous_resolved_theme != self.resolved_theme {
            effects.push(Effect::ModifyTray(self.tray_icon()));
        }
        if previous_fps_limit != self.settings.fps_limit {
            if let Some(change) = self.animation.set_limit(self.settings.fps_limit) {
                effects.push(Effect::SetTimer {
                    kind: TimerKind::Animation,
                    interval_ms: change.interval_ms,
                });
            }
        }
        let _ = pending.previous;
        let _ = pending.sync_run_entry;
        effects
    }

    fn handle_exit(&mut self) -> Vec<Effect> {
        self.running = false;
        let mut effects = Vec::new();
        if let Some(pending) = self.pending_commit.take() {
            effects.push(Effect::CancelCommit {
                operation_id: pending.operation_id,
            });
        }
        effects.extend([
            Effect::KillTimer(TimerKind::CpuSampling),
            Effect::KillTimer(TimerKind::Animation),
            Effect::RemoveTray,
            Effect::SaveSettings(self.settings),
            Effect::Quit,
        ]);
        effects
    }

    fn tray_icon(&self) -> TrayIcon {
        TrayIcon {
            theme: self.resolved_theme,
            frame: self.frames.current(),
            tooltip: self.tooltip.clone(),
            cpu_sparkline: self.cpu_sparkline,
            memory_sparkline: self.memory_sparkline,
            gpu_sparkline: self.gpu_sparkline,
            cpu_breakdown: self.sampler.latest_breakdown(),
            memory: self.memory,
            storage: self.storage,
            gpu: self.gpu,
            usage: self.usage,
            process: self.process,
        }
    }
}

fn format_percent(value: Option<f32>) -> String {
    match value {
        Some(percent) => format!("{percent:.1}%"),
        None => "--.-%".to_owned(),
    }
}

fn format_tooltip(
    cpu_percent: Option<f32>,
    memory: Option<MemoryStatus>,
    gpu: Option<GpuStatus>,
) -> String {
    format!(
        "CPU: {}\nMemory: {}\nGPU: {}",
        format_percent(cpu_percent),
        format_percent(memory.and_then(MemoryStatus::usage_percent)),
        format_percent(gpu.and_then(GpuStatus::utilization_percent))
    )
}

#[cfg(test)]
mod tests {
    use super::{App, CPU_SAMPLE_INTERVAL_MS};
    use crate::{
        application::{CommitStatus, Effect, Event, TimerKind},
        core::{AppSettings, FpsLimit, MemoryStatus, ResolvedTheme, SystemTimes, ThemePreference},
    };

    fn started_app() -> App {
        let mut app = App::new(AppSettings::default(), ResolvedTheme::Dark);
        let _ = app.start();
        app
    }

    #[test]
    fn c2_start_is_idempotent_and_initialises_only_expected_timers() {
        let mut app = App::new(AppSettings::default(), ResolvedTheme::Dark);
        let effects = app.start();
        assert!(effects.contains(&Effect::SetTimer {
            kind: TimerKind::CpuSampling,
            interval_ms: CPU_SAMPLE_INTERVAL_MS,
        }));
        assert!(effects.contains(&Effect::SetTimer {
            kind: TimerKind::Animation,
            interval_ms: 200,
        }));
        assert!(app.start().is_empty());
    }

    #[test]
    fn c2_event_paths_cover_ignored_first_sample_change_and_unchanged_paths() {
        let mut app = started_app();
        assert!(app
            .dispatch(Event::cpu_sample(SystemTimes::new(0, 0, 0)))
            .is_empty());
        let effects = app.dispatch(Event::cpu_sample(SystemTimes::new(0, 100, 0)));
        assert!(effects.contains(&Effect::SetTimer {
            kind: TimerKind::Animation,
            interval_ms: 50,
        }));
        assert!(app
            .dispatch(Event::SystemThemeChanged(ResolvedTheme::Dark))
            .is_empty());
        assert!(app
            .dispatch(Event::SelectTheme(ThemePreference::System))
            .is_empty());
        assert!(app
            .dispatch(Event::SelectFpsLimit(FpsLimit::Fps20))
            .is_empty());
    }

    #[test]
    fn c2_theme_changes_wait_for_durable_ack_before_updating_memory() {
        let mut app = started_app();
        let effects = app.dispatch(Event::SelectTheme(ThemePreference::Dark));
        assert!(matches!(
            effects.as_slice(),
            [Effect::CommitSettings {
                operation_id: 1,
                settings: AppSettings {
                    theme: ThemePreference::Dark,
                    ..
                },
                expected_generation: 0,
                sync_run_entry: false,
                ..
            }]
        ));
        assert_eq!(app.snapshot().settings.theme, ThemePreference::System);

        let effects = app.dispatch(Event::SettingsCommitFinished {
            settings: AppSettings {
                theme: ThemePreference::Dark,
                ..AppSettings::default()
            },
            status: CommitStatus::Applied,
            new_generation: 1,
            last_operation_id: 1,
        });
        assert!(effects.contains(&Effect::SetThemeMenu(ThemePreference::Dark)));
        assert_eq!(app.snapshot().settings.theme, ThemePreference::Dark);
        assert_eq!(app.snapshot().settings_generation, 1);
        assert_eq!(app.snapshot().last_operation_id, 1);
    }

    #[test]
    fn component_tooltip_shows_memory_percent_independently_of_the_first_cpu_sample() {
        let mut app = started_app();
        assert_eq!(
            app.snapshot().tooltip,
            "CPU: --.-%\nMemory: --.-%\nGPU: --.-%"
        );

        let effects = app.dispatch(Event::CpuSample {
            times: SystemTimes::new(0, 0, 0),
            memory: Some(MemoryStatus::new(16_u64 << 30, 8_u64 << 30)),
            storage: None,
            gpu: Some(
                crate::core::GpuStatus::new(8_u64 << 30, 2_u64 << 30, 16_u64 << 30, 1_u64 << 30)
                    .with_utilization(Some(12.5)),
            ),
            process: None,
        });
        assert!(effects.contains(&Effect::ModifyTray(app.tray_icon())));
        assert_eq!(
            app.snapshot().tooltip,
            "CPU: --.-%\nMemory: 50.0%\nGPU: 12.5%"
        );
        assert_eq!(app.snapshot().cpu_sparkline.len(), 0);

        let _ = app.dispatch(Event::CpuSample {
            times: SystemTimes::new(0, 100, 0),
            memory: Some(MemoryStatus::new(16_u64 << 30, 4_u64 << 30)),
            storage: Some(crate::core::StorageStatus::new(100, 25)),
            gpu: Some(
                crate::core::GpuStatus::new(8_u64 << 30, 4_u64 << 30, 16_u64 << 30, 2_u64 << 30)
                    .with_utilization(Some(40.0)),
            ),
            process: Some(crate::core::ProcessStatus::new(
                5 * 1024 * 1024,
                Some(crate::core::CpuLoad::percent(0.4)),
            )),
        });
        assert_eq!(
            app.snapshot().tooltip,
            "CPU: 100.0%\nMemory: 75.0%\nGPU: 40.0%"
        );
        assert_eq!(app.snapshot().cpu_sparkline.len(), 1);
        assert_eq!(app.snapshot().memory_sparkline.len(), 1);
        assert_eq!(app.snapshot().gpu_sparkline.len(), 1);
        assert_eq!(
            app.snapshot()
                .cpu_breakdown
                .map(|breakdown| breakdown.total.value()),
            Some(100.0)
        );
        assert_eq!(
            app.snapshot()
                .storage
                .and_then(crate::core::StorageStatus::used_percent),
            Some(75.0)
        );

        assert_eq!(
            app.snapshot().process.map(|status| status.private_bytes),
            Some(5 * 1024 * 1024)
        );

        let effects = app.dispatch(Event::CpuSample {
            times: SystemTimes::new(0, 200, 0),
            memory: Some(MemoryStatus::new(0, 0)),
            storage: Some(crate::core::StorageStatus::new(0, 0)),
            gpu: None,
            process: None,
        });
        assert_eq!(
            app.snapshot().tooltip,
            "CPU: 100.0%\nMemory: 75.0%\nGPU: 40.0%"
        );
        assert_eq!(app.snapshot().cpu_sparkline.len(), 2);
        assert_eq!(app.snapshot().memory_sparkline.len(), 2);
        assert_eq!(app.snapshot().gpu_sparkline.len(), 2);
        assert!(effects.contains(&Effect::ModifyTray(app.tray_icon())));
    }

    #[test]
    fn component_usage_sample_updates_tray_only_when_the_snapshot_changes() {
        let mut app = started_app();
        let mut usage = crate::core::UsageSnapshot::default();
        usage.claude.today_cents = 725;
        let effects = app.dispatch(Event::UsageSample(usage));
        assert!(effects.contains(&Effect::ModifyTray(app.tray_icon())));
        assert_eq!(app.snapshot().usage.claude.today_cents, 725);
        assert!(app.dispatch(Event::UsageSample(usage)).is_empty());
    }

    #[test]
    fn c2_exit_cancels_in_flight_commit_before_quit() {
        let mut app = started_app();
        let _ = app.dispatch(Event::SelectTheme(ThemePreference::Dark));
        let effects = app.dispatch(Event::ExitRequested);
        assert!(effects.contains(&Effect::CancelCommit { operation_id: 1 }));
        assert!(effects.contains(&Effect::Quit));
        assert!(!app.snapshot().running);
    }
}
