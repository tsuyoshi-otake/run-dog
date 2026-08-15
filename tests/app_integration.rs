//! Non-live integration tests. Every external boundary is a deterministic fake:
//! no HKCU writes, no tray icon, no process launch, and no live CPU/clock read.

use std::collections::VecDeque;

use run_dog::{
    application::{
        dispatch_and_execute, dispatch_cpu_tick, App, Clock, CpuSource, Effect, EffectPort, Event,
        SettingsStore, ThemeSource, TimerKind, TrayIcon, ANIMATION_FRAME_COUNT,
    },
    core::{AppSettings, FpsLimit, ResolvedTheme, SystemTimes, ThemePreference},
};

#[derive(Default)]
struct FakeClock {
    milliseconds: u64,
}

impl FakeClock {
    fn advance(&mut self, milliseconds: u64) {
        self.milliseconds += milliseconds;
    }
}

impl Clock for FakeClock {
    fn now_millis(&self) -> u64 {
        self.milliseconds
    }
}

struct FakeCpu {
    samples: VecDeque<SystemTimes>,
}

impl FakeCpu {
    fn new(samples: impl IntoIterator<Item = SystemTimes>) -> Self {
        Self {
            samples: samples.into_iter().collect(),
        }
    }
}

impl CpuSource for FakeCpu {
    fn read_system_times(&mut self) -> Option<SystemTimes> {
        self.samples.pop_front()
    }
}

struct FakeThemeSource {
    theme: ResolvedTheme,
}

impl ThemeSource for FakeThemeSource {
    fn system_theme(&mut self) -> ResolvedTheme {
        self.theme
    }
}

/// One object represents the in-memory settings store, fake tray/scheduler,
/// fake startup registry, and fake process launcher at the test boundary.
struct FakePlatform {
    stored_settings: AppSettings,
    saved_settings: Vec<AppSettings>,
    effects: Vec<Effect>,
    tray: Option<TrayIcon>,
    active_timers: Vec<(TimerKind, u32)>,
    startup_results: VecDeque<bool>,
    startup_requests: Vec<bool>,
    task_manager_launches: usize,
    quit_requested: bool,
}

impl FakePlatform {
    fn new(settings: AppSettings, startup_results: impl IntoIterator<Item = bool>) -> Self {
        Self {
            stored_settings: settings,
            saved_settings: Vec::new(),
            effects: Vec::new(),
            tray: None,
            active_timers: Vec::new(),
            startup_results: startup_results.into_iter().collect(),
            startup_requests: Vec::new(),
            task_manager_launches: 0,
            quit_requested: false,
        }
    }

    fn timer_interval(&self, kind: TimerKind) -> Option<u32> {
        self.active_timers
            .iter()
            .find_map(|(candidate, interval)| (*candidate == kind).then_some(*interval))
    }

    fn record_timer(&mut self, kind: TimerKind, interval_ms: u32) {
        if let Some((_, existing_interval)) = self
            .active_timers
            .iter_mut()
            .find(|(candidate, _)| *candidate == kind)
        {
            *existing_interval = interval_ms;
        } else {
            self.active_timers.push((kind, interval_ms));
        }
    }
}

impl SettingsStore for FakePlatform {
    fn load(&mut self) -> AppSettings {
        self.stored_settings
    }

    fn save(&mut self, settings: AppSettings) {
        self.stored_settings = settings;
        self.saved_settings.push(settings);
    }
}

impl EffectPort for FakePlatform {
    fn apply(&mut self, effect: &Effect) {
        self.effects.push(effect.clone());
        match effect {
            Effect::AddTray(icon) | Effect::ModifyTray(icon) => self.tray = Some(icon.clone()),
            Effect::RemoveTray => self.tray = None,
            Effect::SetTimer { kind, interval_ms } => self.record_timer(*kind, *interval_ms),
            Effect::KillTimer(kind) => self
                .active_timers
                .retain(|(candidate, _)| candidate != kind),
            Effect::SaveSettings(settings) => self.save(*settings),
            Effect::LaunchTaskManager => self.task_manager_launches += 1,
            Effect::Quit => self.quit_requested = true,
            Effect::SetThemeMenu(_)
            | Effect::SetFpsMenu(_)
            | Effect::SetStartupMenu(_)
            | Effect::RequestStartup(_) => {}
        }
    }

    fn set_startup(&mut self, enabled: bool) -> bool {
        self.startup_requests.push(enabled);
        self.startup_results.pop_front().unwrap_or(true)
    }
}

struct TestRig {
    app: App,
    clock: FakeClock,
    cpu: FakeCpu,
    platform: FakePlatform,
}

impl TestRig {
    fn boot(
        settings: AppSettings,
        system_theme: ResolvedTheme,
        samples: impl IntoIterator<Item = SystemTimes>,
        startup_results: impl IntoIterator<Item = bool>,
    ) -> Self {
        let mut platform = FakePlatform::new(settings, startup_results);
        let loaded_settings = platform.load();
        let mut theme_source = FakeThemeSource {
            theme: system_theme,
        };
        let mut app = App::new(loaded_settings, theme_source.system_theme());
        for effect in app.start() {
            platform.apply(&effect);
        }
        Self {
            app,
            clock: FakeClock::default(),
            cpu: FakeCpu::new(samples),
            platform,
        }
    }

    fn cpu_tick(&mut self) {
        self.clock.advance(2_000);
        dispatch_cpu_tick(&mut self.app, &mut self.cpu, &mut self.platform);
    }

    fn event(&mut self, event: Event) {
        dispatch_and_execute(&mut self.app, &mut self.platform, event);
    }
}

#[test]
fn integration_boot_uses_only_fake_ports_and_configures_the_two_timers() {
    let rig = TestRig::boot(AppSettings::default(), ResolvedTheme::Dark, [], []);

    assert_eq!(rig.clock.now_millis(), 0);
    assert_eq!(
        rig.platform.timer_interval(TimerKind::CpuSampling),
        Some(2_000)
    );
    assert_eq!(rig.platform.timer_interval(TimerKind::Animation), Some(200));
    assert_eq!(
        rig.platform.tray.as_ref().map(|tray| tray.tooltip.as_str()),
        Some("CPU: --.-%")
    );
    assert_eq!(rig.app.snapshot().frame, 0);
}

#[test]
fn integration_cpu_sampling_changes_speed_without_rearming_an_unchanged_timer() {
    let mut rig = TestRig::boot(
        AppSettings::default(),
        ResolvedTheme::Dark,
        [
            SystemTimes::new(0, 0, 0),
            SystemTimes::new(0, 100, 0),
            SystemTimes::new(0, 200, 0),
        ],
        [],
    );

    rig.cpu_tick();
    assert_eq!(rig.clock.now_millis(), 2_000);
    assert_eq!(
        rig.platform.tray.as_ref().map(|tray| tray.tooltip.as_str()),
        Some("CPU: --.-%")
    );

    rig.cpu_tick();
    assert_eq!(rig.platform.timer_interval(TimerKind::Animation), Some(50));
    assert_eq!(
        rig.platform.tray.as_ref().map(|tray| tray.tooltip.as_str()),
        Some("CPU: 100.0%")
    );
    let rearm_count = rig
        .platform
        .effects
        .iter()
        .filter(|effect| {
            matches!(
                effect,
                Effect::SetTimer {
                    kind: TimerKind::Animation,
                    ..
                }
            )
        })
        .count();

    rig.cpu_tick();
    assert_eq!(
        rig.platform
            .effects
            .iter()
            .filter(|effect| matches!(
                effect,
                Effect::SetTimer {
                    kind: TimerKind::Animation,
                    ..
                }
            ))
            .count(),
        rearm_count
    );

    rig.event(Event::AnimationTimerElapsed);
    assert_eq!(rig.app.snapshot().frame, 1);
    assert!(rig.app.snapshot().frame < ANIMATION_FRAME_COUNT);
}

#[test]
fn integration_theme_and_fps_selections_persist_to_the_fake_setting_store() {
    let mut rig = TestRig::boot(AppSettings::default(), ResolvedTheme::Dark, [], []);

    rig.event(Event::SelectTheme(ThemePreference::Dark));
    assert_eq!(
        rig.platform
            .saved_settings
            .last()
            .map(|settings| settings.theme),
        Some(ThemePreference::Dark)
    );
    assert_eq!(rig.app.snapshot().resolved_theme, ResolvedTheme::Dark);

    rig.event(Event::SystemThemeChanged(ResolvedTheme::Light));
    assert_eq!(rig.app.snapshot().resolved_theme, ResolvedTheme::Dark);

    rig.event(Event::SelectTheme(ThemePreference::System));
    rig.event(Event::SelectFpsLimit(FpsLimit::Fps10));
    assert_eq!(rig.app.snapshot().resolved_theme, ResolvedTheme::Light);
    assert_eq!(rig.app.snapshot().settings.fps_limit, FpsLimit::Fps10);
    assert_eq!(rig.platform.stored_settings.fps_limit, FpsLimit::Fps10);
}

#[test]
fn integration_startup_failure_rolls_back_and_success_commits_without_hkcu() {
    let mut rig = TestRig::boot(
        AppSettings::default(),
        ResolvedTheme::Dark,
        [],
        [false, true],
    );

    rig.event(Event::ToggleStartup);
    assert_eq!(rig.platform.startup_requests, vec![true]);
    assert!(!rig.app.snapshot().settings.launch_at_startup);
    assert!(!rig.platform.stored_settings.launch_at_startup);

    rig.event(Event::ToggleStartup);
    assert_eq!(rig.platform.startup_requests, vec![true, true]);
    assert!(rig.app.snapshot().settings.launch_at_startup);
    assert!(rig.platform.stored_settings.launch_at_startup);
}

#[test]
fn integration_taskbar_recovery_activation_and_exit_stay_inside_fake_adapters() {
    let mut rig = TestRig::boot(AppSettings::default(), ResolvedTheme::Dark, [], []);
    let original_adds = rig
        .platform
        .effects
        .iter()
        .filter(|effect| matches!(effect, Effect::AddTray(_)))
        .count();

    rig.event(Event::TaskbarRecreated);
    rig.event(Event::TrayActivated);
    rig.event(Event::ExitRequested);

    assert_eq!(
        rig.platform
            .effects
            .iter()
            .filter(|effect| matches!(effect, Effect::AddTray(_)))
            .count(),
        original_adds + 1
    );
    assert_eq!(rig.platform.task_manager_launches, 1);
    assert!(rig.platform.active_timers.is_empty());
    assert!(rig.platform.tray.is_none());
    assert!(rig.platform.quit_requested);
    assert!(!rig.app.snapshot().running);
}
