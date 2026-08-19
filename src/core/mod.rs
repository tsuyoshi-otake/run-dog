//! Pure domain logic used by the application state machine.

mod animation;
mod cpu;
mod gpu;
mod memory;
mod settings;
mod sparkline;
mod storage;
mod theme;
mod usage;

pub use animation::{AnimationController, AnimationRateChange, FpsLimit, FrameCursor};
pub use cpu::{
    breakdown_between, process_share, usage_between, CpuBreakdown, CpuLoad, CpuSampler,
    ProcessStatus, ProcessTimes, SystemTimes,
};
pub use gpu::GpuStatus;
pub use memory::MemoryStatus;
pub use settings::{AppSettings, PendingJournal, SettingsRecord};
pub use sparkline::{Sparkline, SPARKLINE_CAPACITY};
pub use storage::StorageStatus;
pub use theme::{ResolvedTheme, ThemePreference};
pub use usage::{
    cost_cents, days_to_ymd, format_plan_label, is_long_context_request, local_hms, local_ymd,
    resolve_codex_model, ymd_iso, ymd_key, LimitWindow, ProviderUsage, TokenUsage, UsageSnapshot,
};
