use std::collections::VecDeque;

use crate::core::{AppSettings, ResolvedTheme, SystemTimes};

use super::{App, Effect, Event};

/// Read-only CPU port. The production adapter reads `GetSystemTimes`; tests
/// provide a finite in-memory sample queue.
pub trait CpuSource {
    fn read_system_times(&mut self) -> Option<SystemTimes>;
}

/// Settings port intentionally exposes only the small data structure needed by
/// the state machine, instead of leaking registry details into application code.
pub trait SettingsStore {
    fn load(&mut self) -> AppSettings;
    fn save(&mut self, settings: AppSettings);
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
    fn set_startup(&mut self, enabled: bool) -> bool;
}

/// Dispatches a CPU tick only when the source supplied a snapshot.
pub fn dispatch_cpu_tick<P: EffectPort, S: CpuSource>(app: &mut App, source: &mut S, port: &mut P) {
    if let Some(sample) = source.read_system_times() {
        dispatch_and_execute(app, port, Event::CpuSample(sample));
    }
}

/// Runs an event and its internally generated startup-result event(s).
///
/// This is deliberately small: it makes success and failure of registry work
/// explicit to integration tests, and avoids a live platform dependency.
pub fn dispatch_and_execute<P: EffectPort>(app: &mut App, port: &mut P, event: Event) {
    let mut pending_events = VecDeque::from([event]);
    while let Some(event) = pending_events.pop_front() {
        for effect in app.dispatch(event) {
            if let Effect::RequestStartup(enabled) = effect {
                port.apply(&effect);
                let succeeded = port.set_startup(enabled);
                pending_events.push_back(Event::StartupChangeFinished { enabled, succeeded });
            } else {
                port.apply(&effect);
            }
        }
    }
}
