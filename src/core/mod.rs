//! Pure domain logic used by the application state machine.

mod animation;
mod cpu;
mod memory;
mod settings;
mod sparkline;
mod storage;
mod theme;
mod usage;

pub use animation::{AnimationController, AnimationRateChange, FpsLimit, FrameCursor};
pub use cpu::{breakdown_between, usage_between, CpuBreakdown, CpuLoad, CpuSampler, SystemTimes};
pub use memory::MemoryStatus;
pub use settings::{AppSettings, PendingJournal, SettingsRecord};
pub use sparkline::{Sparkline, SPARKLINE_CAPACITY};
pub use storage::StorageStatus;
pub use theme::{ResolvedTheme, ThemePreference};
pub use usage::{
    cost_cents, days_to_ymd, format_plan_label, local_hms, local_ymd, ymd_key, LimitWindow,
    ProviderUsage, TokenUsage, UsageSnapshot,
};
