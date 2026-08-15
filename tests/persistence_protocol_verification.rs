//! Adversarial, non-live verification of the settings and startup protocol.
//!
//! `RegistryProtocolAdapter` deliberately models the externally observable
//! Win32 registry protocol: the three settings values are independently
//! written and the HKCU Run value is a separate operation.  It does *not*
//! call Win32.  `AtomicCommitOracle`, in contrast, is an independent domain
//! rule: a user-visible configuration generation either commits as one tuple
//! or leaves the preceding tuple intact.

use std::collections::{BTreeSet, VecDeque};

use proptest::{
    prelude::*,
    test_runner::{Config as ProptestConfig, FileFailurePersistence},
};
use run_dog::{
    application::{dispatch_and_execute, App, Effect, EffectPort, Event, SettingsStore},
    core::{AppSettings, FpsLimit, ResolvedTheme, ThemePreference},
};

/// Individual calls visible at the Registry boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FaultPoint {
    OpenSettingsKey,
    WriteTheme,
    WriteFpsLimit,
    WriteStartupFlag,
    OpenRunKey,
    SetRunValue,
    DeleteRunValue,
}

/// Events retained by the test adapter.  They are a protocol trace, not an
/// oracle: assertions below derive expected durable state independently.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProtocolEvent {
    Attempt(FaultPoint),
    ReadTheme,
    ReadFpsLimit,
    ReadStartupFlag,
    ExternalCommit,
}

#[derive(Clone, Debug, Default)]
struct FailurePlan {
    points: VecDeque<FaultPoint>,
}

impl FailurePlan {
    fn once(point: FaultPoint) -> Self {
        Self {
            points: VecDeque::from([point]),
        }
    }

    fn take(&mut self, point: FaultPoint) -> bool {
        if self.points.front().copied() == Some(point) {
            let _ = self.points.pop_front();
            true
        } else {
            false
        }
    }
}

/// The physical layout relevant to the production protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RegistryImage {
    theme: Option<ThemePreference>,
    fps_limit: Option<FpsLimit>,
    launch_at_startup: Option<bool>,
    /// The production Run value is either absent or a command line.  Only its
    /// enabled/absent meaning belongs to this domain verification.
    run_value_present: bool,
}

impl RegistryImage {
    fn from_durable(state: DurableState) -> Self {
        Self {
            theme: Some(state.settings.theme),
            fps_limit: Some(state.settings.fps_limit),
            launch_at_startup: Some(state.settings.launch_at_startup),
            run_value_present: state.run_value_present,
        }
    }

    fn visible(self) -> DurableState {
        DurableState {
            settings: AppSettings::from_persisted(
                self.theme.map(ThemePreference::persisted_name),
                self.fps_limit.map(FpsLimit::persisted_name),
                self.launch_at_startup,
            ),
            run_value_present: self.run_value_present,
        }
    }
}

/// The externally meaningful configuration.  It intentionally has no
/// Registry-key or implementation-specific representation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DurableState {
    settings: AppSettings,
    run_value_present: bool,
}

impl DurableState {
    const fn new(settings: AppSettings, run_value_present: bool) -> Self {
        Self {
            settings,
            run_value_present,
        }
    }
}

/// Where an external writer completes between the production loader's reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadBoundary {
    AfterTheme,
    AfterFpsLimit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReadInterleave {
    boundary: ReadBoundary,
    replacement: DurableState,
}

/// A protocol-compatible, non-live test dependency.  Its write ordering and
/// error swallowing match `windows::registry::{save_settings,
/// set_launch_at_startup}`; its failure points emulate key exhaustion,
/// permission loss, a crash, or a transient external error.
struct RegistryProtocolAdapter {
    image: RegistryImage,
    failures: FailurePlan,
    trace: Vec<ProtocolEvent>,
    startup_requests: Vec<bool>,
    interleave: Option<ReadInterleave>,
}

impl RegistryProtocolAdapter {
    fn new(initial: DurableState) -> Self {
        Self {
            image: RegistryImage::from_durable(initial),
            failures: FailurePlan::default(),
            trace: Vec::new(),
            startup_requests: Vec::new(),
            interleave: None,
        }
    }

    fn failing_at(mut self, point: FaultPoint) -> Self {
        self.failures = FailurePlan::once(point);
        self
    }

    fn interleave_after(mut self, boundary: ReadBoundary, replacement: DurableState) -> Self {
        self.interleave = Some(ReadInterleave {
            boundary,
            replacement,
        });
        self
    }

    fn visible(&self) -> DurableState {
        self.image.visible()
    }

    fn external_commit(&mut self, replacement: DurableState) {
        self.trace.push(ProtocolEvent::ExternalCommit);
        self.image = RegistryImage::from_durable(replacement);
    }

    fn call_count(&self, point: FaultPoint) -> usize {
        self.trace
            .iter()
            .filter(|event| **event == ProtocolEvent::Attempt(point))
            .count()
    }

    fn allow(&mut self, point: FaultPoint) -> bool {
        self.trace.push(ProtocolEvent::Attempt(point));
        !self.failures.take(point)
    }

    fn save_settings_like_windows(&mut self, settings: AppSettings) {
        if !self.allow(FaultPoint::OpenSettingsKey) {
            return;
        }

        // These calls intentionally proceed after an earlier error, precisely
        // as the production adapter discards each `write_*` result.
        if self.allow(FaultPoint::WriteTheme) {
            self.image.theme = Some(settings.theme);
        }
        if self.allow(FaultPoint::WriteFpsLimit) {
            self.image.fps_limit = Some(settings.fps_limit);
        }
        if self.allow(FaultPoint::WriteStartupFlag) {
            self.image.launch_at_startup = Some(settings.launch_at_startup);
        }
    }

    fn set_startup_like_windows(&mut self, enabled: bool) -> bool {
        self.startup_requests.push(enabled);
        if !self.allow(FaultPoint::OpenRunKey) {
            return false;
        }

        let point = if enabled {
            FaultPoint::SetRunValue
        } else {
            FaultPoint::DeleteRunValue
        };
        if !self.allow(point) {
            return false;
        }
        self.image.run_value_present = enabled;
        true
    }

    fn interleave_if_requested(&mut self, boundary: ReadBoundary) {
        if self
            .interleave
            .is_some_and(|interleave| interleave.boundary == boundary)
        {
            let replacement = self
                .interleave
                .take()
                .expect("the checked interleave must still be present")
                .replacement;
            self.external_commit(replacement);
        }
    }
}

impl SettingsStore for RegistryProtocolAdapter {
    fn load(&mut self) -> AppSettings {
        let theme = self.image.theme;
        self.trace.push(ProtocolEvent::ReadTheme);
        self.interleave_if_requested(ReadBoundary::AfterTheme);

        let fps_limit = self.image.fps_limit;
        self.trace.push(ProtocolEvent::ReadFpsLimit);
        self.interleave_if_requested(ReadBoundary::AfterFpsLimit);

        let launch_at_startup = self.image.launch_at_startup;
        self.trace.push(ProtocolEvent::ReadStartupFlag);
        AppSettings::from_persisted(
            theme.map(ThemePreference::persisted_name),
            fps_limit.map(FpsLimit::persisted_name),
            launch_at_startup,
        )
    }

    fn save(&mut self, settings: AppSettings) {
        self.save_settings_like_windows(settings);
    }
}

impl EffectPort for RegistryProtocolAdapter {
    fn apply(&mut self, effect: &Effect) {
        if let Effect::SaveSettings(settings) = effect {
            self.save_settings_like_windows(*settings);
        }
    }

    fn set_startup(&mut self, enabled: bool) -> bool {
        self.set_startup_like_windows(enabled)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Candidate {
    operation_id: u64,
    generation: u64,
    state: DurableState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommitDecision {
    Applied,
    Duplicate,
    RejectedStaleOrEqual,
}

/// Independent reference rule for the required durable protocol.
///
/// A commit has one operation ID and a strictly increasing generation.  A
/// failed attempt does not call `submit`; therefore it cannot leak a partial
/// image.  This rule makes version and duplicate requirements explicit even
/// though the current registry representation has neither field.
struct AtomicCommitOracle {
    state: DurableState,
    generation: u64,
    applied_operation_ids: BTreeSet<u64>,
}

impl AtomicCommitOracle {
    fn new(initial: DurableState) -> Self {
        Self {
            state: initial,
            generation: 0,
            applied_operation_ids: BTreeSet::new(),
        }
    }

    fn state(&self) -> DurableState {
        self.state
    }

    fn submit(&mut self, candidate: Candidate) -> CommitDecision {
        if self.applied_operation_ids.contains(&candidate.operation_id) {
            return CommitDecision::Duplicate;
        }
        if candidate.generation <= self.generation {
            return CommitDecision::RejectedStaleOrEqual;
        }

        self.state = candidate.state;
        self.generation = candidate.generation;
        let inserted = self.applied_operation_ids.insert(candidate.operation_id);
        assert!(inserted, "a previously unseen operation ID must insert");
        CommitDecision::Applied
    }
}

fn default_state() -> DurableState {
    DurableState::new(AppSettings::default(), false)
}

fn settings(theme: ThemePreference, fps_limit: FpsLimit, launch_at_startup: bool) -> AppSettings {
    AppSettings {
        theme,
        fps_limit,
        launch_at_startup,
    }
}

fn start_nonlive_app(port: &mut RegistryProtocolAdapter) -> App {
    let mut app = App::new(port.load(), ResolvedTheme::Dark);
    for effect in app.start() {
        port.apply(&effect);
    }
    app
}

#[test]
fn api_integration_successful_user_intents_converge_with_the_independent_oracle() {
    let initial = default_state();
    let mut port = RegistryProtocolAdapter::new(initial);
    let mut app = start_nonlive_app(&mut port);
    let mut oracle = AtomicCommitOracle::new(initial);

    let selected_theme = ThemePreference::Dark;
    dispatch_and_execute(&mut app, &mut port, Event::SelectTheme(selected_theme));
    let theme_candidate =
        DurableState::new(settings(selected_theme, FpsLimit::Fps20, false), false);
    assert_eq!(
        oracle.submit(Candidate {
            operation_id: 1,
            generation: 1,
            state: theme_candidate,
        }),
        CommitDecision::Applied
    );
    assert_eq!(port.visible(), oracle.state());

    dispatch_and_execute(&mut app, &mut port, Event::ToggleStartup);
    let startup_candidate =
        DurableState::new(settings(selected_theme, FpsLimit::Fps20, true), true);
    assert_eq!(
        oracle.submit(Candidate {
            operation_id: 2,
            generation: 2,
            state: startup_candidate,
        }),
        CommitDecision::Applied
    );
    assert_eq!(app.snapshot().settings, startup_candidate.settings);
    assert_eq!(port.visible(), oracle.state());
    assert_eq!(port.startup_requests, vec![true]);
}

#[test]
fn c2_each_persistence_write_has_explicit_success_and_failure_outcomes() {
    let initial = default_state();
    let candidate = DurableState::new(
        settings(ThemePreference::Dark, FpsLimit::Fps40, true),
        false,
    );

    let mut success = RegistryProtocolAdapter::new(initial);
    success.apply(&Effect::SaveSettings(candidate.settings));
    assert_eq!(success.visible(), candidate);

    let partitions = [
        (
            FaultPoint::OpenSettingsKey,
            DurableState::new(AppSettings::default(), false),
        ),
        (
            FaultPoint::WriteTheme,
            DurableState::new(
                settings(ThemePreference::System, FpsLimit::Fps40, true),
                false,
            ),
        ),
        (
            FaultPoint::WriteFpsLimit,
            DurableState::new(
                settings(ThemePreference::Dark, FpsLimit::Fps20, true),
                false,
            ),
        ),
        (
            FaultPoint::WriteStartupFlag,
            DurableState::new(
                settings(ThemePreference::Dark, FpsLimit::Fps40, false),
                false,
            ),
        ),
    ];
    for (point, expected_physical_result) in partitions {
        let mut port = RegistryProtocolAdapter::new(initial).failing_at(point);
        port.apply(&Effect::SaveSettings(candidate.settings));
        assert_eq!(
            port.visible(),
            expected_physical_result,
            "fault at {point:?}"
        );
        assert_eq!(
            port.call_count(point),
            1,
            "fault point {point:?} was not reached"
        );
    }
}

#[test]
fn c2_startup_key_success_failure_and_later_settings_failure_are_distinct() {
    let initial = default_state();

    let mut key_failure = RegistryProtocolAdapter::new(initial).failing_at(FaultPoint::OpenRunKey);
    let mut app = start_nonlive_app(&mut key_failure);
    dispatch_and_execute(&mut app, &mut key_failure, Event::ToggleStartup);
    assert_eq!(app.snapshot().settings, initial.settings);
    assert_eq!(key_failure.visible(), initial);

    let mut settings_failure =
        RegistryProtocolAdapter::new(initial).failing_at(FaultPoint::WriteStartupFlag);
    let mut app = start_nonlive_app(&mut settings_failure);
    dispatch_and_execute(&mut app, &mut settings_failure, Event::ToggleStartup);
    assert!(app.snapshot().settings.launch_at_startup);
    assert!(settings_failure.visible().run_value_present);
    assert!(!settings_failure.visible().settings.launch_at_startup);
}

#[test]
fn failure_injection_detects_partial_settings_commit_against_atomic_oracle() {
    let initial = default_state();
    let candidate = DurableState::new(
        settings(ThemePreference::Dark, FpsLimit::Fps40, false),
        false,
    );
    let oracle = AtomicCommitOracle::new(initial);
    let mut port = RegistryProtocolAdapter::new(initial).failing_at(FaultPoint::WriteFpsLimit);

    port.apply(&Effect::SaveSettings(candidate.settings));

    // The injected operation failed, so the independent transactional rule
    // leaves the old generation durable.  The real protocol exposes a hybrid.
    assert_eq!(oracle.state(), initial);
    assert_ne!(port.visible(), oracle.state());
    assert_eq!(port.visible().settings.theme, ThemePreference::Dark);
    assert_eq!(port.visible().settings.fps_limit, FpsLimit::Fps20);
}

#[test]
fn api_integration_detects_run_value_then_settings_failure_split_commit() {
    let initial = default_state();
    let expected = DurableState::new(
        settings(ThemePreference::System, FpsLimit::Fps20, true),
        true,
    );
    let mut oracle = AtomicCommitOracle::new(initial);
    let mut port = RegistryProtocolAdapter::new(initial).failing_at(FaultPoint::WriteStartupFlag);
    let mut app = start_nonlive_app(&mut port);

    dispatch_and_execute(&mut app, &mut port, Event::ToggleStartup);
    assert_eq!(
        oracle.submit(Candidate {
            operation_id: 1,
            generation: 1,
            state: expected,
        }),
        CommitDecision::Applied
    );

    assert_eq!(app.snapshot().settings, expected.settings);
    assert_ne!(port.visible(), oracle.state());
    assert_eq!(
        port.visible(),
        DurableState::new(
            settings(ThemePreference::System, FpsLimit::Fps20, false),
            true
        )
    );
}

#[test]
fn loader_observes_a_torn_snapshot_when_an_external_generation_arrives_mid_read() {
    let before = default_state();
    let after = DurableState::new(settings(ThemePreference::Dark, FpsLimit::Fps40, true), true);
    let mut port =
        RegistryProtocolAdapter::new(before).interleave_after(ReadBoundary::AfterTheme, after);

    let observed = port.load();

    assert_ne!(observed, before.settings);
    assert_ne!(observed, after.settings);
    assert_eq!(observed.theme, ThemePreference::System);
    assert_eq!(observed.fps_limit, FpsLimit::Fps40);
    assert!(observed.launch_at_startup);
}

#[test]
fn stale_and_equal_generation_writers_are_rejected_by_oracle_but_not_by_registry_protocol() {
    let initial = default_state();
    let newer = DurableState::new(settings(ThemePreference::Dark, FpsLimit::Fps40, true), true);
    let stale = DurableState::new(
        settings(ThemePreference::Light, FpsLimit::Fps10, false),
        false,
    );
    let equal_generation = DurableState::new(
        settings(ThemePreference::System, FpsLimit::Fps30, true),
        true,
    );
    let mut oracle = AtomicCommitOracle::new(initial);
    assert_eq!(
        oracle.submit(Candidate {
            operation_id: 20,
            generation: 2,
            state: newer,
        }),
        CommitDecision::Applied
    );

    let mut port = RegistryProtocolAdapter::new(newer);
    port.apply(&Effect::SaveSettings(stale.settings));
    assert_eq!(
        oracle.submit(Candidate {
            operation_id: 10,
            generation: 1,
            state: stale,
        }),
        CommitDecision::RejectedStaleOrEqual
    );
    assert_ne!(port.visible(), oracle.state());

    port.external_commit(newer);
    port.apply(&Effect::SaveSettings(equal_generation.settings));
    assert_eq!(
        oracle.submit(Candidate {
            operation_id: 21,
            generation: 2,
            state: equal_generation,
        }),
        CommitDecision::RejectedStaleOrEqual
    );
    assert_ne!(port.visible(), oracle.state());
}

#[test]
fn retry_and_restart_expose_partial_state_even_when_a_later_retry_converges() {
    let initial = default_state();
    let candidate = DurableState::new(settings(ThemePreference::Dark, FpsLimit::Fps40, true), true);
    let mut port = RegistryProtocolAdapter::new(initial).failing_at(FaultPoint::WriteFpsLimit);

    port.apply(&Effect::SaveSettings(candidate.settings));
    let after_crash = port.load();
    assert_ne!(after_crash, initial.settings);
    assert_ne!(after_crash, candidate.settings);

    // The one-shot error has been consumed.  A duplicate retry writes every
    // physical value again and eventually converges, but cannot undo the
    // already observable hybrid generation.
    port.apply(&Effect::SaveSettings(candidate.settings));
    assert_eq!(port.visible().settings, candidate.settings);
    assert_eq!(port.call_count(FaultPoint::WriteTheme), 2);
    assert_eq!(port.call_count(FaultPoint::WriteFpsLimit), 2);
    assert_eq!(port.call_count(FaultPoint::WriteStartupFlag), 2);
}

#[test]
fn resource_exhaustion_keeps_durable_state_old_but_the_application_reports_new_state() {
    let initial = default_state();
    let mut port = RegistryProtocolAdapter::new(initial).failing_at(FaultPoint::OpenSettingsKey);
    let mut app = start_nonlive_app(&mut port);

    dispatch_and_execute(
        &mut app,
        &mut port,
        Event::SelectTheme(ThemePreference::Dark),
    );

    assert_eq!(port.visible(), initial);
    assert_eq!(app.snapshot().settings.theme, ThemePreference::Dark);
    let reloaded_after_restart = port.load();
    assert_eq!(reloaded_after_restart, initial.settings);
}

#[test]
fn oracle_is_monotonic_and_idempotent_for_duplicate_and_order_reversed_candidates() {
    let initial = default_state();
    let newer = DurableState::new(settings(ThemePreference::Dark, FpsLimit::Fps30, true), true);
    let older = DurableState::new(
        settings(ThemePreference::Light, FpsLimit::Fps10, false),
        false,
    );
    let mut oracle = AtomicCommitOracle::new(initial);
    let newest_candidate = Candidate {
        operation_id: 2,
        generation: 2,
        state: newer,
    };

    assert_eq!(oracle.submit(newest_candidate), CommitDecision::Applied);
    assert_eq!(oracle.submit(newest_candidate), CommitDecision::Duplicate);
    assert_eq!(
        oracle.submit(Candidate {
            operation_id: 1,
            generation: 1,
            state: older,
        }),
        CommitDecision::RejectedStaleOrEqual
    );
    assert_eq!(oracle.state(), newer);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 2_048,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "verification/evidence/pbt-counterexamples.regressions",
        ))),
        .. ProptestConfig::default()
    })]

    /// Requirement-derived happy-path model.  It does not reuse `App`'s
    /// transition functions to calculate the expected state.
    #[test]
    fn pbt_successful_user_intents_converge_to_the_independent_settings_model(
        intent_codes in prop::collection::vec(0_u8..8, 0..48),
    ) {
        let initial = default_state();
        let mut expected = initial;
        let mut port = RegistryProtocolAdapter::new(initial);
        let mut app = start_nonlive_app(&mut port);

        for code in intent_codes {
            match code {
                0..=2 => {
                    let theme = ThemePreference::ALL[usize::from(code)];
                    expected.settings.theme = theme;
                    dispatch_and_execute(&mut app, &mut port, Event::SelectTheme(theme));
                }
                3..=6 => {
                    let limit = FpsLimit::ALL[usize::from(code - 3)];
                    expected.settings.fps_limit = limit;
                    dispatch_and_execute(&mut app, &mut port, Event::SelectFpsLimit(limit));
                }
                7 => {
                    expected.settings.launch_at_startup = !expected.settings.launch_at_startup;
                    expected.run_value_present = expected.settings.launch_at_startup;
                    dispatch_and_execute(&mut app, &mut port, Event::ToggleStartup);
                }
                _ => unreachable!("the generator produces only 0..8"),
            }
            prop_assert_eq!(app.snapshot().settings, expected.settings);
            prop_assert_eq!(port.visible(), expected);
            prop_assert_eq!(app.snapshot().pending_startup_change, None);
        }
    }

    /// This is deliberately ignored in the normal green suite: it is a
    /// requirement conformance probe expected to fail against the current
    /// production protocol.  Running it saves its RNG seed and minimized
    /// counterexample in `verification/evidence/pbt-counterexamples.regressions`.
    #[test]
    #[ignore = "expected atomicity counterexample; run via scripts/run-verification.ps1"]
    fn pbt_faulted_settings_write_must_not_expose_a_partial_generation(
        initial_theme in 0_usize..3,
        initial_fps in 0_usize..4,
        initial_startup in any::<bool>(),
        fault_index in 0_usize..3,
    ) {
        let initial_settings = settings(
            ThemePreference::ALL[initial_theme],
            FpsLimit::ALL[initial_fps],
            initial_startup,
        );
        let initial = DurableState::new(initial_settings, initial_startup);
        let candidate = DurableState::new(
            settings(
                ThemePreference::ALL[(initial_theme + 1) % 3],
                FpsLimit::ALL[(initial_fps + 1) % 4],
                !initial_startup,
            ),
            initial_startup,
        );
        let faults = [
            FaultPoint::WriteTheme,
            FaultPoint::WriteFpsLimit,
            FaultPoint::WriteStartupFlag,
        ];
        let mut port = RegistryProtocolAdapter::new(initial).failing_at(faults[fault_index]);
        let oracle = AtomicCommitOracle::new(initial);

        port.apply(&Effect::SaveSettings(candidate.settings));

        // A failed atomic commit must leave the previous complete generation
        // observable.  The actual protocol instead leaves a shrunk hybrid.
        prop_assert_eq!(port.visible(), oracle.state());
    }
}
