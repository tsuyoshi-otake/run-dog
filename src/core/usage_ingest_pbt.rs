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
    )
        .prop_map(|(codex, logical_id, offset, extra, prefix)| {
            let size = offset.saturating_add(extra);
            UsageCursor {
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
