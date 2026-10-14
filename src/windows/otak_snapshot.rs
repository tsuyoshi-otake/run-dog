//! Read-only bridge to otak-usage's fenced, per-day scan snapshot.
//!
//! This reader intentionally does not touch the extension's mutable cache or
//! lease heartbeat. A stopped VS Code window can leave a useful snapshot, but
//! the lock pointer must identify the same immutable artifact before and after
//! the read.
//!
//! Token classes this build does not price are counted, not fatal: a newer
//! extension must not silently freeze the display floor at an old snapshot.

use std::{fs::File, io::Read, path::Path};

use serde_json::Value;

use crate::core::{cost_nanos, local_ymd, ymd_key, TokenUsage};

const MAX_JSON_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOKEN_FIELD: u64 = 1_000_000_000_000;
const MAX_USAGE_FIELDS: usize = 64;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct ProviderSnapshot {
    pub today_cost_nanos: u64,
    pub month_cost_nanos: u64,
    pub month_input_tokens: u64,
    pub month_output_tokens: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct OtakSnapshot {
    pub updated_at_ms: u64,
    pub claude: Option<ProviderSnapshot>,
    pub codex: Option<ProviderSnapshot>,
    /// Usage fields, over every day and model, that this build does not
    /// price. Non-zero means the floor may sit below the extension's display.
    pub unknown_usage_fields: u32,
}

/// Why no snapshot was read. Reported through the usage diagnostics so a
/// stale floor can be told apart from a missing extension.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SnapshotRejection {
    /// The requested month and day are inconsistent.
    Window,
    /// The Claude or Codex directory is not absolute.
    GroupKey,
    /// The extension storage directory is unavailable.
    Storage,
    /// The leader lock is missing or not a version-2 fence.
    Lock,
    /// The fenced artifact is missing, oversized, or not JSON.
    Snapshot,
    /// The artifact's shape or fence does not match.
    Format,
    /// The artifact, or one of its days, lies outside the requested month.
    OutsideMonth,
    /// A model key or token field is malformed or out of range.
    Usage,
    /// Totals exceed `u64`.
    Overflow,
    /// The lock changed while reading.
    FenceChanged,
}

impl SnapshotRejection {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::GroupKey => "group-key",
            Self::Storage => "storage",
            Self::Lock => "lock",
            Self::Snapshot => "snapshot",
            Self::Format => "format",
            Self::OutsideMonth => "outside-month",
            Self::Usage => "usage",
            Self::Overflow => "overflow",
            Self::FenceChanged => "fence-changed",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Fence {
    epoch: u64,
    holder: String,
    token: String,
}

/// `month` is YYYYMM01 and `today` is YYYYMMDD in the machine's local time.
/// A missing, stale-month, changed, or malformed extension snapshot is
/// rejected with the reason; the caller keeps whatever floor it already holds.
pub(super) fn read_usage_snapshot(
    storage_dir: &Path,
    claude_dir: &Path,
    codex_home: &Path,
    month: u32,
    today: u32,
) -> Result<OtakSnapshot, SnapshotRejection> {
    if month % 100 != 1 || today / 100 != month / 100 || today < month {
        return Err(SnapshotRejection::Window);
    }
    let key = group_key(claude_dir, codex_home).ok_or(SnapshotRejection::GroupKey)?;
    let lock_path = storage_dir.join(format!("scan-leader-{key}.lock"));
    let fence = read_lock_fence(&lock_path).ok_or(SnapshotRejection::Lock)?;
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
    let raw = read_bounded_json(&snapshot_path).ok_or(SnapshotRejection::Snapshot)?;
    let result = parse_snapshot(&raw, &fence, month, today)?;
    if read_lock_fence(&lock_path).as_ref() != Some(&fence) {
        return Err(SnapshotRejection::FenceChanged);
    }
    Ok(result)
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

fn parse_snapshot(
    raw: &Value,
    fence: &Fence,
    month: u32,
    today: u32,
) -> Result<OtakSnapshot, SnapshotRejection> {
    use SnapshotRejection::{Format, OutsideMonth, Overflow, Usage};
    let leader = raw.get("leader").and_then(Value::as_str).ok_or(Format)?;
    if raw.get("version").and_then(Value::as_u64) != Some(1)
        || raw.get("fence").and_then(parse_fence).as_ref() != Some(fence)
        || leader.is_empty()
    {
        return Err(Format);
    }
    let claude_available = raw
        .get("claudeAvailable")
        .and_then(Value::as_bool)
        .ok_or(Format)?;
    let codex_available = raw
        .get("codexAvailable")
        .and_then(Value::as_bool)
        .ok_or(Format)?;
    if !claude_available && !codex_available {
        return Err(Format);
    }
    let updated_at_ms = raw
        .get("updatedAtMs")
        .and_then(Value::as_u64)
        .ok_or(Format)?;
    // The extension uses local calendar days. Its snapshot must have been
    // published during the requested month, even when the lease is now stale.
    let (year, month_of_year, day) =
        local_ymd(updated_at_ms, super::usage::timezone_bias_minutes());
    let updated_day = ymd_key(year, month_of_year, day);
    if updated_day / 100 != month / 100 || updated_day > today {
        return Err(OutsideMonth);
    }

    let mut result = OtakSnapshot {
        updated_at_ms,
        claude: claude_available.then(ProviderSnapshot::default),
        codex: codex_available.then(ProviderSnapshot::default),
        unknown_usage_fields: 0,
    };
    let days = raw.get("days").and_then(Value::as_object).ok_or(Format)?;
    if days.is_empty() || days.len() > 31 {
        return Err(Format);
    }
    for (day_key, bucket) in days {
        let day = parse_day(day_key).ok_or(Format)?;
        if day / 100 != month / 100 || day > today {
            return Err(OutsideMonth);
        }
        let models = bucket.as_object().ok_or(Format)?;
        if models.len() > 512 {
            return Err(Format);
        }
        for (key, raw_usage) in models {
            let usage = parse_usage(raw_usage, &mut result.unknown_usage_fields).ok_or(Usage)?;
            let (model, provider) = if let Some(model) = key.strip_prefix("claude/") {
                (model, &mut result.claude)
            } else {
                (key.strip_prefix("codex/").ok_or(Usage)?, &mut result.codex)
            };
            if model.is_empty() || model.len() > 256 {
                return Err(Usage);
            }
            let Some(provider) = provider.as_mut() else {
                continue;
            };
            // otak-usage counts unknown models as $0, but retains their tokens.
            let cost = cost_nanos(model, usage, Some(day_key)).unwrap_or(0);
            provider.month_cost_nanos = provider
                .month_cost_nanos
                .checked_add(cost)
                .ok_or(Overflow)?;
            provider.month_input_tokens = provider
                .month_input_tokens
                .checked_add(usage.processed_input_tokens())
                .ok_or(Overflow)?;
            provider.month_output_tokens = provider
                .month_output_tokens
                .checked_add(usage.output)
                .ok_or(Overflow)?;
            if day == today {
                provider.today_cost_nanos = provider
                    .today_cost_nanos
                    .checked_add(cost)
                    .ok_or(Overflow)?;
            }
        }
    }
    Ok(result)
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

/// Known token classes are validated. Unknown keys are counted into
/// `unknown_fields` rather than rejecting the snapshot: otak-usage adds
/// classification fields (its `longContext*` subsets never remove tokens
/// from the base fields), and an unpriced class only lowers the floor.
fn parse_usage(raw: &Value, unknown_fields: &mut u32) -> Option<TokenUsage> {
    let object = raw.as_object()?;
    if object.len() > MAX_USAGE_FIELDS {
        return None;
    }
    let field = |name: &str, required: bool| -> Option<u64> {
        let n = match object.get(name) {
            Some(value) => value.as_u64()?,
            None if !required => 0,
            None => return None,
        };
        (n <= MAX_TOKEN_FIELD).then_some(n)
    };
    let usage = TokenUsage {
        input: field("input", true)?,
        cached_input: field("cachedInput", true)?,
        cache_read: field("cacheRead", true)?,
        cache_write_5m: field("cacheWrite5m", true)?,
        cache_write_1h: field("cacheWrite1h", true)?,
        output: field("output", true)?,
        long_context_input: field("longContextInput", false)?,
        long_context_cached_input: field("longContextCachedInput", false)?,
        long_context_output: field("longContextOutput", false)?,
        long_context_cache_read: field("longContextCacheRead", false)?,
        long_context_cache_write_5m: field("longContextCacheWrite5m", false)?,
        long_context_cache_write_1h: field("longContextCacheWrite1h", false)?,
    };
    let unknown = object
        .keys()
        .filter(|key| {
            !matches!(
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
                    | "longContextCacheRead"
                    | "longContextCacheWrite5m"
                    | "longContextCacheWrite1h"
            )
        })
        .count();
    *unknown_fields = unknown_fields.saturating_add(u32::try_from(unknown).ok()?);
    Some(usage)
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
    use super::{group_key, parse_snapshot, read_usage_snapshot, Fence, SnapshotRejection};
    use crate::core::{cost_nanos, TokenUsage};
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
    fn snapshot_prices_both_providers_and_includes_cached_input_tokens() {
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
                "2026-09-19": {
                    "codex/gpt-6-luna": {"input": 1000000, "cachedInput": 0, "cacheRead": 0, "cacheWrite5m": 0, "cacheWrite1h": 0, "output": 100000},
                    "claude/claude-sonnet-5-5": {"input": 100, "cachedInput": 0, "cacheRead": 1000000, "cacheWrite5m": 0, "cacheWrite1h": 0, "output": 100000}
                }
            }
        });
        let result = parse_snapshot(&snapshot, &fence, 20260901, 20260919).unwrap();
        assert_eq!(result.updated_at_ms, 1_789_776_000_000);
        assert_eq!(result.unknown_usage_fields, 0);
        let codex = result.codex.unwrap();
        let claude = result.claude.unwrap();
        assert_eq!(codex.month_input_tokens, 2_500_000);
        assert_eq!(codex.month_output_tokens, 200_000);
        assert!(codex.month_cost_nanos > codex.today_cost_nanos);
        assert!(codex.today_cost_nanos > 0);
        assert_eq!(claude.month_input_tokens, 1_000_100);
        assert_eq!(claude.month_output_tokens, 100_000);
        assert_eq!(claude.month_cost_nanos, claude.today_cost_nanos);
        assert!(claude.today_cost_nanos > 0);
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
        assert_eq!(
            parse_snapshot(&raw, &fence, 20260901, 20260919).unwrap_err(),
            SnapshotRejection::Format
        );
        raw["fence"]["epoch"] = json!(7);
        raw["days"]["2026-09-19"]["codex/gpt-6-luna"]["input"] = json!(-1);
        assert_eq!(
            parse_snapshot(&raw, &fence, 20260901, 20260919).unwrap_err(),
            SnapshotRejection::Usage
        );
        raw["days"]["2026-09-19"]["codex/gpt-6-luna"]["input"] = json!(100);
        assert_eq!(
            parse_snapshot(&raw, &fence, 20261001, 20261019).unwrap_err(),
            SnapshotRejection::OutsideMonth
        );

        raw["claudeAvailable"] = json!(false);
        let codex_only = parse_snapshot(&raw, &fence, 20260901, 20260919).unwrap();
        assert!(codex_only.claude.is_none());
        assert!(codex_only.codex.is_some());
        raw["claudeAvailable"] = json!(true);
        raw["codexAvailable"] = json!(false);
        let claude_only = parse_snapshot(&raw, &fence, 20260901, 20260919).unwrap();
        assert!(claude_only.claude.is_some());
        assert!(claude_only.codex.is_none());
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
        let temporary_root = PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join("tmp");
        std::fs::create_dir_all(&temporary_root).unwrap();
        let storage = temporary_root.join(format!(
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
            read_usage_snapshot(&storage, claude, codex, 20260901, 20260919)
                .unwrap()
                .codex
                .unwrap()
                .month_cost_nanos
                > 0
        );
        let replacement = json!({"version": 2, "epoch": 8, "holder": "window-2",
            "leaseToken": "11111111111111111111111111111111", "heartbeatMs": 0});
        std::fs::write(&lock_path, replacement.to_string()).unwrap();
        assert_eq!(
            read_usage_snapshot(&storage, claude, codex, 20260901, 20260919).unwrap_err(),
            SnapshotRejection::Snapshot
        );
        std::fs::remove_dir_all(storage).unwrap();
    }

    #[test]
    fn long_context_cache_subsets_are_priced_and_unknown_fields_are_counted() {
        let fence = fence();
        // otak-usage 1.26.16 classifies the over-threshold part of each cache
        // class as a `longContext*` subset; the base fields keep every token.
        let mut raw = json!({
            "version": 1,
            "updatedAtMs": 1789776000000_u64,
            "leader": "window-1",
            "claudeAvailable": true,
            "codexAvailable": false,
            "fence": {"epoch": 7, "holder": "window-1", "leaseToken": fence.token},
            "days": {"2026-09-19": {"claude/claude-haiku-5-5": {
                "input": 50000, "cachedInput": 0, "cacheRead": 60000,
                "cacheWrite5m": 10000, "cacheWrite1h": 2000, "output": 5000,
                "longContextInput": 50000, "longContextCachedInput": 0,
                "longContextOutput": 5000, "longContextCacheRead": 60000,
                "longContextCacheWrite5m": 10000, "longContextCacheWrite1h": 2000
            }}}
        });
        let priced = parse_snapshot(&raw, &fence, 20260901, 20260919).unwrap();
        let claude = priced.claude.unwrap();
        assert_eq!(priced.unknown_usage_fields, 0);
        assert_eq!(claude.month_input_tokens, 122_000);
        assert_eq!(claude.month_output_tokens, 5_000);
        // The local reader prices the same request through the SKU.
        let measured = TokenUsage {
            input: 50_000,
            cache_read: 60_000,
            cache_write_5m: 10_000,
            cache_write_1h: 2_000,
            output: 5_000,
            ..TokenUsage::default()
        };
        assert_eq!(
            claude.today_cost_nanos,
            cost_nanos("claude-haiku-5-5-long-context", measured, None).unwrap()
        );

        raw["days"]["2026-09-19"]["claude/claude-haiku-5-5"]["cacheWrite24h"] = json!(1);
        let tolerated = parse_snapshot(&raw, &fence, 20260901, 20260919).unwrap();
        assert_eq!(tolerated.unknown_usage_fields, 1);
        assert_eq!(tolerated.claude, priced.claude);

        raw["days"]["2026-09-19"]["claude/claude-haiku-5-5"]["cacheRead"] = json!("60000");
        assert_eq!(
            parse_snapshot(&raw, &fence, 20260901, 20260919).unwrap_err(),
            SnapshotRejection::Usage
        );
    }

    /// Run manually on a workstation with the otak-usage VS Code extension.
    #[test]
    #[ignore]
    fn live_extension_snapshot_has_current_month_provider_totals() {
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
        let result = read_usage_snapshot(
            &storage,
            &profile.join(".claude"),
            &profile.join(".codex"),
            month_start,
            today,
        )
        .expect("matching fenced otak-usage snapshot");
        assert!(result.claude.is_some() || result.codex.is_some());
        for (name, provider) in [("Claude", result.claude), ("Codex", result.codex)] {
            if let Some(provider) = provider {
                eprintln!(
                    "otak {name}: today ${:.2}, month ${:.2}",
                    provider.today_cost_nanos as f64 / 1_000_000_000.0,
                    provider.month_cost_nanos as f64 / 1_000_000_000.0,
                );
            }
        }
    }
}
