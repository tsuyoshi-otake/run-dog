use std::collections::VecDeque;

use crate::core::{
    AppSettings, GpuStatus, MemoryStatus, ResolvedTheme, StorageStatus, SystemTimes,
};

use super::{commit_protocol::CommitRequest, App, Effect, Event};

/// Read-only CPU port. The production adapter reads `GetSystemTimes`; tests
/// provide a finite in-memory sample queue.
pub trait CpuSource {
    fn read_system_times(&mut self) -> Option<SystemTimes>;
    fn read_memory(&mut self) -> Option<MemoryStatus> {
        None
    }
    fn read_storage(&mut self) -> Option<StorageStatus> {
        None
    }
    fn read_gpu(&mut self) -> Option<GpuStatus> {
        None
    }
}

/// Settings port intentionally exposes only the small data structure needed by
/// the state machine, instead of leaking registry details into application code.
pub trait SettingsStore {
    fn load(&mut self) -> AppSettings;
    fn load_generation(&mut self) -> u64;
    fn load_last_operation_id(&mut self) -> u64;
}

/// Supplies the operating-system theme in a testable form.
pub trait ThemeSource {
    fn system_theme(&mut self) -> ResolvedTheme;
}

/// A clock abstraction retained for deterministic integration test rigs. The
/// application itself is timer-message driven and therefore never polls it.
pub trait Clock {
    fn now_millis(&self) -> u64;
}

/// Executes an effect at the platform boundary.
pub trait EffectPort {
    fn apply(&mut self, effect: &Effect);
    fn execute_commit(&mut self, request: CommitRequest) -> super::CommitOutcome;
    fn cancel_commit(&mut self, operation_id: u64);
    fn now_millis(&self) -> u64;
}

/// Dispatches a CPU tick only when the source supplied a snapshot.
pub fn dispatch_cpu_tick<P: EffectPort, S: CpuSource>(app: &mut App, source: &mut S, port: &mut P) {
    if let Some(times) = source.read_system_times() {
        let memory = source.read_memory();
        dispatch_and_execute(
            app,
            port,
            Event::CpuSample {
                times,
                memory,
                storage: source.read_storage(),
                gpu: source.read_gpu(),
                process: None,
            },
        );
    }
}

/// Runs an event and its internally generated commit-result event(s).
pub fn dispatch_and_execute<P: EffectPort>(app: &mut App, port: &mut P, event: Event) {
    let mut pending_events = VecDeque::from([event]);
    while let Some(event) = pending_events.pop_front() {
        for effect in app.dispatch(event) {
            match effect {
                Effect::CommitSettings {
                    operation_id,
                    settings,
                    previous,
                    expected_generation,
                    sync_run_entry,
                    deadline_millis: _,
                } => {
                    port.apply(&effect);
                    let outcome = port.execute_commit(CommitRequest {
                        operation_id,
                        expected_generation,
                        settings,
                        previous,
                        sync_run_entry,
                        deadline_millis: port
                            .now_millis()
                            .saturating_add(super::effect::COMMIT_DEADLINE_MS),
                    });
                    pending_events.push_back(Event::SettingsCommitFinished {
                        settings,
                        status: outcome.status,
                        new_generation: outcome.generation,
                        last_operation_id: outcome.last_operation_id,
                    });
                }
                Effect::CancelCommit { operation_id } => {
                    port.cancel_commit(operation_id);
                    port.apply(&effect);
                }
                other => port.apply(&other),
            }
        }
    }
}
