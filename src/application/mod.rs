//! State transitions and ports. This module has no Win32 imports.

mod app;
mod effect;
mod event;
mod ports;

pub use app::{App, AppSnapshot, ANIMATION_FRAME_COUNT, CPU_SAMPLE_INTERVAL_MS};
pub use effect::{Effect, TimerKind, TrayIcon};
pub use event::Event;
pub use ports::{
    dispatch_and_execute, dispatch_cpu_tick, Clock, CpuSource, EffectPort, SettingsStore,
    ThemeSource,
};
