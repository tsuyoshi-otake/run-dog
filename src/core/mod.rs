//! Pure domain logic used by the application state machine.

mod animation;
mod claude_dedupe;
mod codex_usage;
mod cpu;
mod gpu;
mod memory;
mod settings;
mod sparkline;
mod storage;
mod theme;
mod usage;
mod usage_checkpoint;
mod usage_diagnostics;
mod usage_durable;
mod usage_fetch;
#[cfg(test)]
mod usage_ingest_fuzz;
#[cfg(test)]
mod usage_ingest_pbt;
mod usage_perf;
mod usage_state;

pub use animation::{AnimationController, AnimationRateChange, FpsLimit, FrameCursor};
pub use claude_dedupe::{claude_dedupe_digest, hex_decode, retain_keys_for_month, ClaudeDedupeKey};
pub use codex_usage::{decide_codex_event, CodexTokenTotals, CodexUsageDecision};
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
    cost_cents, days_to_ymd, format_banked_reset_label, format_compact_token_count,
    format_fable_limit_label, format_plan_label, is_long_context_request, local_hms, local_ymd,
    resolve_codex_model, ymd_iso, ymd_key, LimitWindow, ProviderUsage, TokenUsage, UsageSnapshot,
};
pub use usage_checkpoint::{
    FileCheckpointCursor, FileCheckpointKey, UsageCheckpoint, USAGE_CHECKPOINT_MIGRATION_VERSION,
};
pub use usage_diagnostics::{
    fetch_result_code, persist_result_code, persist_result_detail, rebuild_reason_code,
    CheckpointSource, DiagnosticEvent, DiagnosticKind, DiagnosticRing, DiagnosticSnapshot,
    RebuildState, RescanReason, RestoreResult, StartupMode, DIAGNOSTIC_RING_CAP,
};
pub use usage_durable::{
    load_usage_state, persist_usage_state, GenerationBlobs, LoadStatus, MemoryBlobs, PersistStatus,
    MAX_PRIOR_GENERATIONS,
};
pub use usage_fetch::{
    cancel_fetch, decide_apply, finish_fetch, freshness_after_failure, next_backoff_ms,
    record_spawn_failure, reject_late_result, should_start_fetch, start_fetch, FetchApplyDecision,
    FetchErrorKind, FetchOutcome, LimitsFreshness, ProviderFetchKind, ProviderFetchState,
    FETCH_BACKOFF_INITIAL_MS, FETCH_BACKOFF_MAX_MS,
};
#[cfg(test)]
pub use usage_ingest_fuzz::{
    contains_forbidden_secret, fuzz_case_count, last_safe_complete_record_offset, mutate,
    seed_corpus, XorShift, DEFAULT_FUZZ_CASES, FUZZ_SEED, MAX_FUZZ_INPUT,
};
pub use usage_perf::{
    warm_restart_allows_auxiliary_io, warm_restart_does_not_reaggregate, PerfScenario,
    ScenarioRunStatus, UsageWorkCounters,
};
pub use usage_state::{
    usage_store_root_is_forbidden, CursorKind, CursorRebuildReason, UsageAggregate, UsageCursor,
    UsageState, USAGE_STATE_HEADER, USAGE_STATE_SCHEMA_VERSION,
};
