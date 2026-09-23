//! Read-only bridge to otak-usage's fenced, per-day scan snapshot.
//!
//! This reader intentionally does not touch the extension's mutable cache or
//! lease heartbeat. A stopped VS Code window can leave a useful snapshot, but
//! the lock pointer must identify the same immutable artifact before and after
//! the read.

use std::{fs::File, io::Read, path::Path};

use serde_json::Value;

use crate::core::{cost_nanos, local_ymd, ymd_key, TokenUsage};

const MAX_JSON_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOKEN_FIELD: u64 = 1_000_000_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CodexSnapshot {
    pub today_cost_nanos: u64,
    pub month_cost_nanos: u64,
    pub month_input_tokens: u64,
    pub month_output_tokens: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Fence {
    epoch: u64,
    holder: String,
    token: String,
}

/// `month` is YYYYMM01 and `today` is YYYYMMDD in the machine's local time.
/// A missing, stale-month, changed, or malformed extension snapshot is ignored.
pub(super) fn read_codex_snapshot(
    storage_dir: &Path,
    claude_dir: &Path,
    codex_home: &Path,
    month: u32,
    today: u32,
) -> Option<CodexSnapshot> {
    if month % 100 != 1 || today / 100 != month / 100 || today < month {
        return None;
    }
    let key = group_key(claude_dir, codex_home)?;
    let lock_path = storage_dir.join(format!("scan-leader-{key}.lock"));
    let fence = read_lock_fence(&lock_path)?;
    let token: String = fence
        .token
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let snapshot_path = storage_dir.join(format!(
        "scan-snapshot-{key}.json.epoch-{}.{token}.json",
        fence.epoch
    ));
    let raw = read_bounded_json(&snapshot_path)?;
    let result = parse_snapshot(&raw, &fence, month, today)?;
    (read_lock_fence(&lock_path)? == fence).then_some(result)
}

fn read_lock_fence(path: &Path) -> Option<Fence> {
    let lock = read_bounded_json(path)?;
    if lock.get("version")?.as_u64()? != 2 {
        return None;
    }
    parse_fence(&lock)
}

fn parse_fence(value: &Value) -> Option<Fence> {
    let epoch = value.get("epoch")?.as_u64()?;
    let holder = value.get("holder")?.as_str()?;
    let token = value.get("leaseToken")?.as_str()?;
    if epoch == 0 || holder.is_empty() || holder.len() > 256 || !(16..=128).contains(&token.len()) {
        return None;
    }
    Some(Fence {
        epoch,
        holder: holder.to_owned(),
        token: token.to_owned(),
    })
}

fn read_bounded_json(path: &Path) -> Option<Value> {
    let mut file = File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_JSON_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_JSON_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn parse_snapshot(raw: &Value, fence: &Fence, month: u32, today: u32) -> Option<CodexSnapshot> {
    if raw.get("version")?.as_u64()? != 1
        || raw.get("fence").and_then(parse_fence).as_ref()? != fence
        || raw.get("leader")?.as_str()?.is_empty()
        || !raw.get("claudeAvailable")?.is_boolean()
        || !raw.get("codexAvailable")?.as_bool()?
    {
        return None;
    }
    let updated_at_ms = raw.get("updatedAtMs")?.as_u64()?;
    // The extension uses local calendar days. Its snapshot must have been
    // published during the requested month, even when the lease is now stale.
    let (year, month_of_year, day) =
        local_ymd(updated_at_ms, super::usage::timezone_bias_minutes());
    let updated_day = ymd_key(year, month_of_year, day);
    if updated_day / 100 != month / 100 || updated_day > today {
        return None;
    }

    let mut result = CodexSnapshot {
        today_cost_nanos: 0,
        month_cost_nanos: 0,
        month_input_tokens: 0,
        month_output_tokens: 0,
    };
    let days = raw.get("days")?.as_object()?;
    if days.is_empty() || days.len() > 31 {
        return None;
    }
    for (day_key, bucket) in days {
        let day = parse_day(day_key)?;
        if day / 100 != month / 100 || day > today {
            return None;
        }
        let models = bucket.as_object()?;
        if models.len() > 512 {
            return None;
        }
        for (key, raw_usage) in models {
            let usage = parse_usage(raw_usage)?;
            let Some(model) = key.strip_prefix("codex/") else {
                if key.starts_with("claude/") {
                    continue;
                }
                return None;
            };
            if model.is_empty() || model.len() > 256 {
                return None;
            }
            // otak-usage counts unknown models as $0, but retains their tokens.
            let cost = cost_nanos(model, usage, Some(day_key)).unwrap_or(0);
            result.month_cost_nanos = result.month_cost_nanos.checked_add(cost)?;
            result.month_input_tokens = result
                .month_input_tokens
                .checked_add(usage.processed_input_tokens())?;
            result.month_output_tokens = result.month_output_tokens.checked_add(usage.output)?;
            if day == today {
                result.today_cost_nanos = result.today_cost_nanos.checked_add(cost)?;
            }
        }
    }
    Some(result)
}

fn parse_day(value: &str) -> Option<u32> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(i, b)| i != 4 && i != 7 && !b.is_ascii_digit())
    {
        return None;
    }
    let year = value[..4].parse::<u32>().ok()?;
    let month = value[5..7].parse::<u32>().ok()?;
    let day = value[8..10].parse::<u32>().ok()?;
    let days_in_month = match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return None,
    };
    if day == 0 || day > days_in_month || year < 2000 {
        return None;
    }
    Some(year * 10_000 + month * 100 + day)
}

fn parse_usage(raw: &Value) -> Option<TokenUsage> {
    let object = raw.as_object()?;
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "input"
                | "cachedInput"
                | "cacheRead"
                | "cacheWrite5m"
                | "cacheWrite1h"
                | "output"
                | "longContextInput"
                | "longContextCachedInput"
                | "longContextOutput"
        ) {
            return None;
        }
    }
    let field = |name: &str, required: bool| -> Option<u64> {
        let n = match object.get(name) {
            Some(value) => value.as_u64()?,
            None if !required => 0,
            None => return None,
        };
        (n <= MAX_TOKEN_FIELD).then_some(n)
    };
    Some(TokenUsage {
        input: field("input", true)?,
        cached_input: field("cachedInput", true)?,
        cache_read: field("cacheRead", true)?,
        cache_write_5m: field("cacheWrite5m", true)?,
        cache_write_1h: field("cacheWrite1h", true)?,
        output: field("output", true)?,
        long_context_input: field("longContextInput", false)?,
        long_context_cached_input: field("longContextCachedInput", false)?,
        long_context_output: field("longContextOutput", false)?,
    })
}

/// Exact JavaScript `hashKey()` over Windows `path.resolve(...).toLowerCase()`.
fn group_key(claude_dir: &Path, codex_home: &Path) -> Option<String> {
    fn normalize(path: &Path) -> Option<String> {
        if !path.is_absolute() {
            return None;
        }
        let path = path.to_str()?.replace('/', "\\").to_lowercase();
        Some(path.trim_end_matches('\\').to_owned())
    }
    let joined = format!("{}\0{}", normalize(claude_dir)?, normalize(codex_home)?);
    let mut h1 = 0x811c_9dc5_u32;
    let mut h2 = 0x0100_0193_u32;
    for c in joined.encode_utf16() {
        h1 = (h1 ^ u32::from(c)).wrapping_mul(0x0100_0193);
        h2 = h2.wrapping_add(u32::from(c)).wrapping_mul(0x85eb_ca6b) ^ (h1 >> 13);
    }
    let hash = (u64::from(h2 >> 11) << 32) | u64::from(h1);
    Some(to_base36(hash))
}

fn to_base36(mut value: u64) -> String {
    let mut chars = Vec::new();
    loop {
        let digit = (value % 36) as u8;
        chars.push(if digit < 10 {
            b'0' + digit
        } else {
            b'a' + digit - 10
        });
        value /= 36;
        if value == 0 {
            break;
        }
    }
    chars.reverse();
    String::from_utf8(chars).expect("base36 is ASCII")
}

#[cfg(test)]
mod tests {
    use super::{group_key, parse_snapshot, read_codex_snapshot, Fence};
    use serde_json::json;
    use std::{
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    fn fence() -> Fence {
        Fence {
            epoch: 7,
            holder: "window-1".into(),
            token: "0123456789abcdef0123456789abcdef".into(),
        }
    }

    #[test]
    fn snapshot_prices_day_buckets_and_includes_cached_input_tokens() {
        let fence = fence();
        let snapshot = json!({
            "version": 1,
            "updatedAtMs": 1789776000000_u64,
            "leader": "window-1",
            "claudeAvailable": true,
            "codexAvailable": true,
            "fence": {"epoch": 7, "holder": "window-1", "leaseToken": fence.token},
            "days": {
                "2026-09-18": {"codex/gpt-6-luna": {"input": 1000000, "cachedInput": 500000, "cacheRead": 0, "cacheWrite5m": 0, "cacheWrite1h": 0, "output": 100000}},
                "2026-09-19": {"codex/gpt-6-luna": {"input": 1000000, "cachedInput": 0, "cacheRead": 0, "cacheWrite5m": 0, "cacheWrite1h": 0, "output": 100000}}
            }
        });
        let result = parse_snapshot(&snapshot, &fence, 20260901, 20260919).unwrap();
        assert_eq!(result.month_input_tokens, 2_500_000);
        assert_eq!(result.month_output_tokens, 200_000);
        assert!(result.month_cost_nanos > result.today_cost_nanos);
        assert!(result.today_cost_nanos > 0);
    }

    #[test]
    fn mismatched_fence_bad_usage_and_old_month_are_rejected() {
        let fence = fence();
        let mut raw = json!({
            "version": 1,
            "updatedAtMs": 1789776000000_u64,
            "leader": "window-1",
            "claudeAvailable": true,
            "codexAvailable": true,
            "fence": {"epoch": 7, "holder": "window-1", "leaseToken": fence.token},
            "days": {"2026-09-19": {"codex/gpt-6-luna": {"input": 100, "cachedInput": 0, "cacheRead": 0, "cacheWrite5m": 0, "cacheWrite1h": 0, "output": 10}}}
        });
        raw["fence"]["epoch"] = json!(6);
        assert!(parse_snapshot(&raw, &fence, 20260901, 20260919).is_none());
        raw["fence"]["epoch"] = json!(7);
        raw["days"]["2026-09-19"]["codex/gpt-6-luna"]["input"] = json!(-1);
        assert!(parse_snapshot(&raw, &fence, 20260901, 20260919).is_none());
        raw["days"]["2026-09-19"]["codex/gpt-6-luna"]["input"] = json!(100);
        assert!(parse_snapshot(&raw, &fence, 20261001, 20261019).is_none());
    }

    #[test]
    fn javascript_group_key_is_stable_for_case_insensitive_windows_paths() {
        let left = group_key(
            Path::new(r"C:\Users\Test\.claude"),
            Path::new(r"C:\Users\Test\.codex"),
        );
        let right = group_key(
            Path::new(r"c:\users\test\.claude"),
            Path::new(r"c:\users\test\.codex"),
        );
        assert_eq!(left, right);
        assert_eq!(left.as_deref(), Some("2d0ucitc844"));
    }

    #[test]
    fn reader_uses_only_the_artifact_named_by_the_lock_fence() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let storage = std::env::temp_dir().join(format!(
            "run-dog-otak-snapshot-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&storage).unwrap();
        let claude = Path::new(r"C:\Users\Test\.claude");
        let codex = Path::new(r"C:\Users\Test\.codex");
        let key = group_key(claude, codex).unwrap();
        let fence = fence();
        let lock_path = storage.join(format!("scan-leader-{key}.lock"));
        let artifact_path = storage.join(format!(
            "scan-snapshot-{key}.json.epoch-{}.{}.json",
            fence.epoch, fence.token
        ));
        let lock = json!({
            "version": 2, "epoch": fence.epoch,
            "holder": fence.holder, "leaseToken": fence.token,
            "heartbeatMs": 0
        });
        let snapshot = json!({
            "version": 1, "updatedAtMs": 1789776000000_u64,
            "leader": "window-1", "claudeAvailable": true, "codexAvailable": true,
            "fence": {"epoch": fence.epoch, "holder": fence.holder, "leaseToken": fence.token},
            "days": {"2026-09-19": {"codex/gpt-6-luna": {
                "input": 1000000, "cachedInput": 0, "cacheRead": 0,
                "cacheWrite5m": 0, "cacheWrite1h": 0, "output": 100000
            }}}
        });
        std::fs::write(&lock_path, lock.to_string()).unwrap();
        std::fs::write(&artifact_path, snapshot.to_string()).unwrap();
        assert!(
            read_codex_snapshot(&storage, claude, codex, 20260901, 20260919)
                .unwrap()
                .month_cost_nanos
                > 0
        );
        let replacement = json!({"version": 2, "epoch": 8, "holder": "window-2",
            "leaseToken": "11111111111111111111111111111111", "heartbeatMs": 0});
        std::fs::write(&lock_path, replacement.to_string()).unwrap();
        assert!(read_codex_snapshot(&storage, claude, codex, 20260901, 20260919).is_none());
        std::fs::remove_dir_all(storage).unwrap();
    }

    /// Run manually on a workstation with the otak-usage VS Code extension.
    #[test]
    #[ignore]
    fn live_extension_snapshot_has_a_current_month_codex_total() {
        let profile = PathBuf::from(std::env::var_os("USERPROFILE").unwrap());
        let storage = PathBuf::from(std::env::var_os("APPDATA").unwrap())
            .join("Code/User/globalStorage/odangoo.otak-usage");
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let (year, month, day) =
            crate::core::local_ymd(now_ms, super::super::usage::timezone_bias_minutes());
        let today = crate::core::ymd_key(year, month, day);
        let month_start = crate::core::ymd_key(year, month, 1);
        let result = read_codex_snapshot(
            &storage,
            &profile.join(".claude"),
            &profile.join(".codex"),
            month_start,
            today,
        )
        .expect("matching fenced otak-usage snapshot");
        assert!(result.month_cost_nanos > 0);
        eprintln!(
            "otak Codex: today ${:.2}, month ${:.2}, input {}, output {}",
            result.today_cost_nanos as f64 / 1_000_000_000.0,
            result.month_cost_nanos as f64 / 1_000_000_000.0,
            result.month_input_tokens,
            result.month_output_tokens,
        );
    }
}
