//! Non-live persistence protocol tests covering atomicity, operation IDs,
//! timeout/cancel late-success ignore, tombstones, and crash recovery.

use std::collections::BTreeSet;

use proptest::{
    prelude::*,
    test_runner::{Config as ProptestConfig, FileFailurePersistence},
};
use run_dog::{
    application::{
        dispatch_and_execute, execute_commit, recover_pending, App, CommitGate, CommitOutcome,
        CommitRequest, CommitStatus, DurableStore, Effect, EffectPort, Event, SettingsStore,
    },
    core::{AppSettings, FpsLimit, PendingJournal, ResolvedTheme, SettingsRecord, ThemePreference},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FaultPoint {
    WritePending,
    WriteRecord,
    SetRunValue,
}

#[derive(Clone, Debug, Default)]
struct FailurePlan {
    points: Vec<FaultPoint>,
}

impl FailurePlan {
    fn once(point: FaultPoint) -> Self {
        Self {
            points: vec![point],
        }
    }

    fn take(&mut self, point: FaultPoint) -> bool {
        if self.points.first().copied() == Some(point) {
            self.points.remove(0);
            true
        } else {
            false
        }
    }
}

struct MemoryStore {
    record: SettingsRecord,
    pending: Option<PendingJournal>,
    run_value_present: bool,
    tombstoned: bool,
    gate: CommitGate,
    failures: FailurePlan,
    startup_requests: Vec<bool>,
}

impl MemoryStore {
    fn new(record: SettingsRecord) -> Self {
        Self {
            record,
            pending: None,
            run_value_present: record.settings.launch_at_startup,
            tombstoned: false,
            gate: CommitGate::with_clock(0),
            failures: FailurePlan::default(),
            startup_requests: Vec::new(),
        }
    }

    fn failing_at(mut self, point: FaultPoint) -> Self {
        self.failures = FailurePlan::once(point);
        self
    }
}

impl DurableStore for MemoryStore {
    fn load_record(&mut self) -> SettingsRecord {
        self.record
    }

    fn write_record(&mut self, record: SettingsRecord, expected_generation: u64) -> bool {
        if self.tombstoned || self.record.generation != expected_generation {
            return false;
        }
        if self.failures.take(FaultPoint::WriteRecord) {
            return false;
        }
        self.record = record;
        true
    }

    fn load_pending(&mut self) -> Option<PendingJournal> {
        self.pending
    }

    fn write_pending(&mut self, journal: &PendingJournal) -> bool {
        if self.tombstoned || self.failures.take(FaultPoint::WritePending) {
            return false;
        }
        self.pending = Some(*journal);
        true
    }

    fn clear_pending(&mut self) -> bool {
        self.pending = None;
        true
    }

    fn is_tombstoned(&mut self) -> bool {
        self.tombstoned
    }

    fn set_startup(&mut self, enabled: bool) -> bool {
        self.startup_requests.push(enabled);
        if self.failures.take(FaultPoint::SetRunValue) {
            return false;
        }
        self.run_value_present = enabled;
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

impl SettingsStore for MemoryStore {
    fn load(&mut self) -> AppSettings {
        self.record.settings
    }

    fn load_generation(&mut self) -> u64 {
        self.record.generation
    }

    fn load_last_operation_id(&mut self) -> u64 {
        self.record.last_operation_id
    }
}

impl EffectPort for MemoryStore {
    fn apply(&mut self, _effect: &Effect) {}

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

fn settings(theme: ThemePreference, fps: FpsLimit, startup: bool) -> AppSettings {
    AppSettings {
        theme,
        fps_limit: fps,
        launch_at_startup: startup,
    }
}

fn start_app(store: &mut MemoryStore) -> App {
    let mut app = App::with_persistence(
        store.load(),
        store.load_generation(),
        store.load_last_operation_id(),
        ResolvedTheme::Dark,
    );
    for effect in app.start() {
        store.apply(&effect);
    }
    app
}

#[test]
fn api_integration_successful_intents_advance_generation_and_operation_id() {
    let mut store = MemoryStore::new(SettingsRecord::new(0, 0, AppSettings::default()));
    let mut app = start_app(&mut store);

    dispatch_and_execute(
        &mut app,
        &mut store,
        Event::SelectTheme(ThemePreference::Dark),
    );
    assert_eq!(app.snapshot().settings.theme, ThemePreference::Dark);
    assert_eq!(store.record.generation, 1);
    assert_eq!(store.record.last_operation_id, 1);

    dispatch_and_execute(&mut app, &mut store, Event::ToggleStartup);
    assert!(app.snapshot().settings.launch_at_startup);
    assert!(store.run_value_present);
    assert_eq!(store.record.generation, 2);
    assert_eq!(store.record.last_operation_id, 2);
}

#[test]
fn duplicate_operation_id_is_idempotent_and_does_not_mutate_twice() {
    let mut store = MemoryStore::new(SettingsRecord::new(
        1,
        7,
        settings(ThemePreference::Dark, FpsLimit::Fps20, false),
    ));
    let outcome = execute_commit(
        &mut store,
        CommitRequest {
            operation_id: 7,
            expected_generation: 1,
            settings: settings(ThemePreference::Light, FpsLimit::Fps40, true),
            previous: settings(ThemePreference::Dark, FpsLimit::Fps20, false),
            sync_run_entry: true,
            deadline_millis: 100,
        },
    );
    assert_eq!(outcome.status, CommitStatus::Duplicate);
    assert_eq!(store.record.generation, 1);
    assert_eq!(store.startup_requests, Vec::<bool>::new());
}

#[test]
fn timeout_before_mutation_leaves_previous_generation() {
    let mut store = MemoryStore::new(SettingsRecord::new(0, 0, AppSettings::default()));
    store.gate.set_now(50);
    let outcome = execute_commit(
        &mut store,
        CommitRequest {
            operation_id: 1,
            expected_generation: 0,
            settings: settings(ThemePreference::Dark, FpsLimit::Fps40, true),
            previous: AppSettings::default(),
            sync_run_entry: true,
            deadline_millis: 50,
        },
    );
    assert_eq!(outcome.status, CommitStatus::TimedOut);
    assert_eq!(store.record.generation, 0);
    assert!(store.pending.is_none());
}

#[test]
fn cancel_marks_operation_terminal_and_late_completion_is_ignored() {
    let mut store = MemoryStore::new(SettingsRecord::new(0, 0, AppSettings::default()));
    store.cancel_commit(1);
    let outcome = execute_commit(
        &mut store,
        CommitRequest {
            operation_id: 1,
            expected_generation: 0,
            settings: settings(ThemePreference::Dark, FpsLimit::Fps40, true),
            previous: AppSettings::default(),
            sync_run_entry: true,
            deadline_millis: 1_000,
        },
    );
    assert_eq!(outcome.status, CommitStatus::Cancelled);
    assert_eq!(store.record.settings, AppSettings::default());
}

#[test]
fn run_failure_rolls_settings_content_back_without_split() {
    let mut store = MemoryStore::new(SettingsRecord::new(0, 0, AppSettings::default()))
        .failing_at(FaultPoint::SetRunValue);
    let mut app = start_app(&mut store);
    dispatch_and_execute(&mut app, &mut store, Event::ToggleStartup);
    assert_eq!(app.snapshot().settings, AppSettings::default());
    assert!(!store.run_value_present);
    assert_eq!(store.record.settings, AppSettings::default());
    assert!(store.pending.is_none());
}

#[test]
fn tombstoned_store_refuses_recreate_writes() {
    let mut store = MemoryStore::new(SettingsRecord::new(3, 4, AppSettings::default()));
    store.tombstoned = true;
    let outcome = execute_commit(
        &mut store,
        CommitRequest {
            operation_id: 5,
            expected_generation: 3,
            settings: settings(ThemePreference::Dark, FpsLimit::Fps10, true),
            previous: AppSettings::default(),
            sync_run_entry: false,
            deadline_millis: 100,
        },
    );
    assert_eq!(outcome.status, CommitStatus::Tombstoned);
    assert_eq!(store.record.generation, 3);
}

#[test]
fn crash_recovery_completes_pending_run_sync() {
    let desired = settings(ThemePreference::System, FpsLimit::Fps20, true);
    let mut store = MemoryStore::new(SettingsRecord::new(1, 9, desired));
    store.pending = Some(PendingJournal {
        operation_id: 9,
        base_generation: 0,
        desired,
        previous: AppSettings::default(),
        sync_run_entry: true,
        deadline_millis: 10_000,
    });
    store.run_value_present = false;
    let outcome = recover_pending(&mut store);
    assert_eq!(outcome.status, CommitStatus::Applied);
    assert!(store.run_value_present);
    assert!(store.pending.is_none());
}

#[test]
fn crash_recovery_rolls_back_expired_pending_desired_state() {
    let desired = settings(ThemePreference::Dark, FpsLimit::Fps40, true);
    let mut store = MemoryStore::new(SettingsRecord::new(1, 3, desired));
    store.gate.set_now(5_000);
    store.pending = Some(PendingJournal {
        operation_id: 3,
        base_generation: 0,
        desired,
        previous: AppSettings::default(),
        sync_run_entry: true,
        deadline_millis: 1_000,
    });
    let outcome = recover_pending(&mut store);
    assert_eq!(outcome.status, CommitStatus::TimedOut);
    assert_eq!(store.record.settings, AppSettings::default());
    assert!(store.pending.is_none());
}

#[test]
fn resource_exhaustion_keeps_ui_and_durable_on_old_generation() {
    let mut store = MemoryStore::new(SettingsRecord::new(0, 0, AppSettings::default()))
        .failing_at(FaultPoint::WritePending);
    let mut app = start_app(&mut store);
    dispatch_and_execute(
        &mut app,
        &mut store,
        Event::SelectTheme(ThemePreference::Dark),
    );
    assert_eq!(app.snapshot().settings.theme, ThemePreference::System);
    assert_eq!(store.record.settings.theme, ThemePreference::System);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1_024,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "verification/evidence/pbt-counterexamples.regressions",
        ))),
        .. ProptestConfig::default()
    })]

    #[test]
    fn pbt_faulted_settings_write_must_not_expose_a_partial_generation(
        generation in 0_u64..16,
        operation_id in 1_u64..32,
    ) {
        let initial = SettingsRecord::new(generation, operation_id.saturating_sub(1), AppSettings::default());
        let mut store = MemoryStore::new(initial).failing_at(FaultPoint::WriteRecord);
        let outcome = execute_commit(
            &mut store,
            CommitRequest {
                operation_id,
                expected_generation: generation,
                settings: settings(ThemePreference::Dark, FpsLimit::Fps40, true),
                previous: AppSettings::default(),
                sync_run_entry: false,
                deadline_millis: 100,
            },
        );
        prop_assert_eq!(outcome.status, CommitStatus::Failed);
        prop_assert_eq!(store.record, initial);
        prop_assert!(store.pending.is_none());
    }

    #[test]
    fn pbt_successful_user_intents_converge_to_the_independent_settings_model(
        intent_codes in prop::collection::vec(0_u8..8, 0..32),
    ) {
        let mut expected = SettingsRecord::new(0, 0, AppSettings::default());
        let mut applied_ids = BTreeSet::new();
        let mut store = MemoryStore::new(expected);
        let mut app = start_app(&mut store);

        for code in intent_codes {
            match code {
                0..=2 => {
                    let theme = ThemePreference::ALL[usize::from(code)];
                    if expected.settings.theme != theme {
                        expected.settings.theme = theme;
                        expected.generation += 1;
                        expected.last_operation_id += 1;
                        applied_ids.insert(expected.last_operation_id);
                    }
                    dispatch_and_execute(&mut app, &mut store, Event::SelectTheme(theme));
                }
                3..=6 => {
                    let limit = FpsLimit::ALL[usize::from(code - 3)];
                    if expected.settings.fps_limit != limit {
                        expected.settings.fps_limit = limit;
                        expected.generation += 1;
                        expected.last_operation_id += 1;
                        applied_ids.insert(expected.last_operation_id);
                    }
                    dispatch_and_execute(&mut app, &mut store, Event::SelectFpsLimit(limit));
                }
                7 => {
                    expected.settings.launch_at_startup = !expected.settings.launch_at_startup;
                    expected.generation += 1;
                    expected.last_operation_id += 1;
                    applied_ids.insert(expected.last_operation_id);
                    dispatch_and_execute(&mut app, &mut store, Event::ToggleStartup);
                }
                _ => unreachable!(),
            }
            prop_assert_eq!(app.snapshot().settings, expected.settings);
            prop_assert_eq!(store.record, expected);
            prop_assert_eq!(store.run_value_present, expected.settings.launch_at_startup);
            prop_assert_eq!(app.snapshot().pending_commit, None);
        }
        prop_assert_eq!(applied_ids.len() as u64, expected.last_operation_id);
    }
}
