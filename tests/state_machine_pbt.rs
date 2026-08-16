//! Property-based, non-live integration test for arbitrary application event
//! sequences. The port contains no Win32 implementation.

use proptest::prelude::*;
use run_dog::{
    application::{
        dispatch_and_execute, execute_commit, App, CommitGate, CommitOutcome, CommitRequest,
        DurableStore, Effect, EffectPort, Event, ANIMATION_FRAME_COUNT,
    },
    core::{
        AppSettings, FpsLimit, PendingJournal, ResolvedTheme, SettingsRecord, SystemTimes,
        ThemePreference,
    },
};

#[derive(Default)]
struct NonLivePort {
    effects: Vec<Effect>,
    record: Option<SettingsRecord>,
    pending: Option<PendingJournal>,
    gate: CommitGate,
}

impl NonLivePort {
    fn record(&self) -> SettingsRecord {
        self.record
            .unwrap_or_else(|| SettingsRecord::new(0, 0, AppSettings::default()))
    }
}

impl DurableStore for NonLivePort {
    fn load_record(&mut self) -> SettingsRecord {
        self.record()
    }

    fn write_record(&mut self, record: SettingsRecord, expected_generation: u64) -> bool {
        if self.record().generation != expected_generation {
            return false;
        }
        self.record = Some(record);
        true
    }

    fn load_pending(&mut self) -> Option<PendingJournal> {
        self.pending
    }

    fn write_pending(&mut self, journal: &PendingJournal) -> bool {
        self.pending = Some(*journal);
        true
    }

    fn clear_pending(&mut self) -> bool {
        self.pending = None;
        true
    }

    fn is_tombstoned(&mut self) -> bool {
        false
    }

    fn set_startup(&mut self, _enabled: bool) -> bool {
        true
    }

    fn now_millis(&self) -> u64 {
        self.gate.now_millis()
    }

    fn is_cancelled(&self, operation_id: u64) -> bool {
        self.gate.is_cancelled(operation_id)
    }

    fn mark_timed_out(&mut self, operation_id: u64) {
        self.gate.mark_timed_out(operation_id);
    }

    fn mark_cancelled(&mut self, operation_id: u64) {
        self.gate.cancel(operation_id);
    }

    fn is_timed_out(&self, operation_id: u64) -> bool {
        self.gate.is_timed_out(operation_id)
    }
}

impl EffectPort for NonLivePort {
    fn apply(&mut self, effect: &Effect) {
        self.effects.push(effect.clone());
    }

    fn execute_commit(&mut self, request: CommitRequest) -> CommitOutcome {
        execute_commit(self, request)
    }

    fn cancel_commit(&mut self, operation_id: u64) {
        self.gate.cancel(operation_id);
    }

    fn now_millis(&self) -> u64 {
        self.gate.now_millis()
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
            Event::CpuSample {
                times: *times,
                memory: None,
                storage: None,
            }
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
