//! Durable Claude assistant dedupe keys.
//!
//! The disk contract is FNV-1a 128 of a length-prefixed (message, request)
//! pair, plus the event's local month. Raw message/request IDs are not stored.
//!
//! Collision policy: a matching digest+month is treated as already counted and
//! skipped. That prefers No Double Count over inventing a second identity we
//! cannot prove. 128-bit FNV-1a is not claimed collision-free.

use std::collections::HashSet;

/// Public-domain FNV-1a 128 offset basis.
const FNV1A128_OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
/// Public-domain FNV-1a 128 prime.
const FNV1A128_PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013B;

const DOMAIN: &[u8] = b"rundog-claude-dedupe-1";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ClaudeDedupeKey {
    pub digest: [u8; 16],
    /// `ymd_key(year, month, 1)` of the event. Same grain as month aggregates.
    pub month: u32,
}

impl ClaudeDedupeKey {
    #[must_use]
    pub fn new(message_id: &str, request_id: &str, month: u32) -> Self {
        Self {
            digest: claude_dedupe_digest(message_id, request_id),
            month,
        }
    }

    #[must_use]
    pub fn encode_hex(self) -> String {
        hex_encode(&self.digest)
    }
}

/// Length-prefixed so (`"ab"`, `"c"`) and (`"a"`, `"bc"`) do not collide.
#[must_use]
pub fn claude_dedupe_digest(message_id: &str, request_id: &str) -> [u8; 16] {
    let mut bytes =
        Vec::with_capacity(DOMAIN.len() + 1 + 8 + message_id.len() + 8 + request_id.len());
    bytes.extend_from_slice(DOMAIN);
    bytes.push(0);
    bytes.extend_from_slice(&(message_id.len() as u64).to_le_bytes());
    bytes.extend_from_slice(message_id.as_bytes());
    bytes.extend_from_slice(&(request_id.len() as u64).to_le_bytes());
    bytes.extend_from_slice(request_id.as_bytes());
    fnv1a_128(&bytes).to_be_bytes()
}

/// Drop keys whose event month is older than the current aggregate month.
#[must_use]
pub fn retain_keys_for_month(
    keys: &HashSet<ClaudeDedupeKey>,
    month_start: u32,
) -> HashSet<ClaudeDedupeKey> {
    keys.iter()
        .copied()
        .filter(|key| key.month >= month_start)
        .collect()
}

fn fnv1a_128(bytes: &[u8]) -> u128 {
    let mut hash = FNV1A128_OFFSET;
    for &byte in bytes {
        hash ^= u128::from(byte);
        hash = hash.wrapping_mul(FNV1A128_PRIME);
    }
    hash
}

fn hex_encode(bytes: &[u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(32);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[must_use]
pub fn hex_decode(text: &str) -> Option<[u8; 16]> {
    if text.len() != 32 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0_u8; 16];
    for (index, chunk) in text.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        out[index] = u8::from_str_radix(core::str::from_utf8(chunk).ok()?, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{
        claude_dedupe_digest, fnv1a_128, hex_decode, retain_keys_for_month, ClaudeDedupeKey,
        FNV1A128_OFFSET,
    };
    use std::collections::HashSet;

    #[test]
    fn component_fnv1a128_empty_is_the_public_offset() {
        assert_eq!(fnv1a_128(b""), FNV1A128_OFFSET);
    }

    #[test]
    fn component_length_prefix_separates_adjacent_fields() {
        let left = claude_dedupe_digest("ab", "c");
        let right = claude_dedupe_digest("a", "bc");
        assert_ne!(left, right);
    }

    #[test]
    fn component_digest_is_stable_for_the_same_pair() {
        let first = claude_dedupe_digest("m1", "r1");
        let second = claude_dedupe_digest("m1", "r1");
        assert_eq!(first, second);
        assert_eq!(
            hex_decode(&ClaudeDedupeKey::new("m1", "r1", 20_260_901).encode_hex()),
            Some(first)
        );
    }

    #[test]
    fn component_collision_policy_skips_the_later_event() {
        let key = ClaudeDedupeKey::new("m1", "r1", 20_260_901);
        let mut seen = HashSet::new();
        assert!(seen.insert(key));
        assert!(!seen.insert(key), "same digest+month is already counted");
    }

    #[test]
    fn component_month_rollover_keeps_current_month_keys() {
        let old = ClaudeDedupeKey::new("old", "old", 20_260_801);
        let current = ClaudeDedupeKey::new("cur", "cur", 20_260_901);
        let kept = retain_keys_for_month(&[old, current].into_iter().collect(), 20_260_901);
        assert!(!kept.contains(&old));
        assert!(kept.contains(&current));
    }

    #[test]
    fn component_encoded_key_does_not_contain_raw_ids() {
        let key = ClaudeDedupeKey::new("msg_secret", "req_secret", 20_260_901);
        let encoded = format!("dkey={}\t{}\n", key.encode_hex(), key.month);
        assert!(!encoded.contains("msg_secret"));
        assert!(!encoded.contains("req_secret"));
        assert!(!encoded.contains("DefaultHasher"));
    }
}
