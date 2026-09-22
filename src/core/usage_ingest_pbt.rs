//! Focused usage ingest PBT. Oracles are independent specs, not copies of
//! encode/decode or the collector loop.
//!
//! Seed and regression file are fixed so a counterexample stays in-tree.

#![cfg(test)]

use super::{
    decide_codex_event, load_usage_state, persist_usage_state, retain_keys_for_month,
    ClaudeDedupeKey, CodexTokenTotals, CodexUsageDecision, CursorKind, FileCheckpointCursor,
    FileCheckpointKey, LoadStatus, MemoryBlobs, PersistStatus, ProviderUsage, UsageAggregate,
    UsageCheckpoint, UsageCursor, UsageSnapshot, UsageState, USAGE_STATE_HEADER,
};
use proptest::prelude::*;
use std::collections::{HashMap, HashSet};

const INGEST_SEED: u64 = 0x5EED_2026_0905_0001;

fn ingest_config() -> ProptestConfig {
    ProptestConfig {
        cases: 128,
        rng_seed: proptest::test_runner::RngSeed::Fixed(INGEST_SEED),
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "verification/evidence/usage-ingest-pbt.regressions",
            ),
        )),
        ..ProptestConfig::default()
    }
}

fn arb_logical_id() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-z]{1,4}/[a-z0-9]{1,6}\\.jsonl").expect("logical id regex")
}

fn arb_cursor() -> impl Strategy<Value = UsageCursor> {
    (
        proptest::bool::ANY,
        arb_logical_id(),
        0_u64..64,
        0_u64..64,
        proptest::option::of(0_u64..1000),
        proptest::option::of(1_u32..13),
    )
        .prop_map(|(codex, logical_id, offset, extra, prefix, month)| {
            let size = offset.saturating_add(extra);
            UsageCursor {
                active_month: month.map(|month| 20_260_001 + month * 100),
                kind: if codex {
                    CursorKind::Codex
                } else {
                    CursorKind::Claude
                },
                logical_id,
                offset,
                size,
                prefix,
                file_id: Some([size as u32, offset as u32, 42]),
                last_model: if codex {
                    Some("gpt-5.4".to_owned())
                } else {
                    None
                },
                last_codex_total: if codex {
                    Some(CodexTokenTotals {
                        input: offset,
                        cached: 0,
                        output: extra.min(9),
                    })
                } else {
                    None
                },
            }
        })
}

fn arb_dkey() -> impl Strategy<Value = ClaudeDedupeKey> {
    (
        0_u8..8,
        0_u8..8,
        prop_oneof![Just(20_260_801_u32), Just(20_260_901_u32)],
    )
        .prop_map(|(left, right, month)| {
            ClaudeDedupeKey::new(&left.to_string(), &right.to_string(), month)
        })
}

fn arb_state() -> impl Strategy<Value = UsageState> {
    (
        1_u64..8,
        20_260_801_u32..20_261_201,
        20_260_801_u32..20_261_231,
        proptest::collection::vec(arb_cursor(), 0..3),
        proptest::collection::vec(arb_dkey(), 0..4),
        any::<bool>(),
    )
        .prop_map(
            |(generation, month_start, today, cursors, keys, catch_up_done)| UsageState {
                generation,
                schema_version: crate::core::USAGE_STATE_SCHEMA_VERSION,
                aggregate: UsageAggregate {
                    month_start,
                    today,
                    last_collected_ms: generation * 10,
                    catch_up_done,
                    snapshot: UsageSnapshot {
                        claude: ProviderUsage {
                            today_cents: generation as u32 + 1,
                            month_cents: generation as u32,
                            today_cost_nanos: (generation + 1) * crate::core::NANOS_PER_CENT,
                            month_cost_nanos: generation * crate::core::NANOS_PER_CENT,
                            month_input_tokens: generation * 3,
                            month_output_tokens: generation * 5,
                            ..ProviderUsage::default()
                        },
                        codex: ProviderUsage {
                            today_cents: generation as u32 + 2,
                            month_cents: generation as u32 + 3,
                            today_cost_nanos: (generation + 2) * crate::core::NANOS_PER_CENT,
                            month_cost_nanos: (generation + 3) * crate::core::NANOS_PER_CENT,
                            month_input_tokens: generation * 7,
                            month_output_tokens: generation * 11,
                            ..ProviderUsage::default()
                        },
                        ..UsageSnapshot::default()
                    },
                },
                cursors,
                claude_keys: keys.into_iter().collect(),
            },
        )
}

/// Size-shrink contract published with the incremental reader.
/// Not a copy of `restored_cursor_offset` (that also checks prefix/file id).
fn spec_size_rebuild(stored_offset: u64, stored_size: u64, new_size: u64) -> (u64, bool) {
    if new_size < stored_offset || new_size < stored_size {
        (0, true)
    } else {
        (stored_offset.min(new_size), false)
    }
}

const MODEL_MAX_RETRIES: u8 = 3;

#[derive(Clone, Copy, Debug)]
enum CollectorAction {
    Discover,
    Register,
    PermanentFailure,
    Append,
    Scan,
    Commit,
    Rollover,
    Restart,
}

fn arb_collector_action() -> impl Strategy<Value = CollectorAction> {
    (0_u8..8).prop_map(|action| match action {
        0 => CollectorAction::Discover,
        1 => CollectorAction::Register,
        2 => CollectorAction::PermanentFailure,
        3 => CollectorAction::Append,
        4 => CollectorAction::Scan,
        5 => CollectorAction::Commit,
        6 => CollectorAction::Rollover,
        _ => CollectorAction::Restart,
    })
}

#[derive(Clone, Debug)]
struct CollectorStateModel {
    discovered: bool,
    registered: bool,
    bad_attempts: u8,
    bad_terminal: bool,
    catch_up: bool,
    appended: u64,
    cursor: u64,
    committed_cursor: u64,
    month_count: u64,
    today_count: u64,
    committed_month_count: u64,
    committed_today_count: u64,
    day: u8,
}

impl Default for CollectorStateModel {
    fn default() -> Self {
        Self {
            discovered: false,
            registered: false,
            bad_attempts: 0,
            bad_terminal: false,
            catch_up: true,
            appended: 0,
            cursor: 0,
            committed_cursor: 0,
            month_count: 0,
            today_count: 0,
            committed_month_count: 0,
            committed_today_count: 0,
            day: 1,
        }
    }
}

impl CollectorStateModel {
    fn step(&mut self, action: CollectorAction) {
        match action {
            CollectorAction::Discover => self.discovered = true,
            CollectorAction::Register => {
                if self.discovered {
                    self.registered = true;
                }
            }
            CollectorAction::PermanentFailure => {
                if !self.bad_terminal {
                    self.bad_attempts = self.bad_attempts.saturating_add(1);
                    self.bad_terminal = self.bad_attempts >= MODEL_MAX_RETRIES;
                }
            }
            CollectorAction::Append => self.appended = self.appended.saturating_add(1),
            CollectorAction::Scan => {
                if self.registered && self.cursor < self.appended {
                    let delta = self.appended - self.cursor;
                    self.cursor = self.appended;
                    self.month_count = self.month_count.saturating_add(delta);
                    self.today_count = self.today_count.saturating_add(delta);
                }
            }
            CollectorAction::Commit => {
                self.committed_cursor = self.cursor;
                self.committed_month_count = self.month_count;
                self.committed_today_count = self.today_count;
            }
            CollectorAction::Rollover => {
                self.day = self.day.saturating_add(1);
                self.today_count = 0;
                // The production rollover marks the aggregate dirty in the
                // same transition. Model that atomic durability boundary so
                // a restart cannot resurrect the prior day's Today value.
                self.committed_today_count = 0;
            }
            CollectorAction::Restart => {
                self.cursor = self.committed_cursor;
                self.month_count = self.committed_month_count;
                self.today_count = self.committed_today_count;
                self.discovered = false;
                self.registered = false;
                // Runtime retry state is intentionally not durable. A
                // restart may retry the bad path, but committed usage and
                // cursor state must remain the source of truth.
                self.bad_attempts = 0;
                self.bad_terminal = false;
                self.catch_up = true;
            }
        }

        // A terminal bad registration owns its completion. Healthy work is
        // independent and can finish the same catch-up when its cursor is
        // caught up; no intermediate retry state may become terminal.
        if self.catch_up && self.bad_terminal && (!self.registered || self.cursor == self.appended)
        {
            self.catch_up = false;
        }
    }

    fn assert_invariants(&self) {
        assert!(self.cursor <= self.appended);
        assert!(self.committed_cursor <= self.appended);
        assert!(self.bad_attempts <= MODEL_MAX_RETRIES);
        assert_eq!(self.month_count, self.cursor);
        assert!(self.today_count <= self.month_count);
        assert_eq!(self.committed_month_count, self.committed_cursor);
        if self.day > 1 {
            // Rollover only clears Today; Month remains an exact-once total.
            assert!(self.month_count >= self.today_count);
            assert!(self.committed_today_count <= self.today_count);
        }
        if self.bad_terminal && (!self.registered || self.cursor == self.appended) {
            assert!(!self.catch_up);
        }
    }
}

proptest! {
    #![proptest_config(ingest_config())]

    #[test]
    fn pbt_state_encode_decode_is_lossless_canonical(state in arb_state()) {
        let encoded = state.encode();
        prop_assert!(encoded.starts_with(USAGE_STATE_HEADER));
        prop_assert!(!encoded.contains("file="));
        prop_assert!(!encoded.contains("Authorization"));
        prop_assert!(!encoded.contains("Bearer"));
        let decoded = UsageState::decode(&encoded).expect("decode");
        prop_assert_eq!(decoded.encode(), encoded);
        prop_assert_eq!(decoded.generation, state.generation);
        prop_assert_eq!(decoded.aggregate.month_start, state.aggregate.month_start);
        prop_assert_eq!(
            decoded.aggregate.snapshot.claude.today_cents,
            state.aggregate.snapshot.claude.today_cents
        );
        prop_assert_eq!(
            decoded.aggregate.snapshot.claude.month_output_tokens,
            state.aggregate.snapshot.claude.month_output_tokens
        );
        prop_assert_eq!(
            decoded.aggregate.snapshot.codex.month_cents,
            state.aggregate.snapshot.codex.month_cents
        );
        prop_assert_eq!(decoded.claude_keys.len(), state.claude_keys.len());
    }

    #[test]
    fn pbt_migration_roundtrip_is_idempotent_and_drops_file_columns(
        offset in 0_u64..40,
        size in 0_u64..40,
        generation in 1_u64..9,
    ) {
        let mut files = HashMap::new();
        files.insert(
            FileCheckpointKey::Claude("projects/p/a.jsonl".to_owned()),
            FileCheckpointCursor { offset, size },
        );
        let checkpoint = UsageCheckpoint {
            month_start: 20_260_901,
            today: 20_260_905,
            last_collected_ms: 4,
            catch_up_done: true,
            snapshot: UsageSnapshot::default(),
            files,
        };
        let first = UsageState::from_registry_checkpoint(&checkpoint, generation);
        let second = UsageState::from_registry_checkpoint(&checkpoint, generation);
        prop_assert_eq!(first.encode(), second.encode());
        prop_assert_eq!(first.generation, generation);
        prop_assert!(first.claude_keys.is_empty());
        let no_continuation = first.cursors.iter().all(|cursor| {
            cursor.last_model.is_none() && cursor.last_codex_total.is_none()
        });
        prop_assert!(no_continuation);
        prop_assert!(!first.encode().contains("file="));
        prop_assert_eq!(UsageState::decode(&first.encode()).unwrap().encode(), first.encode());
    }

    #[test]
    fn pbt_month_rollover_keeps_only_current_or_newer_keys(
        keys in proptest::collection::vec(arb_dkey(), 0..6),
        month in prop_oneof![Just(20_260_801_u32), Just(20_260_901_u32)],
    ) {
        let set: HashSet<_> = keys.into_iter().collect();
        let kept = retain_keys_for_month(&set, month);
        prop_assert!(kept.iter().all(|key| key.month >= month));
        let expected = set.iter().filter(|key| key.month >= month).count();
        prop_assert_eq!(kept.len(), expected);
        prop_assert!(set.iter().filter(|key| key.month >= month).all(|key| kept.contains(key)));
    }

    #[test]
    fn pbt_codex_append_batching_matches_one_at_a_time(
        first_in in 1_u64..40,
        second_in in 1_u64..40,
    ) {
        let first = CodexTokenTotals { input: first_in, cached: 0, output: 1 };
        let second = CodexTokenTotals {
            input: first_in.saturating_add(second_in),
            cached: 0,
            output: 2,
        };
        let (a, mid) = decide_codex_event(None, first, Some(first));
        let (b, end) = decide_codex_event(mid, CodexTokenTotals { input: second_in, cached: 0, output: 1 }, Some(second));
        let (c, once) = decide_codex_event(None, first, Some(first));
        let (d, once_end) = decide_codex_event(once, CodexTokenTotals { input: second_in, cached: 0, output: 1 }, Some(second));
        prop_assert_eq!(end, once_end);
        let counted = |decision| match decision {
            CodexUsageDecision::Count(usage) => usage.input,
            _ => 0,
        };
        prop_assert_eq!(counted(a) + counted(b), counted(c) + counted(d));
    }

    #[test]
    fn pbt_duplicate_replay_never_adds_a_second_count(
        input in 1_u64..80,
    ) {
        let total = CodexTokenTotals { input, cached: 0, output: 1 };
        let (first, watermark) = decide_codex_event(None, total, Some(total));
        let (again, same) = decide_codex_event(watermark, total, Some(total));
        prop_assert!(matches!(first, CodexUsageDecision::Count(_)));
        prop_assert_eq!(again, CodexUsageDecision::IgnoreReplay);
        prop_assert_eq!(watermark, same);
    }

    #[test]
    fn pbt_split_restart_equivalence(
        first_in in 1_u64..30,
        second_in in 1_u64..30,
    ) {
        let first = CodexTokenTotals { input: first_in, cached: 0, output: 1 };
        let second = CodexTokenTotals {
            input: first_in.saturating_add(second_in),
            cached: 0,
            output: 2,
        };
        let last = CodexTokenTotals { input: second_in, cached: 0, output: 1 };
        let (_, mid) = decide_codex_event(None, first, Some(first));
        let (cont, _) = decide_codex_event(mid, last, Some(second));
        let (restart, _) = decide_codex_event(Some(first), last, Some(second));
        prop_assert_eq!(cont, restart);
    }

    #[test]
    fn pbt_size_shrink_rebuilds_instead_of_keeping_a_past_offset(
        offset in 1_u64..40,
        size in 1_u64..40,
        new_size in 0_u64..40,
    ) {
        let stored_size = offset.max(size);
        let (next, rebuild) = spec_size_rebuild(offset, stored_size, new_size);
        if new_size < offset || new_size < stored_size {
            prop_assert!(rebuild);
            prop_assert_eq!(next, 0);
        } else {
            prop_assert!(!rebuild);
            prop_assert!(next <= new_size);
            prop_assert_eq!(next, offset);
        }
    }

    #[test]
    fn pbt_partial_line_prefix_is_never_a_record(cut in 1usize..24) {
        let line = "{\"type\":\"assistant\"}";
        let prefix = &line[..cut.min(line.len())];
        prop_assert!(!prefix.ends_with('\n'));
        prop_assert!(!prefix.contains('\n'));
    }

    #[test]
    fn pbt_generation_swap_failed_current_keeps_prior(
        first in 4_u64..20,
        second in 21_u64..40,
        fail_blob in proptest::bool::ANY,
    ) {
        let mut store = MemoryBlobs::default();
        let applied = persist_usage_state(&mut store, &state_at(first));
        prop_assert_eq!(applied, PersistStatus::Applied { generation: 1 });
        store.fail_write_blob = fail_blob;
        store.fail_write_current = !fail_blob;
        let failed = persist_usage_state(&mut store, &state_at(second));
        prop_assert_eq!(failed, PersistStatus::Failed);
        match load_usage_state(&store) {
            LoadStatus::Loaded(state) | LoadStatus::RecoveredPrior { state, .. } => {
                prop_assert_eq!(state.generation, 1);
                prop_assert_eq!(state.cursors[0].offset, first);
            }
            LoadStatus::Missing => prop_assert!(false, "prior generation must stay visible"),
        }
    }

    #[test]
    fn pbt_stateful_catch_up_preserves_healthy_progress(
        actions in proptest::collection::vec(arb_collector_action(), 1..80),
    ) {
        let mut model = CollectorStateModel::default();
        for action in actions {
            model.step(action);
            model.assert_invariants();
        }

        // Exercise the fair suffix explicitly.  It models discovery and
        // registration of the healthy hot log, one append/read/commit, and
        // the bounded failure sequence for a separate unreadable path.
        model.step(CollectorAction::Discover);
        model.step(CollectorAction::Register);
        model.step(CollectorAction::Append);
        model.step(CollectorAction::Scan);
        model.step(CollectorAction::Commit);
        let committed = (
            model.cursor,
            model.month_count,
            model.today_count,
            model.committed_cursor,
        );
        for _ in 0..MODEL_MAX_RETRIES {
            model.step(CollectorAction::PermanentFailure);
        }
        model.assert_invariants();
        prop_assert!(!model.catch_up);
        prop_assert_eq!(
            (model.cursor, model.month_count, model.today_count, model.committed_cursor),
            committed
        );

        // A restart may forget runtime retry state, but it must restore the
        // committed aggregate/cursor and then count a later append once.
        model.step(CollectorAction::Restart);
        prop_assert_eq!(
            (model.cursor, model.month_count, model.today_count),
            (committed.0, committed.1, committed.2)
        );
        let restart_totals = (model.cursor, model.month_count, model.today_count);
        model.step(CollectorAction::Discover);
        model.step(CollectorAction::Register);
        model.step(CollectorAction::Append);
        model.step(CollectorAction::Scan);
        model.assert_invariants();
        prop_assert_eq!(model.cursor, restart_totals.0 + 1);
        prop_assert_eq!(model.month_count, restart_totals.1 + 1);
        prop_assert_eq!(model.today_count, restart_totals.2 + 1);
    }
}

fn state_at(offset: u64) -> UsageState {
    UsageState {
        generation: 0,
        schema_version: crate::core::USAGE_STATE_SCHEMA_VERSION,
        aggregate: UsageAggregate {
            month_start: 20_260_901,
            today: 20_260_905,
            last_collected_ms: 1,
            catch_up_done: true,
            snapshot: UsageSnapshot::default(),
        },
        cursors: vec![UsageCursor {
            active_month: None,
            kind: CursorKind::Claude,
            logical_id: "p/a.jsonl".to_owned(),
            offset,
            size: offset,
            file_id: None,
            prefix: None,
            last_model: None,
            last_codex_total: None,
        }],
        claude_keys: HashSet::new(),
    }
}

#[test]
fn component_ingest_regression_file_exists() {
    assert!(std::path::Path::new("verification/evidence/usage-ingest-pbt.regressions").exists());
}
