use crate::core::{
    AnimationController, AppSettings, CpuSampler, FpsLimit, FrameCursor, ResolvedTheme,
    SystemTimes, ThemePreference,
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
    tooltip: String,
    running: bool,
    pending_commit: Option<PendingCommit>,
    clock_millis: u64,
}

/// Observable state used by non-live integration tests and diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppSnapshot {
    pub settings: AppSettings,
    pub settings_generation: u64,
    pub last_operation_id: u64,
    pub system_theme: ResolvedTheme,
    pub resolved_theme: ResolvedTheme,
    pub frame: usize,
    pub animation_fps: u16,
    pub tooltip: String,
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
            tooltip: "CPU: --.-%".to_owned(),
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
            Event::CpuSample(sample) => self.handle_cpu_sample(sample),
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
            Event::ExitRequested => self.handle_exit(),
        }
    }

    fn handle_cpu_sample(&mut self, sample: SystemTimes) -> Vec<Effect> {
        let Some(load) = self.sampler.push(sample) else {
            return Vec::new();
        };

        let next_tooltip = format!("CPU: {:.1}%", load.value());
        let tooltip_changed = self.tooltip != next_tooltip;
        self.tooltip = next_tooltip;
        let rate_change = self.animation.update(load);

        let mut effects = Vec::with_capacity(2);
        if tooltip_changed {
            effects.push(Effect::ModifyTray(self.tray_icon()));
        }
        if let Some(change) = rate_change {
            effects.push(Effect::SetTimer {
                kind: TimerKind::Animation,
                interval_ms: change.interval_ms,
            });
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
        self.begin_commit(settings, previous, true)
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{App, CPU_SAMPLE_INTERVAL_MS};
    use crate::{
        application::{CommitStatus, Effect, Event, TimerKind},
        core::{AppSettings, FpsLimit, ResolvedTheme, SystemTimes, ThemePreference},
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
            .dispatch(Event::CpuSample(SystemTimes::new(0, 0, 0)))
            .is_empty());
        let effects = app.dispatch(Event::CpuSample(SystemTimes::new(0, 100, 0)));
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
    fn c2_exit_cancels_in_flight_commit_before_quit() {
        let mut app = started_app();
        let _ = app.dispatch(Event::SelectTheme(ThemePreference::Dark));
        let effects = app.dispatch(Event::ExitRequested);
        assert!(effects.contains(&Effect::CancelCommit { operation_id: 1 }));
        assert!(effects.contains(&Effect::Quit));
        assert!(!app.snapshot().running);
    }
}
