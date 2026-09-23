//! Bounded, per-file usage-event deduplication matching otak-usage.

use std::collections::{HashMap, HashSet, VecDeque};

use super::TokenUsage;

pub const SEEN_CAP_CLAUDE: usize = 512;
pub const SEEN_CAP_CODEX: usize = 32;
pub const PENDING_CAP_CLAUDE: usize = 32;
pub const PENDING_RETENTION_MS: u64 = 30 * 60 * 1_000;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct UsageDedupeKey {
    pub digest: u64,
    pub month: u32,
}

impl UsageDedupeKey {
    #[must_use]
    pub const fn new(digest: u64, month: u32) -> Self {
        Self { digest, month }
    }

    #[must_use]
    pub fn encode_hex(self) -> String {
        format!("{:016x}", self.digest)
    }
}

/// The latest Claude contribution for a message that can still be revised.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaudePendingUsage {
    pub key: UsageDedupeKey,
    /// Local `YYYYMMDD` event day, used to reverse the earlier aggregate.
    pub day: u32,
    pub model: String,
    pub usage: TokenUsage,
}

/// Runtime state for one source file. Membership is O(1); the bounded order
/// queues reproduce otak-usage's oldest-first eviction and revision window.
#[derive(Clone, Debug, Default)]
pub struct FileDedupeWindow {
    order: VecDeque<UsageDedupeKey>,
    seen: HashSet<UsageDedupeKey>,
    /// The last 32 ingested keys, including events without revision state.
    recent_order: VecDeque<UsageDedupeKey>,
    pending: HashMap<UsageDedupeKey, ClaudePendingUsage>,
}

impl FileDedupeWindow {
    #[must_use]
    pub fn from_parts(seen_keys: Vec<UsageDedupeKey>, pending: Vec<ClaudePendingUsage>) -> Self {
        let mut window = Self::default();
        for key in seen_keys {
            if window.seen.insert(key) {
                window.order.push_back(key);
            }
        }
        window.recent_order.extend(
            window
                .order
                .iter()
                .skip(window.order.len().saturating_sub(PENDING_CAP_CLAUDE))
                .copied(),
        );
        for event in pending {
            if window.recent_order.contains(&event.key) {
                window.pending.entry(event.key).or_insert(event);
            }
        }
        window
    }

    #[must_use]
    pub fn contains(&self, key: UsageDedupeKey) -> bool {
        self.seen.contains(&key)
    }

    #[must_use]
    pub fn pending(&self, key: UsageDedupeKey) -> Option<&ClaudePendingUsage> {
        self.pending.get(&key)
    }

    /// Add or supersede a key and anchor it at the newest position.
    pub fn remember(
        &mut self,
        key: UsageDedupeKey,
        revision: Option<ClaudePendingUsage>,
        seen_cap: usize,
    ) {
        if self.seen.remove(&key) {
            self.order.retain(|existing| *existing != key);
            self.recent_order.retain(|existing| *existing != key);
            self.remove_pending(key);
        }
        self.order.push_back(key);
        self.recent_order.push_back(key);
        self.seen.insert(key);
        if let Some(revision) = revision {
            self.pending.insert(key, revision);
        }
        while self.recent_order.len() > PENDING_CAP_CLAUDE {
            if let Some(oldest) = self.recent_order.pop_front() {
                self.remove_pending(oldest);
            }
        }
        while self.order.len() > seen_cap {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
                self.recent_order.retain(|existing| *existing != oldest);
                self.remove_pending(oldest);
            }
        }
    }

    pub fn clear_pending(&mut self) -> bool {
        let changed = !self.pending.is_empty();
        self.pending.clear();
        changed
    }

    pub fn retain_month(&mut self, month_start: u32) {
        self.order.retain(|key| key.month >= month_start);
        self.seen.retain(|key| key.month >= month_start);
        self.recent_order
            .retain(|key| key.month >= month_start && self.seen.contains(key));
        let recent_order = &self.recent_order;
        self.pending.retain(|key, _| recent_order.contains(key));
    }

    #[must_use]
    pub fn seen_keys(&self) -> Vec<UsageDedupeKey> {
        self.order.iter().copied().collect()
    }

    #[must_use]
    pub fn pending_values(&self) -> Vec<ClaudePendingUsage> {
        self.recent_order
            .iter()
            .filter_map(|key| self.pending.get(key).cloned())
            .collect()
    }

    fn remove_pending(&mut self, key: UsageDedupeKey) {
        self.pending.remove(&key);
    }
}

#[must_use]
pub fn claude_dedupe_digest(message_id: &str, request_id: &str) -> u64 {
    digest(format!("claude:{message_id}:{request_id}").as_bytes())
}

#[must_use]
pub fn claude_anonymous_dedupe_digest(timestamp: &str, model: &str, usage: TokenUsage) -> u64 {
    digest(
        format!(
            "anon:{timestamp}:{model}:{}:{}:{}:{}:{}",
            usage.input, usage.cache_read, usage.cache_write_5m, usage.cache_write_1h, usage.output,
        )
        .as_bytes(),
    )
}

#[must_use]
pub fn codex_dedupe_digest(timestamp_ms: u64, usage: TokenUsage) -> u64 {
    digest(
        format!(
            "codex:{timestamp_ms}:{}:{}:{}",
            usage.input, usage.cached_input, usage.output
        )
        .as_bytes(),
    )
}

#[must_use]
pub fn hex_decode(text: &str) -> Option<u64> {
    if text.len() != 16 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(text, 16).ok()
}

fn digest(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
}

#[cfg(test)]
mod tests {
    use super::{
        claude_dedupe_digest, codex_dedupe_digest, hex_decode, ClaudePendingUsage,
        FileDedupeWindow, UsageDedupeKey, PENDING_CAP_CLAUDE, SEEN_CAP_CLAUDE,
    };
    use crate::core::TokenUsage;

    fn key(digest: u64) -> UsageDedupeKey {
        UsageDedupeKey::new(digest, 20_260_901)
    }

    fn pending(digest: u64) -> ClaudePendingUsage {
        ClaudePendingUsage {
            key: key(digest),
            day: 20_260_905,
            model: "claude-opus-4-8".to_owned(),
            usage: TokenUsage {
                output: digest,
                ..TokenUsage::default()
            },
        }
    }

    #[test]
    fn component_event_digests_are_stable_and_provider_scoped() {
        let usage = TokenUsage {
            input: 4,
            cached_input: 2,
            output: 1,
            ..TokenUsage::default()
        };
        assert_eq!(
            codex_dedupe_digest(1_780_000_000_000, usage),
            codex_dedupe_digest(1_780_000_000_000, usage)
        );
        assert_ne!(
            codex_dedupe_digest(1_780_000_000_000, usage),
            claude_dedupe_digest("1", "780000000000")
        );
        assert_eq!(hex_decode(&format!("{:016x}", 42)), Some(42));
    }

    #[test]
    fn component_window_suppresses_duplicates_and_reanchors_revisions() {
        let mut window = FileDedupeWindow::default();
        window.remember(key(1), Some(pending(1)), 32);
        assert!(window.contains(key(1)));
        window.remember(key(2), None, 32);
        window.remember(key(1), Some(pending(3)), 32);
        assert_eq!(window.seen_keys(), vec![key(2), key(1)]);
        assert_eq!(window.pending(key(1)).unwrap().usage.output, 3);
    }

    #[test]
    fn component_window_bounds_presence_and_supersede_state_separately() {
        let mut window = FileDedupeWindow::default();
        for value in 0..64 {
            window.remember(key(value), Some(pending(value)), 512);
        }
        assert_eq!(window.pending_values().len(), PENDING_CAP_CLAUDE);
        assert!(window.pending(key(63)).is_some());
        assert!(window.pending(key(31)).is_none());
        for value in 64..600 {
            window.remember(key(value), None, 32);
        }
        assert_eq!(window.seen_keys().len(), 32);
        assert!(!window.contains(key(0)));
    }

    #[test]
    fn component_revision_state_expires_after_thirty_two_new_file_events() {
        let mut window = FileDedupeWindow::default();
        window.remember(key(1), Some(pending(1)), SEEN_CAP_CLAUDE);
        for value in 2..=33 {
            window.remember(key(value), None, SEEN_CAP_CLAUDE);
        }

        assert!(window.contains(key(1)));
        assert!(window.pending(key(1)).is_none());
    }
}
