//! Atomic generation protocol for usage durable state.
//!
//! Blob write must succeed before `current` advances. A failed persist never
//! reports success. Load prefers a valid prior generation over an empty rebuild.

use super::UsageState;
use std::collections::BTreeMap;

pub const MAX_PRIOR_GENERATIONS: u64 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistStatus {
    Applied { generation: u64 },
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LoadStatus {
    Loaded(UsageState),
    RecoveredPrior {
        state: UsageState,
        requested: Option<u64>,
    },
    Missing,
}

pub trait GenerationBlobs {
    fn write_blob(&mut self, generation: u64, bytes: &[u8]) -> bool;
    fn read_blob(&self, generation: u64) -> Option<Vec<u8>>;
    fn write_current(&mut self, generation: u64) -> bool;
    fn read_current(&self) -> Option<u64>;
    fn highest_blob_generation(&self) -> Option<u64>;
}

#[derive(Clone, Debug, Default)]
pub struct MemoryBlobs {
    pub current: Option<u64>,
    pub blobs: BTreeMap<u64, Vec<u8>>,
    pub fail_write_blob: bool,
    pub fail_write_current: bool,
}

impl GenerationBlobs for MemoryBlobs {
    fn write_blob(&mut self, generation: u64, bytes: &[u8]) -> bool {
        if self.fail_write_blob {
            return false;
        }
        self.blobs.insert(generation, bytes.to_vec());
        true
    }

    fn read_blob(&self, generation: u64) -> Option<Vec<u8>> {
        self.blobs.get(&generation).cloned()
    }

    fn write_current(&mut self, generation: u64) -> bool {
        if self.fail_write_current {
            return false;
        }
        self.current = Some(generation);
        true
    }

    fn read_current(&self) -> Option<u64> {
        self.current
    }

    fn highest_blob_generation(&self) -> Option<u64> {
        self.blobs.keys().next_back().copied()
    }
}

#[must_use]
pub fn persist_usage_state(store: &mut impl GenerationBlobs, state: &UsageState) -> PersistStatus {
    // Recovery can succeed without a current pointer. Never reuse an existing
    // generation (including an unpublished blob left by an interrupted save).
    let highest = store
        .read_current()
        .unwrap_or(0)
        .max(store.highest_blob_generation().unwrap_or(0))
        .max(state.generation);
    let Some(generation) = highest.checked_add(1) else {
        return PersistStatus::Failed;
    };
    let mut next = state.clone();
    next.generation = generation;
    next.schema_version = super::usage_state::USAGE_STATE_SCHEMA_VERSION;
    let payload = next.encode();
    if !store.write_blob(generation, payload.as_bytes()) {
        return PersistStatus::Failed;
    }
    if !store.write_current(generation) {
        return PersistStatus::Failed;
    }
    PersistStatus::Applied { generation }
}

#[must_use]
pub fn load_usage_state(store: &impl GenerationBlobs) -> LoadStatus {
    if let Some(requested) = store.read_current() {
        if let Some(state) = decode_blob(store, requested) {
            return LoadStatus::Loaded(state);
        }
        if let Some(state) = recover_prior(store, requested) {
            return LoadStatus::RecoveredPrior {
                state,
                requested: Some(requested),
            };
        }
        return LoadStatus::Missing;
    }
    if let Some(highest) = store.highest_blob_generation() {
        if let Some(state) = decode_blob(store, highest).or_else(|| recover_prior(store, highest)) {
            return LoadStatus::RecoveredPrior {
                state,
                requested: None,
            };
        }
    }
    LoadStatus::Missing
}

fn decode_blob(store: &impl GenerationBlobs, generation: u64) -> Option<UsageState> {
    let bytes = store.read_blob(generation)?;
    let text = core::str::from_utf8(&bytes).ok()?;
    let state = UsageState::decode(text)?;
    (state.generation == generation).then_some(state)
}

fn recover_prior(store: &impl GenerationBlobs, requested: u64) -> Option<UsageState> {
    let start = requested.saturating_sub(1);
    let floor = requested.saturating_sub(MAX_PRIOR_GENERATIONS);
    let mut generation = start;
    loop {
        if generation == 0 {
            return decode_blob(store, 0);
        }
        if let Some(state) = decode_blob(store, generation) {
            return Some(state);
        }
        if generation <= floor {
            return None;
        }
        generation -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{load_usage_state, persist_usage_state, LoadStatus, MemoryBlobs, PersistStatus};
    use crate::core::{CursorKind, UsageAggregate, UsageCursor, UsageSnapshot, UsageState};

    fn state_with_offset(offset: u64) -> UsageState {
        UsageState {
            generation: 0,
            schema_version: crate::core::USAGE_STATE_SCHEMA_VERSION,
            aggregate: UsageAggregate {
                month_start: 20_260_901,
                today: 20_260_905,
                last_collected_ms: 1,
                catch_up_done: false,
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
            claude_keys: std::collections::HashSet::new(),
        }
    }

    #[test]
    fn component_exhausted_generation_does_not_overwrite_the_last_blob() {
        let mut store = MemoryBlobs {
            current: Some(u64::MAX),
            ..MemoryBlobs::default()
        };
        store.blobs.insert(u64::MAX, b"prior".to_vec());
        assert_eq!(
            persist_usage_state(&mut store, &state_with_offset(16)),
            PersistStatus::Failed
        );
        assert_eq!(store.blobs[&u64::MAX], b"prior");
        assert_eq!(store.current, Some(u64::MAX));
    }

    #[test]
    fn component_failed_blob_write_is_not_reported_durable() {
        let mut store = MemoryBlobs {
            fail_write_blob: true,
            ..MemoryBlobs::default()
        };
        assert_eq!(
            persist_usage_state(&mut store, &state_with_offset(8)),
            PersistStatus::Failed
        );
        assert_eq!(load_usage_state(&store), LoadStatus::Missing);
        assert!(store.current.is_none());
        assert!(store.blobs.is_empty());
    }

    #[test]
    fn component_failed_current_write_keeps_prior_generation() {
        let mut store = MemoryBlobs::default();
        assert_eq!(
            persist_usage_state(&mut store, &state_with_offset(8)),
            PersistStatus::Applied { generation: 1 }
        );
        store.fail_write_current = true;
        assert_eq!(
            persist_usage_state(&mut store, &state_with_offset(16)),
            PersistStatus::Failed
        );
        match load_usage_state(&store) {
            LoadStatus::Loaded(state) => {
                assert_eq!(state.generation, 1);
                assert_eq!(state.cursors[0].offset, 8);
            }
            other => panic!("expected prior generation, got {other:?}"),
        }
    }

    #[test]
    fn component_corrupt_current_blob_recovers_valid_prior() {
        let mut store = MemoryBlobs::default();
        assert!(matches!(
            persist_usage_state(&mut store, &state_with_offset(8)),
            PersistStatus::Applied { generation: 1 }
        ));
        assert!(matches!(
            persist_usage_state(&mut store, &state_with_offset(16)),
            PersistStatus::Applied { generation: 2 }
        ));
        store.blobs.insert(2, b"truncated".to_vec());
        match load_usage_state(&store) {
            LoadStatus::RecoveredPrior { state, requested } => {
                assert_eq!(requested, Some(2));
                assert_eq!(state.generation, 1);
                assert_eq!(state.cursors[0].offset, 8);
            }
            other => panic!("expected recovered prior, got {other:?}"),
        }
    }

    #[test]
    fn component_missing_current_recovers_highest_valid_blob() {
        let mut store = MemoryBlobs::default();
        assert!(matches!(
            persist_usage_state(&mut store, &state_with_offset(8)),
            PersistStatus::Applied { generation: 1 }
        ));
        store.current = None;
        match load_usage_state(&store) {
            LoadStatus::RecoveredPrior { state, requested } => {
                assert_eq!(requested, None);
                assert_eq!(state.generation, 1);
            }
            other => panic!("expected blob recovery, got {other:?}"),
        }
    }

    #[test]
    fn component_recover_prior_walks_over_a_missing_gap() {
        let mut store = MemoryBlobs::default();
        assert!(matches!(
            persist_usage_state(&mut store, &state_with_offset(8)),
            PersistStatus::Applied { generation: 1 }
        ));
        assert!(matches!(
            persist_usage_state(&mut store, &state_with_offset(16)),
            PersistStatus::Applied { generation: 2 }
        ));
        assert!(matches!(
            persist_usage_state(&mut store, &state_with_offset(24)),
            PersistStatus::Applied { generation: 3 }
        ));
        store.blobs.remove(&2);
        store.blobs.insert(3, b"truncated".to_vec());
        match load_usage_state(&store) {
            LoadStatus::RecoveredPrior { state, requested } => {
                assert_eq!(requested, Some(3));
                assert_eq!(state.generation, 1);
                assert_eq!(state.cursors[0].offset, 8);
            }
            other => panic!("expected gap walk to generation 1, got {other:?}"),
        }
    }

    #[test]
    fn component_missing_current_prefers_highest_blob_not_generation_one() {
        let mut store = MemoryBlobs::default();
        assert!(matches!(
            persist_usage_state(&mut store, &state_with_offset(8)),
            PersistStatus::Applied { generation: 1 }
        ));
        assert!(matches!(
            persist_usage_state(&mut store, &state_with_offset(16)),
            PersistStatus::Applied { generation: 2 }
        ));
        store.current = None;
        store.blobs.remove(&1);
        match load_usage_state(&store) {
            LoadStatus::RecoveredPrior { state, requested } => {
                assert_eq!(requested, None);
                assert_eq!(state.generation, 2);
                assert_eq!(state.cursors[0].offset, 16);
            }
            other => panic!("expected highest blob 2, got {other:?}"),
        }
    }
}
