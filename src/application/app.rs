use crate::core::{
    AnimationController, AppSettings, CpuSampler, FpsLimit, FrameCursor, ResolvedTheme,
    SystemTimes, ThemePreference,
};

use super::{Effect, Event, TimerKind, TrayIcon};

pub const CPU_SAMPLE_INTERVAL_MS: u32 = 2_000;
pub const ANIMATION_FRAME_COUNT: usize = 3;

/// Pure application state. All operating-system calls are expressed as
/// [`Effect`] values returned by [`Self::start`] or [`Self::dispatch`].
#[derive(Clone, Debug)]
pub struct App {
    settings: AppSettings,
    system_theme: ResolvedTheme,
    resolved_theme: ResolvedTheme,
    sampler: CpuSampler,
    animation: AnimationController,
    frames: FrameCursor,
    tooltip: String,
    running: bool,
    pending_startup_change: Option<bool>,
}

/// Observable state used by non-live integration tests and diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppSnapshot {
    pub settings: AppSettings,
    pub system_theme: ResolvedTheme,
    pub resolved_theme: ResolvedTheme,
    pub frame: usize,
    pub animation_fps: u16,
    pub tooltip: String,
    pub running: bool,
    pub pending_startup_change: Option<bool>,
}

impl App {
    #[must_use]
    pub fn new(settings: AppSettings, system_theme: ResolvedTheme) -> Self {
        let resolved_theme = settings.theme.resolve(system_theme);
        Self {
            animation: AnimationController::new(settings.fps_limit),
            settings,
            system_theme,
            resolved_theme,
            sampler: CpuSampler::default(),
            frames: FrameCursor::new(ANIMATION_FRAME_COUNT)
                .expect("RunDog embeds a fixed non-empty frame set"),
            tooltip: "CPU: --.-%".to_owned(),
            running: false,
            pending_startup_change: None,
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            settings: self.settings,
            system_theme: self.system_theme,
            resolved_theme: self.resolved_theme,
            frame: self.frames.current(),
            animation_fps: self.animation.current_fps(),
            tooltip: self.tooltip.clone(),
            running: self.running,
            pending_startup_change: self.pending_startup_change,
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
            Event::StartupChangeFinished { enabled, succeeded } => {
                self.handle_startup_result(enabled, succeeded)
            }
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
        if self.settings.theme == theme {
            return Vec::new();
        }
        self.settings.theme = theme;
        let previous_resolved_theme = self.resolved_theme;
        self.resolved_theme = theme.resolve(self.system_theme);

        let mut effects = vec![
            Effect::SaveSettings(self.settings),
            Effect::SetThemeMenu(self.settings.theme),
        ];
        if previous_resolved_theme != self.resolved_theme {
            effects.push(Effect::ModifyTray(self.tray_icon()));
        }
        effects
    }

    fn handle_fps_selection(&mut self, limit: FpsLimit) -> Vec<Effect> {
        if self.settings.fps_limit == limit {
            return Vec::new();
        }
        self.settings.fps_limit = limit;
        let rate_change = self.animation.set_limit(limit);

        let mut effects = vec![
            Effect::SaveSettings(self.settings),
            Effect::SetFpsMenu(self.settings.fps_limit),
        ];
        if let Some(change) = rate_change {
            effects.push(Effect::SetTimer {
                kind: TimerKind::Animation,
                interval_ms: change.interval_ms,
            });
        }
        effects
    }

    fn handle_startup_toggle(&mut self) -> Vec<Effect> {
        if self.pending_startup_change.is_some() {
            return Vec::new();
        }
        let requested = !self.settings.launch_at_startup;
        self.pending_startup_change = Some(requested);
        vec![Effect::RequestStartup(requested)]
    }

    fn handle_startup_result(&mut self, enabled: bool, succeeded: bool) -> Vec<Effect> {
        if self.pending_startup_change != Some(enabled) {
            return Vec::new();
        }
        self.pending_startup_change = None;

        if !succeeded {
            return vec![Effect::SetStartupMenu(self.settings.launch_at_startup)];
        }

        self.settings.launch_at_startup = enabled;
        vec![
            Effect::SaveSettings(self.settings),
            Effect::SetStartupMenu(self.settings.launch_at_startup),
        ]
    }

    fn handle_exit(&mut self) -> Vec<Effect> {
        self.running = false;
        self.pending_startup_change = None;
        vec![
            Effect::KillTimer(TimerKind::CpuSampling),
            Effect::KillTimer(TimerKind::Animation),
            Effect::RemoveTray,
            Effect::SaveSettings(self.settings),
            Effect::Quit,
        ]
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
        application::{Effect, Event, TimerKind},
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
    fn c2_theme_changes_only_modify_icon_when_resolved_theme_changes() {
        let mut app = started_app();
        let effects = app.dispatch(Event::SelectTheme(ThemePreference::Dark));
        assert_eq!(effects.len(), 2);
        assert_eq!(app.snapshot().resolved_theme, ResolvedTheme::Dark);
        let effects = app.dispatch(Event::SelectTheme(ThemePreference::Light));
        assert!(matches!(effects.last(), Some(Effect::ModifyTray(_))));
    }

    #[test]
    fn c2_startup_results_cover_busy_mismatch_failure_and_success() {
        let mut app = started_app();
        assert_eq!(
            app.dispatch(Event::ToggleStartup),
            vec![Effect::RequestStartup(true)]
        );
        assert!(app.dispatch(Event::ToggleStartup).is_empty());
        assert!(app
            .dispatch(Event::StartupChangeFinished {
                enabled: false,
                succeeded: true
            })
            .is_empty());
        assert_eq!(
            app.dispatch(Event::StartupChangeFinished {
                enabled: true,
                succeeded: false
            }),
            vec![Effect::SetStartupMenu(false)]
        );
        assert_eq!(
            app.dispatch(Event::ToggleStartup),
            vec![Effect::RequestStartup(true)]
        );
        assert_eq!(
            app.dispatch(Event::StartupChangeFinished {
                enabled: true,
                succeeded: true
            }),
            vec![
                Effect::SaveSettings(AppSettings {
                    launch_at_startup: true,
                    ..AppSettings::default()
                }),
                Effect::SetStartupMenu(true),
            ]
        );
    }

    #[test]
    fn c2_exit_stops_lifecycle_and_drops_later_events() {
        let mut app = started_app();
        let effects = app.dispatch(Event::ExitRequested);
        assert!(effects.contains(&Effect::Quit));
        assert!(!app.snapshot().running);
        assert!(app.dispatch(Event::AnimationTimerElapsed).is_empty());
        assert!(app.dispatch(Event::ExitRequested).is_empty());
    }
}
