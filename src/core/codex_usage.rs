//! Codex `token_count` state: count advancing usage, ignore snapshot replay.

use super::TokenUsage;

/// Cumulative or last-turn token fields from a Codex `token_count` event.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CodexTokenTotals {
    pub input: u64,
    pub cached: u64,
    pub output: u64,
}

impl CodexTokenTotals {
    #[must_use]
    pub fn to_usage(self) -> TokenUsage {
        let cached = self.cached.min(self.input);
        TokenUsage {
            input: self.input - cached,
            cached_input: cached,
            output: self.output,
            ..TokenUsage::default()
        }
    }

    fn dominates(self, last: Self) -> bool {
        self.input >= last.input
            && self.cached >= last.cached
            && self.output >= last.output
            && self != last
    }

    fn reset_after(self, previous: Self) -> bool {
        self.input < previous.input || self.output < previous.output
    }
}

/// What to do with one Codex `token_count` after comparing session totals.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CodexUsageDecision {
    Count(TokenUsage),
    IgnoreReplay,
    IgnoreInheritedBaseline,
}

/// Decide whether `last` is new spend.
///
/// `total` is session-cumulative. When it is missing, identity is incomplete and
/// `last` is counted without claiming replay protection.
#[must_use]
pub fn decide_codex_event(
    previous_total: Option<CodexTokenTotals>,
    last: CodexTokenTotals,
    total: Option<CodexTokenTotals>,
) -> (CodexUsageDecision, Option<CodexTokenTotals>) {
    let Some(total) = total else {
        return (CodexUsageDecision::Count(last.to_usage()), previous_total);
    };
    match previous_total {
        None if total == last => (CodexUsageDecision::Count(last.to_usage()), Some(total)),
        None if total.dominates(last) => (CodexUsageDecision::IgnoreInheritedBaseline, Some(total)),
        None => (CodexUsageDecision::Count(last.to_usage()), Some(total)),
        Some(previous) if total == previous => (CodexUsageDecision::IgnoreReplay, Some(previous)),
        Some(previous) if total.reset_after(previous) => {
            (CodexUsageDecision::Count(last.to_usage()), Some(total))
        }
        Some(_) => (CodexUsageDecision::Count(last.to_usage()), Some(total)),
    }
}

#[cfg(test)]
mod tests {
    use super::{decide_codex_event, CodexTokenTotals, CodexUsageDecision};

    fn totals(input: u64, cached: u64, output: u64) -> CodexTokenTotals {
        CodexTokenTotals {
            input,
            cached,
            output,
        }
    }

    #[test]
    fn component_identical_snapshot_is_replay() {
        let last = totals(80, 20, 5);
        let (first, prev) = decide_codex_event(None, last, Some(last));
        assert!(matches!(first, CodexUsageDecision::Count(_)));
        let (again, _) = decide_codex_event(prev, last, Some(last));
        assert_eq!(again, CodexUsageDecision::IgnoreReplay);
    }

    #[test]
    fn component_cumulative_second_turn_counts_last_only() {
        let turn1 = totals(80, 20, 5);
        let turn2_last = totals(40, 10, 2);
        let turn2_total = totals(120, 30, 7);
        let (_, prev) = decide_codex_event(None, turn1, Some(turn1));
        let (second, _) = decide_codex_event(prev, turn2_last, Some(turn2_total));
        assert_eq!(second, CodexUsageDecision::Count(turn2_last.to_usage()));
    }

    #[test]
    fn component_fork_inherited_snapshot_is_baseline() {
        let inherited_last = totals(80, 20, 5);
        let inherited_total = totals(160, 40, 10);
        let (first, prev) = decide_codex_event(None, inherited_last, Some(inherited_total));
        assert_eq!(first, CodexUsageDecision::IgnoreInheritedBaseline);
        let child_last = totals(40, 0, 3);
        let child_total = totals(200, 40, 13);
        let (child, _) = decide_codex_event(prev, child_last, Some(child_total));
        assert_eq!(child, CodexUsageDecision::Count(child_last.to_usage()));
    }

    #[test]
    fn component_same_tuple_with_advancing_total_counts_twice() {
        let last = totals(80, 20, 5);
        let (_, prev) = decide_codex_event(None, last, Some(last));
        let (second, _) = decide_codex_event(prev, last, Some(totals(160, 40, 10)));
        assert_eq!(second, CodexUsageDecision::Count(last.to_usage()));
    }

    #[test]
    fn component_counter_reset_counts_new_epoch() {
        let (_, prev) = decide_codex_event(None, totals(80, 0, 5), Some(totals(80, 0, 5)));
        let reset = totals(10, 0, 1);
        let (decision, _) = decide_codex_event(prev, reset, Some(reset));
        assert_eq!(decision, CodexUsageDecision::Count(reset.to_usage()));
    }

    #[test]
    fn component_missing_total_counts_last_without_replay_claim() {
        let last = totals(80, 20, 5);
        let (first, prev) = decide_codex_event(None, last, None);
        assert_eq!(first, CodexUsageDecision::Count(last.to_usage()));
        let (again, _) = decide_codex_event(prev, last, None);
        assert_eq!(again, CodexUsageDecision::Count(last.to_usage()));
    }

    fn fold_counted_input(
        events: &[(CodexTokenTotals, CodexTokenTotals)],
        start: Option<CodexTokenTotals>,
    ) -> (u64, Option<CodexTokenTotals>) {
        let mut previous = start;
        let mut input = 0_u64;
        for &(last, total) in events {
            let (decision, next) = decide_codex_event(previous, last, Some(total));
            previous = next;
            if let CodexUsageDecision::Count(usage) = decision {
                input = input.saturating_add(usage.processed_input_tokens());
            }
        }
        (input, previous)
    }

    proptest::proptest! {
        #[test]
        fn pbt_unchanged_total_is_never_counted_twice(
            input in 1u64..10_000,
            cached in 0u64..10_000,
            output in 0u64..10_000,
        ) {
            let last = totals(input, cached.min(input), output);
            let (first, prev) = decide_codex_event(None, last, Some(last));
            proptest::prop_assert!(matches!(first, CodexUsageDecision::Count(_)));
            let (again, _) = decide_codex_event(prev, last, Some(last));
            proptest::prop_assert_eq!(again, CodexUsageDecision::IgnoreReplay);
        }

        #[test]
        fn pbt_replay_invariance_after_advancing_session(
            first_in in 1u64..500,
            second_in in 1u64..500,
            cached in 0u64..20,
            output in 0u64..50,
        ) {
            let turn1 = totals(first_in, cached.min(first_in), output);
            let turn2_last = totals(second_in, cached.min(second_in), output);
            let turn2_total = totals(
                first_in.saturating_add(second_in),
                cached.min(first_in).saturating_add(cached.min(second_in)),
                output.saturating_add(output),
            );
            let events = [(turn1, turn1), (turn2_last, turn2_total)];
            let (once, prev) = fold_counted_input(&events, None);
            let (replay, _) = decide_codex_event(prev, turn2_last, Some(turn2_total));
            proptest::prop_assert_eq!(replay, CodexUsageDecision::IgnoreReplay);
            let (again, _) = fold_counted_input(&[(turn2_last, turn2_total)], prev);
            proptest::prop_assert_eq!(again, 0);
            proptest::prop_assert_eq!(once, first_in.saturating_add(second_in));
        }

        #[test]
        fn pbt_restart_equivalence_split_after_first_turn(
            first_in in 1u64..500,
            second_in in 1u64..500,
            cached in 0u64..20,
            output in 0u64..50,
        ) {
            let turn1 = totals(first_in, cached.min(first_in), output);
            let turn2_last = totals(second_in, cached.min(second_in), output);
            let turn2_total = totals(
                first_in.saturating_add(second_in),
                cached.min(first_in).saturating_add(cached.min(second_in)),
                output.saturating_add(output),
            );
            let events = [(turn1, turn1), (turn2_last, turn2_total)];
            let (continuous, _) = fold_counted_input(&events, None);
            let (prefix, watermark) = fold_counted_input(&events[..1], None);
            let (suffix, _) = fold_counted_input(&events[1..], watermark);
            proptest::prop_assert_eq!(continuous, prefix.saturating_add(suffix));
        }
    }
}
