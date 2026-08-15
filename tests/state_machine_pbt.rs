//! Property-based, non-live integration test for arbitrary application event
//! sequences. The port contains no Win32 implementation.

use proptest::prelude::*;
use run_dog::{
    application::{dispatch_and_execute, App, Effect, EffectPort, Event, ANIMATION_FRAME_COUNT},
    core::{AppSettings, FpsLimit, ResolvedTheme, SystemTimes, ThemePreference},
};

#[derive(Default)]
struct NonLivePort {
    effects: Vec<Effect>,
}

impl EffectPort for NonLivePort {
    fn apply(&mut self, effect: &Effect) {
        self.effects.push(effect.clone());
    }

    fn set_startup(&mut self, _enabled: bool) -> bool {
        true
    }
}

fn event_from_code(code: u8, times: &mut SystemTimes) -> Event {
    match code % 10 {
        0 => {
            let kernel_delta = 100_u64;
            let idle_delta = u64::from(code) * 10;
            let user_delta = u64::from(code % 4) * 10;
            times.idle += idle_delta;
            times.kernel += kernel_delta;
            times.user += user_delta;
            Event::CpuSample(*times)
        }
        1 => Event::AnimationTimerElapsed,
        2 => Event::SystemThemeChanged(ResolvedTheme::Light),
        3 => Event::SystemThemeChanged(ResolvedTheme::Dark),
        4 => Event::SelectTheme(ThemePreference::System),
        5 => Event::SelectTheme(ThemePreference::Light),
        6 => Event::SelectTheme(ThemePreference::Dark),
        7 => Event::SelectFpsLimit(FpsLimit::Fps10),
        8 => Event::ToggleStartup,
        _ => Event::TaskbarRecreated,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2_048))]

    #[test]
    fn pbt_arbitrary_nonlive_event_sequences_preserve_application_invariants(
        event_codes in proptest::collection::vec(any::<u8>(), 0..128),
    ) {
        let mut app = App::new(AppSettings::default(), ResolvedTheme::Dark);
        let mut port = NonLivePort::default();
        for effect in app.start() {
            port.apply(&effect);
        }
        let mut times = SystemTimes::default();

        for code in event_codes {
            dispatch_and_execute(&mut app, &mut port, event_from_code(code, &mut times));
            let snapshot = app.snapshot();
            prop_assert!(snapshot.frame < ANIMATION_FRAME_COUNT);
            prop_assert!(snapshot.animation_fps >= 5);
            prop_assert!(snapshot.animation_fps <= snapshot.settings.fps_limit.fps());
            for effect in &port.effects {
                if let Effect::SetTimer { interval_ms, .. } = effect {
                    prop_assert!(*interval_ms > 0);
                }
            }
        }
    }
}
