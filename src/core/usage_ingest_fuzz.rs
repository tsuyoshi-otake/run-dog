//! Deterministic bounded fuzz driver for ingest and update boundaries.
//!
//! This is not libFuzzer / cargo-fuzz (unavailable on this Windows MSVC
//! toolchain). Cases, seed, and mutations are fixed so a failure reproduces.
//! Long campaigns are opt-in via `RUN_DOG_FUZZ_CASES` and must not be a CI gate.

use super::{UsageCheckpoint, UsageState, USAGE_STATE_HEADER};
use crate::update::{
    parse_checksum_manifest, select_update, Release, ReleaseAsset, UpdateRepository, Version,
    CHECKSUM_ASSET_NAME, INSTALLER_ASSET_NAME,
};

pub const FUZZ_SEED: u64 = 0x5EED_2026_0905_0003;
pub const DEFAULT_FUZZ_CASES: u32 = 1_024;
pub const MAX_FUZZ_INPUT: usize = 4_096;
pub const MAX_DECODED_ENCODED: usize = 64 * 1_024;

const FORBIDDEN: &[&str] = &[
    "Authorization",
    "Bearer ",
    "sk-ant-",
    "sk-proj-",
    "access_token",
    "refresh_token",
];

/// Last byte after a newline. A suffix without `\n` is never a complete record.
#[must_use]
pub fn last_safe_complete_record_offset(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map(|index| index as u64 + 1)
        .unwrap_or(0)
}

#[must_use]
pub fn contains_forbidden_secret(text: &str) -> bool {
    FORBIDDEN.iter().any(|needle| text.contains(needle))
}

#[must_use]
pub fn fuzz_case_count() -> u32 {
    std::env::var("RUN_DOG_FUZZ_CASES")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| (1..=100_000).contains(value))
        .unwrap_or(DEFAULT_FUZZ_CASES)
}

pub struct XorShift(u64);

impl XorShift {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn below(&mut self, limit: usize) -> usize {
        if limit == 0 {
            0
        } else {
            (self.next_u64() as usize) % limit
        }
    }
}

#[must_use]
pub fn mutate(rng: &mut XorShift, seed: &[u8]) -> Vec<u8> {
    let mut out = seed.to_vec();
    if out.is_empty() {
        out.push((rng.next_u64() & 0xff) as u8);
        return out;
    }
    match rng.below(5) {
        0 => {
            let index = rng.below(out.len());
            out[index] ^= 1 << rng.below(8);
        }
        1 => {
            if out.len() < MAX_FUZZ_INPUT {
                out.insert(rng.below(out.len() + 1), (rng.next_u64() & 0xff) as u8);
            }
        }
        2 => {
            if out.len() > 1 {
                let index = rng.below(out.len());
                out.remove(index);
            }
        }
        3 => {
            let end = rng.below(out.len()) + 1;
            out.truncate(end);
        }
        _ => {
            let index = rng.below(out.len());
            out[index] = (rng.next_u64() & 0xff) as u8;
        }
    }
    if out.len() > MAX_FUZZ_INPUT {
        out.truncate(MAX_FUZZ_INPUT);
    }
    out
}

#[must_use]
pub fn seed_corpus() -> Vec<Vec<u8>> {
    vec![
        include_bytes!("../../verification/evidence/fuzz-corpus/usage-state.txt").to_vec(),
        include_bytes!("../../verification/evidence/fuzz-corpus/legacy-checkpoint.txt").to_vec(),
        include_bytes!("../../verification/evidence/fuzz-corpus/claude.jsonl").to_vec(),
        include_bytes!("../../verification/evidence/fuzz-corpus/codex.jsonl").to_vec(),
        include_bytes!("../../verification/evidence/fuzz-corpus/timestamp.txt").to_vec(),
        include_bytes!("../../verification/evidence/fuzz-corpus/checksum.txt").to_vec(),
        include_bytes!("../../verification/evidence/fuzz-corpus/release.json").to_vec(),
        USAGE_STATE_HEADER.as_bytes().to_vec(),
        b"rundog-usage-checkpoint-3\nmonth=20260901\nday=20260905\nlast_ms=1\ncatch_up_done=1\nclaude_today=0\nclaude_month=0\nclaude_in=0\nclaude_out=0\ncodex_today=0\ncodex_month=0\ncodex_in=0\ncodex_out=0\n".to_vec(),
        b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  RunDog-Setup-x64.exe\n".to_vec(),
        b"2026-09-05T12:00:00Z".to_vec(),
        br#"{"tag_name":"v1.1.22","draft":false,"prerelease":false,"assets":[]}"#.to_vec(),
    ]
}

fn assert_no_secrets(bytes: &[u8]) {
    if let Ok(text) = std::str::from_utf8(bytes) {
        assert!(
            !contains_forbidden_secret(text),
            "fuzz corpus or output contained a forbidden secret marker"
        );
    }
}

fn fuzz_usage_state(bytes: &[u8]) {
    assert_no_secrets(bytes);
    let Ok(text) = std::str::from_utf8(bytes) else {
        return;
    };
    if let Some(state) = UsageState::decode(text) {
        let encoded = state.encode();
        assert!(encoded.len() <= MAX_DECODED_ENCODED);
        assert!(!contains_forbidden_secret(&encoded));
        assert!(!encoded.contains("file="));
        for cursor in &state.cursors {
            assert!(cursor.offset <= cursor.size);
        }
    }
}

fn fuzz_legacy_checkpoint(bytes: &[u8]) {
    assert_no_secrets(bytes);
    let Ok(text) = std::str::from_utf8(bytes) else {
        return;
    };
    if let Some(checkpoint) = UsageCheckpoint::decode(text) {
        let encoded = checkpoint.encode();
        assert!(encoded.len() <= MAX_DECODED_ENCODED);
        assert!(!contains_forbidden_secret(&encoded));
        for cursor in checkpoint.files.values() {
            assert!(cursor.offset <= cursor.size || cursor.size == 0);
        }
    }
}

fn fuzz_checksum(bytes: &[u8]) {
    assert_no_secrets(bytes);
    let Ok(text) = std::str::from_utf8(bytes) else {
        return;
    };
    let _ = parse_checksum_manifest(text);
}

fn fuzz_update_boundary(bytes: &[u8]) {
    assert_no_secrets(bytes);
    let Ok(text) = std::str::from_utf8(bytes) else {
        return;
    };
    let _ = Version::parse(text.trim());
    if let Ok(repository) = UpdateRepository::new("example-org/run-dog") {
        let current = Version::parse("1.1.21").expect("fixture version");
        let release = Release {
            tag_name: text.chars().take(32).collect(),
            draft: false,
            prerelease: false,
            assets: vec![
                ReleaseAsset {
                    name: INSTALLER_ASSET_NAME.to_owned(),
                    browser_download_url: format!(
                        "https://github.com/example-org/run-dog/releases/download/v9.9.9/{INSTALLER_ASSET_NAME}"
                    ),
                },
                ReleaseAsset {
                    name: CHECKSUM_ASSET_NAME.to_owned(),
                    browser_download_url: format!(
                        "https://github.com/example-org/run-dog/releases/download/v9.9.9/{CHECKSUM_ASSET_NAME}"
                    ),
                },
            ],
        };
        let _ = select_update(&repository, &current, &release);
    }
}

fn fuzz_jsonl_cursor(bytes: &[u8]) {
    let safe = last_safe_complete_record_offset(bytes);
    assert!(safe <= bytes.len() as u64);
    if !bytes.contains(&b'\n') {
        assert_eq!(safe, 0);
    }
    if let Some(last) = bytes.iter().rposition(|&byte| byte == b'\n') {
        assert_eq!(safe, last as u64 + 1);
        if let Some(tail) = bytes.get(safe as usize..) {
            assert!(
                !tail.contains(&b'\n'),
                "safe offset must not sit past a later complete record"
            );
        }
    }
}

/// Runs the default campaign. Panic is a finding.
pub fn run_bounded_ingest_fuzz(cases: u32) {
    let corpus = seed_corpus();
    for seed in &corpus {
        assert!(seed.len() <= MAX_FUZZ_INPUT * 2);
        assert_no_secrets(seed);
    }
    let mut rng = XorShift::new(FUZZ_SEED);
    for case in 0..cases {
        let seed = &corpus[rng.below(corpus.len())];
        let input = mutate(&mut rng, seed);
        match case % 5 {
            0 => fuzz_usage_state(&input),
            1 => fuzz_legacy_checkpoint(&input),
            2 => fuzz_checksum(&input),
            3 => fuzz_update_boundary(&input),
            _ => fuzz_jsonl_cursor(&input),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        fuzz_case_count, last_safe_complete_record_offset, run_bounded_ingest_fuzz, FUZZ_SEED,
    };

    #[test]
    fn component_last_safe_offset_ignores_partial_suffix() {
        assert_eq!(last_safe_complete_record_offset(b""), 0);
        assert_eq!(last_safe_complete_record_offset(b"{"), 0);
        assert_eq!(last_safe_complete_record_offset(b"{}\n{"), 3);
        assert_eq!(last_safe_complete_record_offset(b"{}\n{}\n"), 6);
    }

    #[test]
    fn component_bounded_ingest_fuzz_does_not_panic() {
        assert_eq!(FUZZ_SEED, 0x5EED_2026_0905_0003);
        run_bounded_ingest_fuzz(fuzz_case_count());
    }
}
