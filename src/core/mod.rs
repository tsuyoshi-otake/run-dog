//! Pure domain logic used by the application state machine.

mod animation;
mod cpu;
mod settings;
mod theme;

pub use animation::{AnimationController, AnimationRateChange, FpsLimit, FrameCursor};
pub use cpu::{usage_between, CpuLoad, CpuSampler, SystemTimes};
pub use settings::AppSettings;
pub use theme::{ResolvedTheme, ThemePreference};
