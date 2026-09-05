//! Per-provider remote limit fetch state.
//!
//! Claude and Codex each own single-flight, generation, backoff, and
//! freshness. A failed provider must not zero the other, and must not be
//! presented as 0%. Late results after cancel or a newer generation are
//! rejected. `Drop` is not a safety argument; generation/cancel is.

/// Which remote limit endpoint a slot talks to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderFetchKind {
    Claude,
    Codex,
}

/// Bounded failure class. No host, path, token, or body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum FetchErrorKind {
    SpawnFailed = 1,
    Cancelled = 2,
    GenerationMismatch = 3,
    AuthMissing = 4,
    Transport = 5,
    HttpStatus = 6,
    Parse = 7,
}

impl FetchErrorKind {
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self as u32
    }
}

/// How a stored window may be shown. Failed/unknown are never 0%.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u32)]
pub enum LimitsFreshness {
    #[default]
    Unknown = 0,
    Current = 1,
    Stale = 2,
    Expired = 3,
    Failed = 4,
}

impl LimitsFreshness {
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self as u32
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProviderFetchState {
    pub last_attempt_ms: u64,
    pub last_success_ms: u64,
    pub last_error: Option<FetchErrorKind>,
    pub observed_at_ms: u64,
    pub reset_at_ms: u64,
    pub freshness: LimitsFreshness,
    pub backoff_ms: u64,
    pub request_generation: u64,
    pub in_flight: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FetchOutcome {
    pub generation: u64,
    pub succeeded: bool,
    pub error: Option<FetchErrorKind>,
    pub observed_at_ms: u64,
    pub reset_at_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FetchApplyDecision {
    ApplySuccess,
    RecordFailure,
    RejectLate,
}

pub const FETCH_BACKOFF_INITIAL_MS: u64 = 30 * 1_000;
pub const FETCH_BACKOFF_MAX_MS: u64 = 15 * 60 * 1_000;

#[must_use]
pub fn next_backoff_ms(previous: u64) -> u64 {
    if previous == 0 {
        FETCH_BACKOFF_INITIAL_MS
    } else {
        previous.saturating_mul(2).min(FETCH_BACKOFF_MAX_MS)
    }
}

#[must_use]
pub fn should_start_fetch(state: &ProviderFetchState, now_ms: u64, period_ms: u64) -> bool {
    if state.in_flight {
        return false;
    }
    if state.last_attempt_ms == 0 {
        return true;
    }
    let wait = if state.last_error.is_some() {
        state.backoff_ms
    } else {
        period_ms
    };
    now_ms.saturating_sub(state.last_attempt_ms) >= wait
}

/// Marks a slot in-flight and returns the generation the worker must echo.
pub fn start_fetch(state: &mut ProviderFetchState, now_ms: u64) -> u64 {
    state.request_generation = state.request_generation.saturating_add(1);
    state.in_flight = true;
    state.last_attempt_ms = now_ms;
    state.request_generation
}

#[must_use]
pub fn reject_late_result(
    cancelled: bool,
    started_generation: u64,
    result_generation: u64,
) -> bool {
    cancelled || result_generation == 0 || result_generation != started_generation
}

#[must_use]
pub fn decide_apply(
    state: &ProviderFetchState,
    cancelled: bool,
    outcome: &FetchOutcome,
) -> FetchApplyDecision {
    if reject_late_result(cancelled, state.request_generation, outcome.generation) {
        return FetchApplyDecision::RejectLate;
    }
    if outcome.succeeded {
        FetchApplyDecision::ApplySuccess
    } else {
        FetchApplyDecision::RecordFailure
    }
}

/// Records a finished attempt. Success never writes 0% — the caller applies
/// the payload separately. Failure leaves the last good payload untouched.
pub fn finish_fetch(state: &mut ProviderFetchState, now_ms: u64, outcome: &FetchOutcome) {
    state.in_flight = false;
    match decide_apply(state, false, outcome) {
        FetchApplyDecision::ApplySuccess => {
            state.last_success_ms = now_ms;
            state.last_error = None;
            state.observed_at_ms = outcome.observed_at_ms;
            state.reset_at_ms = outcome.reset_at_ms;
            state.freshness = LimitsFreshness::Current;
            state.backoff_ms = 0;
        }
        FetchApplyDecision::RecordFailure => {
            state.last_error = outcome.error.or(Some(FetchErrorKind::Transport));
            state.freshness = freshness_after_failure(state, now_ms);
            state.backoff_ms = next_backoff_ms(state.backoff_ms);
        }
        FetchApplyDecision::RejectLate => {
            state.last_error = Some(FetchErrorKind::GenerationMismatch);
        }
    }
}

#[must_use]
pub fn freshness_after_failure(state: &ProviderFetchState, now_ms: u64) -> LimitsFreshness {
    if state.last_success_ms == 0 {
        LimitsFreshness::Failed
    } else if state.reset_at_ms != 0 && state.reset_at_ms <= now_ms {
        LimitsFreshness::Expired
    } else {
        LimitsFreshness::Stale
    }
}

/// Shutdown path: bump generation so an in-flight worker cannot apply.
/// This is the cancel contract. Joining or dropping the thread is not required.
pub fn cancel_fetch(state: &mut ProviderFetchState) {
    state.request_generation = state.request_generation.saturating_add(1);
    state.in_flight = false;
    state.last_error = Some(FetchErrorKind::Cancelled);
    state.freshness = freshness_after_failure(state, state.last_attempt_ms);
}

/// Spawn failed after the in-flight bit was taken. Clear the bit so the
/// provider can retry; do not leave the slot stuck.
pub fn record_spawn_failure(state: &mut ProviderFetchState, now_ms: u64) {
    state.in_flight = false;
    state.last_error = Some(FetchErrorKind::SpawnFailed);
    state.freshness = freshness_after_failure(state, now_ms);
    state.backoff_ms = next_backoff_ms(state.backoff_ms);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> ProviderFetchState {
        ProviderFetchState::default()
    }

    #[test]
    fn component_providers_schedule_independently() {
        let mut claude = empty();
        let codex = empty();
        assert!(should_start_fetch(&claude, 1, 300_000));
        let gen = start_fetch(&mut claude, 1);
        assert_eq!(gen, 1);
        assert!(!should_start_fetch(&claude, 2, 300_000));
        assert!(should_start_fetch(&codex, 2, 60_000));
    }

    #[test]
    fn component_failure_does_not_clear_success_watermark() {
        let mut state = empty();
        let gen = start_fetch(&mut state, 10);
        finish_fetch(
            &mut state,
            11,
            &FetchOutcome {
                generation: gen,
                succeeded: true,
                error: None,
                observed_at_ms: 11,
                reset_at_ms: 5_000,
            },
        );
        assert_eq!(state.last_success_ms, 11);
        assert_eq!(state.freshness, LimitsFreshness::Current);
        let gen = start_fetch(&mut state, 20);
        finish_fetch(
            &mut state,
            21,
            &FetchOutcome {
                generation: gen,
                succeeded: false,
                error: Some(FetchErrorKind::HttpStatus),
                observed_at_ms: 21,
                reset_at_ms: 0,
            },
        );
        assert_eq!(state.last_success_ms, 11);
        assert_eq!(state.last_error, Some(FetchErrorKind::HttpStatus));
        assert_eq!(state.freshness, LimitsFreshness::Stale);
        assert_eq!(state.backoff_ms, FETCH_BACKOFF_INITIAL_MS);
    }

    #[test]
    fn component_first_failure_is_failed_not_zero() {
        let mut state = empty();
        let gen = start_fetch(&mut state, 1);
        finish_fetch(
            &mut state,
            2,
            &FetchOutcome {
                generation: gen,
                succeeded: false,
                error: Some(FetchErrorKind::AuthMissing),
                observed_at_ms: 2,
                reset_at_ms: 0,
            },
        );
        assert_eq!(state.last_success_ms, 0);
        assert_eq!(state.freshness, LimitsFreshness::Failed);
        assert_ne!(state.freshness.as_u32(), LimitsFreshness::Current.as_u32());
    }

    #[test]
    fn component_late_generation_is_rejected() {
        let mut state = empty();
        let gen = start_fetch(&mut state, 1);
        assert!(reject_late_result(false, gen, gen.saturating_sub(1)));
        assert!(reject_late_result(true, gen, gen));
        assert!(!reject_late_result(false, gen, gen));
        let decision = decide_apply(
            &state,
            true,
            &FetchOutcome {
                generation: gen,
                succeeded: true,
                error: None,
                observed_at_ms: 2,
                reset_at_ms: 9,
            },
        );
        assert_eq!(decision, FetchApplyDecision::RejectLate);
    }

    #[test]
    fn component_cancel_bumps_generation_without_drop() {
        let mut state = empty();
        let gen = start_fetch(&mut state, 1);
        cancel_fetch(&mut state);
        assert_ne!(state.request_generation, gen);
        assert!(!state.in_flight);
        assert!(reject_late_result(true, state.request_generation, gen));
    }

    #[test]
    fn component_spawn_failure_clears_inflight_for_retry() {
        let mut state = empty();
        let _ = start_fetch(&mut state, 1);
        record_spawn_failure(&mut state, 2);
        assert!(!state.in_flight);
        assert_eq!(state.last_error, Some(FetchErrorKind::SpawnFailed));
        assert!(should_start_fetch(
            &state,
            2 + FETCH_BACKOFF_INITIAL_MS,
            60_000
        ));
    }

    #[test]
    fn component_backoff_doubles_and_caps() {
        let mut wait = 0;
        wait = next_backoff_ms(wait);
        assert_eq!(wait, FETCH_BACKOFF_INITIAL_MS);
        wait = next_backoff_ms(wait);
        assert_eq!(wait, FETCH_BACKOFF_INITIAL_MS * 2);
        for _ in 0..8 {
            wait = next_backoff_ms(wait);
        }
        assert_eq!(wait, FETCH_BACKOFF_MAX_MS);
    }
}
