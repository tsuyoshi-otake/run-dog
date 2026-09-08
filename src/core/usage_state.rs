//! vNext usage durable state. Aggregate and cursors share one generation.
//!
//! This is not the Registry `UsageCheckpoint` text. Extra positional columns
//! from experimental reader/Codex patches are rejected, not merged in.

use std::collections::HashSet;

use super::{
    display_cents, hex_decode, retain_keys_for_month, ClaudeDedupeKey, CodexTokenTotals,
    FileCheckpointKey, ProviderUsage, UsageCheckpoint, UsageSnapshot, NANOS_PER_CENT,
};

const USAGE_STATE_HEADER_V1: &str = "rundog-usage-state-1";
pub const USAGE_STATE_HEADER: &str = "rundog-usage-state-2";
// Version 3 distinguishes transient missing identities from the version 2
// migration that rebuilt aggregates. The serialized field layout is unchanged.
pub const USAGE_STATE_SCHEMA_VERSION: u32 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorKind {
    Claude,
    Codex,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsageCursor {
    pub kind: CursorKind,
    pub logical_id: String,
    pub offset: u64,
    pub size: u64,
    /// Windows volume serial and file index (high, low). Missing legacy IDs
    /// require a fresh aggregate scan rather than trusting an old offset.
    pub file_id: Option<[u32; 3]>,
    /// First-64-byte fingerprint. A hint, not a complete in-place rewrite detector.
    pub prefix: Option<u64>,
    /// Last Codex `turn_context` model. Named line, not a `file=` column.
    pub last_model: Option<String>,
    /// Last seen Codex `total_token_usage`. Named line, not a `file=` column.
    pub last_codex_total: Option<CodexTokenTotals>,
}

/// Why a cursor was rebuilt instead of appended. No paths or payloads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorRebuildReason {
    SizeShrunk,
    PrefixChanged,
    FileIdAndPrefixChanged,
    SameSizeRewriteHint,
    FileIdChanged,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UsageAggregate {
    pub month_start: u32,
    pub today: u32,
    pub last_collected_ms: u64,
    pub catch_up_done: bool,
    pub snapshot: UsageSnapshot,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UsageState {
    pub generation: u64,
    pub schema_version: u32,
    pub aggregate: UsageAggregate,
    pub cursors: Vec<UsageCursor>,
    pub claude_keys: HashSet<ClaudeDedupeKey>,
}

impl UsageState {
    #[must_use]
    pub fn from_registry_checkpoint(checkpoint: &UsageCheckpoint, generation: u64) -> Self {
        let mut cursors: Vec<UsageCursor> = checkpoint
            .files
            .iter()
            .map(|(key, cursor)| {
                let (kind, logical_id) = match key {
                    FileCheckpointKey::Claude(id) => (CursorKind::Claude, id.clone()),
                    FileCheckpointKey::Codex(id) => (CursorKind::Codex, id.clone()),
                };
                UsageCursor {
                    kind,
                    logical_id,
                    offset: cursor.offset,
                    size: cursor.size,
                    file_id: None,
                    prefix: None,
                    last_model: None,
                    last_codex_total: None,
                }
            })
            .collect();
        cursors.sort_by(|left, right| cursor_sort_key(left).cmp(&cursor_sort_key(right)));
        Self {
            generation,
            // Registry checkpoints predate durable identities and still need
            // the legacy aggregate/cursor migration before the next save.
            schema_version: 2,
            aggregate: UsageAggregate {
                month_start: checkpoint.month_start,
                today: checkpoint.today,
                last_collected_ms: checkpoint.last_collected_ms,
                catch_up_done: checkpoint.catch_up_done,
                snapshot: UsageSnapshot {
                    claude: checkpoint.snapshot.claude,
                    codex: checkpoint.snapshot.codex,
                    month_scan_in_progress: false,
                },
            },
            cursors,
            claude_keys: HashSet::new(),
        }
    }

    #[must_use]
    pub fn encode(&self) -> String {
        let mut cursors = self.cursors.clone();
        cursors.sort_by(|left, right| cursor_sort_key(left).cmp(&cursor_sort_key(right)));
        let mut out = format!(
            concat!(
                "{header}\n",
                "schema={schema}\n",
                "generation={generation}\n",
                "month={month}\n",
                "day={day}\n",
                "last_ms={last_ms}\n",
                "catch_up_done={catch_up}\n",
                "claude_today={claude_today}\n",
                "claude_month={claude_month}\n",
                "claude_today_nanos={claude_today_nanos}\n",
                "claude_month_nanos={claude_month_nanos}\n",
                "claude_in={claude_in}\n",
                "claude_out={claude_out}\n",
                "codex_today={codex_today}\n",
                "codex_month={codex_month}\n",
                "codex_today_nanos={codex_today_nanos}\n",
                "codex_month_nanos={codex_month_nanos}\n",
                "codex_in={codex_in}\n",
                "codex_out={codex_out}\n",
            ),
            header = USAGE_STATE_HEADER,
            schema = self.schema_version,
            generation = self.generation,
            month = self.aggregate.month_start,
            day = self.aggregate.today,
            last_ms = self.aggregate.last_collected_ms,
            catch_up = u32::from(self.aggregate.catch_up_done),
            claude_today = self.aggregate.snapshot.claude.today_cents,
            claude_month = self.aggregate.snapshot.claude.month_cents,
            claude_today_nanos = persisted_nanos(
                self.aggregate.snapshot.claude.today_cents,
                self.aggregate.snapshot.claude.today_cost_nanos,
            ),
            claude_month_nanos = persisted_nanos(
                self.aggregate.snapshot.claude.month_cents,
                self.aggregate.snapshot.claude.month_cost_nanos,
            ),
            claude_in = self.aggregate.snapshot.claude.month_input_tokens,
            claude_out = self.aggregate.snapshot.claude.month_output_tokens,
            codex_today = self.aggregate.snapshot.codex.today_cents,
            codex_month = self.aggregate.snapshot.codex.month_cents,
            codex_today_nanos = persisted_nanos(
                self.aggregate.snapshot.codex.today_cents,
                self.aggregate.snapshot.codex.today_cost_nanos,
            ),
            codex_month_nanos = persisted_nanos(
                self.aggregate.snapshot.codex.month_cents,
                self.aggregate.snapshot.codex.month_cost_nanos,
            ),
            codex_in = self.aggregate.snapshot.codex.month_input_tokens,
            codex_out = self.aggregate.snapshot.codex.month_output_tokens,
        );
        for cursor in cursors {
            let kind = match cursor.kind {
                CursorKind::Claude => 'c',
                CursorKind::Codex => 'x',
            };
            out.push_str(&format!(
                "cursor={kind}\t{}\t{}\t{}\n",
                cursor.logical_id, cursor.offset, cursor.size
            ));
            if let Some(prefix) = cursor.prefix {
                out.push_str(&format!("prefix={kind}\t{}\t{prefix}\n", cursor.logical_id));
            }
            if let Some([volume, high, low]) = cursor.file_id {
                out.push_str(&format!(
                    "file_id={kind}\t{}\t{volume}\t{high}\t{low}\n",
                    cursor.logical_id
                ));
            }
            if cursor.kind == CursorKind::Codex {
                if let Some(model) = cursor.last_model.as_deref() {
                    if is_safe_codex_model(model) {
                        out.push_str(&format!(
                            "codex_model={kind}\t{}\t{model}\n",
                            cursor.logical_id
                        ));
                    }
                }
                if let Some(total) = cursor.last_codex_total {
                    out.push_str(&format!(
                        "codex_total={kind}\t{}\t{}\t{}\t{}\n",
                        cursor.logical_id, total.input, total.cached, total.output
                    ));
                }
            }
        }
        let mut keys: Vec<ClaudeDedupeKey> = self.claude_keys.iter().copied().collect();
        keys.sort_by(|left, right| {
            left.digest
                .cmp(&right.digest)
                .then(left.month.cmp(&right.month))
        });
        for key in keys {
            out.push_str(&format!("dkey={}\t{}\n", key.encode_hex(), key.month));
        }
        out
    }

    #[must_use]
    pub fn decode(payload: &str) -> Option<Self> {
        let mut lines = payload.lines();
        match lines.next()? {
            USAGE_STATE_HEADER => {}
            USAGE_STATE_HEADER_V1 => return None,
            _ => return None,
        }
        let mut schema_version = None;
        let mut generation = None;
        let mut month_start = None;
        let mut today = None;
        let mut last_collected_ms = None;
        let mut catch_up_done = None;
        let mut claude_today = 0_u32;
        let mut claude_month = 0_u32;
        let mut claude_today_nanos = None;
        let mut claude_month_nanos = None;
        let mut claude_in = 0_u64;
        let mut claude_out = 0_u64;
        let mut codex_today = 0_u32;
        let mut codex_month = 0_u32;
        let mut codex_today_nanos = None;
        let mut codex_month_nanos = None;
        let mut codex_in = 0_u64;
        let mut codex_out = 0_u64;
        let mut cursors = Vec::new();
        let mut prefixes: Vec<(CursorKind, String, u64)> = Vec::new();
        let mut file_ids = Vec::new();
        let mut models: Vec<(CursorKind, String, String)> = Vec::new();
        let mut totals: Vec<(CursorKind, String, CodexTokenTotals)> = Vec::new();
        let mut claude_keys = HashSet::new();
        for line in lines {
            if line.is_empty() {
                continue;
            }
            if let Some(value) = line.strip_prefix("schema=") {
                schema_version = Some(value.parse().ok()?);
            } else if let Some(value) = line.strip_prefix("generation=") {
                generation = Some(value.parse().ok()?);
            } else if let Some(value) = line.strip_prefix("month=") {
                month_start = Some(value.parse().ok()?);
            } else if let Some(value) = line.strip_prefix("day=") {
                today = Some(value.parse().ok()?);
            } else if let Some(value) = line.strip_prefix("last_ms=") {
                last_collected_ms = Some(value.parse().ok()?);
            } else if let Some(value) = line.strip_prefix("catch_up_done=") {
                catch_up_done = Some(value.parse::<u32>().ok()? != 0);
            } else if let Some(value) = line.strip_prefix("claude_today=") {
                claude_today = value.parse().ok()?;
            } else if let Some(value) = line.strip_prefix("claude_month=") {
                claude_month = value.parse().ok()?;
            } else if let Some(value) = line.strip_prefix("claude_today_nanos=") {
                claude_today_nanos = Some(value.parse().ok()?);
            } else if let Some(value) = line.strip_prefix("claude_month_nanos=") {
                claude_month_nanos = Some(value.parse().ok()?);
            } else if let Some(value) = line.strip_prefix("claude_in=") {
                claude_in = value.parse().ok()?;
            } else if let Some(value) = line.strip_prefix("claude_out=") {
                claude_out = value.parse().ok()?;
            } else if let Some(value) = line.strip_prefix("codex_today=") {
                codex_today = value.parse().ok()?;
            } else if let Some(value) = line.strip_prefix("codex_month=") {
                codex_month = value.parse().ok()?;
            } else if let Some(value) = line.strip_prefix("codex_today_nanos=") {
                codex_today_nanos = Some(value.parse().ok()?);
            } else if let Some(value) = line.strip_prefix("codex_month_nanos=") {
                codex_month_nanos = Some(value.parse().ok()?);
            } else if let Some(value) = line.strip_prefix("codex_in=") {
                codex_in = value.parse().ok()?;
            } else if let Some(value) = line.strip_prefix("codex_out=") {
                codex_out = value.parse().ok()?;
            } else if let Some(value) = line.strip_prefix("cursor=") {
                cursors.push(parse_cursor_line(value)?);
            } else if let Some(value) = line.strip_prefix("prefix=") {
                prefixes.push(parse_prefix_line(value)?);
            } else if let Some(value) = line.strip_prefix("file_id=") {
                let parts: Vec<_> = value.split('\t').collect();
                if parts.len() != 5 {
                    return None;
                }
                let kind = match parts[0] {
                    "c" => CursorKind::Claude,
                    "x" => CursorKind::Codex,
                    _ => return None,
                };
                file_ids.push((
                    kind,
                    parts[1].to_owned(),
                    [
                        parts[2].parse().ok()?,
                        parts[3].parse().ok()?,
                        parts[4].parse().ok()?,
                    ],
                ));
            } else if let Some(value) = line.strip_prefix("codex_model=") {
                models.push(parse_codex_model_line(value)?);
            } else if let Some(value) = line.strip_prefix("codex_total=") {
                totals.push(parse_codex_total_line(value)?);
            } else if let Some(value) = line.strip_prefix("dkey=") {
                claude_keys.insert(parse_dkey_line(value)?);
            } else if line.starts_with("ckey=") {
                // Legacy DefaultHasher u64. Not a disk contract; ignore.
            } else if line.starts_with("file=") {
                return None;
            }
        }
        for (kind, logical_id, prefix) in prefixes {
            if let Some(cursor) = cursors
                .iter_mut()
                .find(|cursor| cursor.kind == kind && cursor.logical_id == logical_id)
            {
                cursor.prefix = Some(prefix);
            }
        }
        for (kind, logical_id, file_id) in file_ids {
            let cursor = cursors
                .iter_mut()
                .find(|cursor| cursor.kind == kind && cursor.logical_id == logical_id)?;
            if cursor.file_id.replace(file_id).is_some() {
                return None;
            }
        }
        for (kind, logical_id, model) in models {
            if let Some(cursor) = cursors
                .iter_mut()
                .find(|cursor| cursor.kind == kind && cursor.logical_id == logical_id)
            {
                cursor.last_model = Some(model);
            }
        }
        for (kind, logical_id, total) in totals {
            if let Some(cursor) = cursors
                .iter_mut()
                .find(|cursor| cursor.kind == kind && cursor.logical_id == logical_id)
            {
                cursor.last_codex_total = Some(total);
            }
        }
        let schema_version = schema_version?;
        if !(2..=USAGE_STATE_SCHEMA_VERSION).contains(&schema_version) {
            return None;
        }
        Some(Self {
            generation: generation?,
            schema_version,
            aggregate: UsageAggregate {
                month_start: month_start?,
                today: today?,
                last_collected_ms: last_collected_ms?,
                catch_up_done: catch_up_done?,
                snapshot: UsageSnapshot {
                    claude: restore_provider(
                        claude_today,
                        claude_month,
                        claude_today_nanos?,
                        claude_month_nanos?,
                        claude_in,
                        claude_out,
                    )?,
                    codex: restore_provider(
                        codex_today,
                        codex_month,
                        codex_today_nanos?,
                        codex_month_nanos?,
                        codex_in,
                        codex_out,
                    )?,
                    month_scan_in_progress: false,
                },
            },
            cursors,
            claude_keys,
        })
    }

    pub fn prune_claude_keys_before_month(&mut self, month_start: u32) {
        self.claude_keys = retain_keys_for_month(&self.claude_keys, month_start);
    }
}

fn persisted_nanos(cents: u32, nanos: u64) -> u64 {
    if nanos == 0 && cents > 0 {
        u64::from(cents).saturating_mul(NANOS_PER_CENT)
    } else {
        nanos
    }
}

fn restore_provider(
    today_cents: u32,
    month_cents: u32,
    today_cost_nanos: u64,
    month_cost_nanos: u64,
    month_input_tokens: u64,
    month_output_tokens: u64,
) -> Option<ProviderUsage> {
    if display_cents(today_cost_nanos) != today_cents
        || display_cents(month_cost_nanos) != month_cents
    {
        return None;
    }
    Some(ProviderUsage {
        today_cents,
        month_cents,
        today_cost_nanos,
        month_cost_nanos,
        month_input_tokens,
        month_output_tokens,
        ..ProviderUsage::default()
    })
}

fn cursor_sort_key(cursor: &UsageCursor) -> (u8, &str) {
    let kind = match cursor.kind {
        CursorKind::Claude => 0,
        CursorKind::Codex => 1,
    };
    (kind, cursor.logical_id.as_str())
}

fn parse_cursor_line(value: &str) -> Option<UsageCursor> {
    let parts: Vec<&str> = value.split('\t').collect();
    if parts.len() != 4 {
        return None;
    }
    let kind = match parts[0] {
        "c" => CursorKind::Claude,
        "x" => CursorKind::Codex,
        _ => return None,
    };
    if parts[1].is_empty() {
        return None;
    }
    Some(UsageCursor {
        kind,
        logical_id: parts[1].to_owned(),
        offset: parts[2].parse().ok()?,
        size: parts[3].parse().ok()?,
        file_id: None,
        prefix: None,
        last_model: None,
        last_codex_total: None,
    })
}

fn parse_prefix_line(value: &str) -> Option<(CursorKind, String, u64)> {
    let parts: Vec<&str> = value.split('\t').collect();
    if parts.len() != 3 {
        return None;
    }
    let kind = match parts[0] {
        "c" => CursorKind::Claude,
        "x" => CursorKind::Codex,
        _ => return None,
    };
    Some((kind, parts[1].to_owned(), parts[2].parse().ok()?))
}

fn parse_codex_model_line(value: &str) -> Option<(CursorKind, String, String)> {
    let parts: Vec<&str> = value.split('\t').collect();
    if parts.len() != 3 {
        return None;
    }
    let kind = match parts[0] {
        "x" => CursorKind::Codex,
        _ => return None,
    };
    if !is_safe_codex_model(parts[2]) {
        return None;
    }
    Some((kind, parts[1].to_owned(), parts[2].to_owned()))
}

fn parse_codex_total_line(value: &str) -> Option<(CursorKind, String, CodexTokenTotals)> {
    let parts: Vec<&str> = value.split('\t').collect();
    if parts.len() != 5 {
        return None;
    }
    let kind = match parts[0] {
        "x" => CursorKind::Codex,
        _ => return None,
    };
    Some((
        kind,
        parts[1].to_owned(),
        CodexTokenTotals {
            input: parts[2].parse().ok()?,
            cached: parts[3].parse().ok()?,
            output: parts[4].parse().ok()?,
        },
    ))
}

fn parse_dkey_line(value: &str) -> Option<ClaudeDedupeKey> {
    let parts: Vec<&str> = value.split('\t').collect();
    if parts.len() != 2 {
        return None;
    }
    Some(ClaudeDedupeKey {
        digest: hex_decode(parts[0])?,
        month: parts[1].parse().ok()?,
    })
}

fn is_safe_codex_model(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 64
        && model
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
}

/// True when a candidate store root would write into provider user data.
#[must_use]
pub fn usage_store_root_is_forbidden(
    root: &std::path::Path,
    forbidden_roots: &[&std::path::Path],
) -> bool {
    forbidden_roots
        .iter()
        .any(|denied| root == *denied || root.starts_with(denied))
}

#[cfg(test)]
mod tests {
    use super::{
        usage_store_root_is_forbidden, CursorKind, UsageCursor, UsageState, USAGE_STATE_HEADER,
        USAGE_STATE_HEADER_V1,
    };
    use crate::core::{
        ClaudeDedupeKey, CodexTokenTotals, FileCheckpointCursor, FileCheckpointKey, ProviderUsage,
        UsageCheckpoint, UsageSnapshot,
    };
    use std::path::Path;

    fn sample_state(catch_up_done: bool) -> UsageState {
        UsageState {
            generation: 3,
            schema_version: crate::core::USAGE_STATE_SCHEMA_VERSION,
            aggregate: super::UsageAggregate {
                month_start: 20_260_901,
                today: 20_260_905,
                last_collected_ms: 10,
                catch_up_done,
                snapshot: UsageSnapshot {
                    claude: ProviderUsage {
                        today_cents: 3,
                        month_cents: 12,
                        today_cost_nanos: 3 * crate::core::NANOS_PER_CENT,
                        month_cost_nanos: 12 * crate::core::NANOS_PER_CENT,
                        month_input_tokens: 100,
                        month_output_tokens: 40,
                        ..ProviderUsage::default()
                    },
                    codex: ProviderUsage {
                        today_cents: 5,
                        month_cents: 9,
                        today_cost_nanos: 5 * crate::core::NANOS_PER_CENT,
                        month_cost_nanos: 9 * crate::core::NANOS_PER_CENT,
                        month_input_tokens: 80,
                        month_output_tokens: 20,
                        ..ProviderUsage::default()
                    },
                    ..UsageSnapshot::default()
                },
            },
            cursors: vec![UsageCursor {
                kind: CursorKind::Claude,
                logical_id: "projects/p/session.jsonl".to_owned(),
                offset: 8,
                size: 16,
                file_id: None,
                prefix: Some(9),
                last_model: None,
                last_codex_total: None,
            }],
            claude_keys: [ClaudeDedupeKey::new("m", "r", 20_260_901)]
                .into_iter()
                .collect(),
        }
    }

    fn sample_codex_state() -> UsageState {
        let mut state = sample_state(true);
        state.cursors.push(UsageCursor {
            kind: CursorKind::Codex,
            logical_id: "sessions/2026/09/a.jsonl".to_owned(),
            offset: 4,
            size: 8,
            file_id: None,
            prefix: None,
            last_model: Some("gpt-5.4".to_owned()),
            last_codex_total: Some(CodexTokenTotals {
                input: 80,
                cached: 20,
                output: 5,
            }),
        });
        state
    }

    #[test]
    fn component_encode_orders_claude_before_codex_then_logical_id() {
        let mut state = sample_state(true);
        state.cursors = vec![
            UsageCursor {
                kind: CursorKind::Codex,
                logical_id: "sessions/z.jsonl".to_owned(),
                offset: 1,
                size: 1,
                file_id: None,
                prefix: None,
                last_model: None,
                last_codex_total: None,
            },
            UsageCursor {
                kind: CursorKind::Claude,
                logical_id: "projects/z.jsonl".to_owned(),
                offset: 2,
                size: 2,
                file_id: None,
                prefix: None,
                last_model: None,
                last_codex_total: None,
            },
            UsageCursor {
                kind: CursorKind::Claude,
                logical_id: "projects/a.jsonl".to_owned(),
                offset: 3,
                size: 3,
                file_id: None,
                prefix: None,
                last_model: None,
                last_codex_total: None,
            },
        ];
        let encoded = state.encode();
        let claude_a = encoded
            .find("cursor=c\tprojects/a.jsonl\t")
            .expect("claude a");
        let claude_z = encoded
            .find("cursor=c\tprojects/z.jsonl\t")
            .expect("claude z");
        let codex_z = encoded
            .find("cursor=x\tsessions/z.jsonl\t")
            .expect("codex z");
        assert!(
            claude_a < claude_z && claude_z < codex_z,
            "canonical cursor order is Claude then Codex, each by logical_id"
        );
    }

    #[test]
    fn component_unsafe_codex_model_is_not_persisted() {
        let mut state = sample_codex_state();
        state.cursors[1].last_model = Some("bad model".to_owned());
        let encoded = state.encode();
        assert!(!encoded.contains("codex_model="));
        state.cursors[1].last_model = Some(String::new());
        assert!(!state.encode().contains("codex_model="));
        state.cursors[1].last_model = Some("x".repeat(65));
        assert!(!state.encode().contains("codex_model="));
    }

    #[test]
    fn component_usage_state_named_codex_continuation_is_not_a_file_column() {
        let encoded = sample_codex_state().encode();
        assert!(encoded.contains("codex_model=x\tsessions/2026/09/a.jsonl\tgpt-5.4\n"));
        assert!(encoded.contains("codex_total=x\tsessions/2026/09/a.jsonl\t80\t20\t5\n"));
        assert!(encoded.contains("cursor=x\tsessions/2026/09/a.jsonl\t4\t8\n"));
        assert!(!encoded.contains("file="));
        let decoded = UsageState::decode(&encoded).expect("state");
        assert_eq!(decoded.cursors[1].last_model.as_deref(), Some("gpt-5.4"));
        assert_eq!(
            decoded.cursors[1].last_codex_total,
            Some(CodexTokenTotals {
                input: 80,
                cached: 20,
                output: 5
            })
        );
    }

    #[test]
    fn component_usage_state_rejects_positional_codex_total_on_cursor() {
        let mut payload = sample_codex_state().encode();
        payload = payload.replace(
            "cursor=x\tsessions/2026/09/a.jsonl\t4\t8\n",
            "cursor=x\tsessions/2026/09/a.jsonl\t4\t8\t80\t20\t5\tgpt-5.4\n",
        );
        assert!(UsageState::decode(&payload).is_none());
    }

    #[test]
    fn component_usage_state_named_prefix_is_not_a_positional_file_column() {
        let encoded = sample_state(true).encode();
        let key = ClaudeDedupeKey::new("m", "r", 20_260_901);
        assert!(encoded.contains("prefix=c\tprojects/p/session.jsonl\t9\n"));
        assert!(encoded.contains("cursor=c\tprojects/p/session.jsonl\t8\t16\n"));
        assert!(encoded.contains(&format!("dkey={}\t20260901\n", key.encode_hex())));
        assert!(!encoded.contains("ckey="));
        assert!(!encoded.contains("file="));
        assert!(!encoded.contains("requestId"));
        assert!(!encoded.contains("\tm\t"));
    }

    #[test]
    fn component_usage_state_ignores_legacy_defaulthasher_ckey() {
        let mut payload = sample_state(true).encode();
        payload.push_str("ckey=7\n");
        let decoded = UsageState::decode(&payload).expect("legacy ckey ignored");
        assert!(!decoded
            .claude_keys
            .iter()
            .any(|key| key.encode_hex() == "0000000000000007"));
    }

    #[test]
    fn component_usage_state_rejects_positional_extra_dkey_columns() {
        let mut payload = sample_state(true).encode();
        let key = ClaudeDedupeKey::new("m", "r", 20_260_901);
        payload = payload.replace(
            &format!("dkey={}\t20260901\n", key.encode_hex()),
            &format!("dkey={}\t20260901\tm1\tr1\n", key.encode_hex()),
        );
        assert!(UsageState::decode(&payload).is_none());
    }

    #[test]
    fn component_usage_state_month_rollover_keeps_current_month_dedupe() {
        let mut state = sample_state(true);
        state
            .claude_keys
            .insert(ClaudeDedupeKey::new("old", "old", 20_260_801));
        state.prune_claude_keys_before_month(20_260_901);
        assert_eq!(state.claude_keys.len(), 1);
        assert!(state
            .claude_keys
            .contains(&ClaudeDedupeKey::new("m", "r", 20_260_901)));
    }

    #[test]
    fn component_usage_state_round_trips_catch_up_in_progress() {
        let state = sample_state(false);
        let decoded = UsageState::decode(&state.encode()).expect("state");
        assert_eq!(decoded, state);
        assert!(!decoded.aggregate.catch_up_done);
    }

    #[test]
    fn component_usage_state_preserves_subcent_cost_and_rejects_cent_only_schema() {
        let mut state = sample_state(true);
        state.aggregate.snapshot.codex.clear_today_cost();
        state.aggregate.snapshot.codex.clear_month_cost();
        state.aggregate.snapshot.codex.add_today_nanos(25_000_000);
        state.aggregate.snapshot.codex.add_month_nanos(25_000_000);
        let encoded = state.encode();
        let decoded = UsageState::decode(&encoded).expect("precise state");
        assert_eq!(decoded.aggregate.snapshot.codex.today_cents, 3);
        assert_eq!(
            decoded.aggregate.snapshot.codex.today_cost_nanos,
            25_000_000
        );
        assert_eq!(decoded, state);

        let legacy = encoded
            .replacen(USAGE_STATE_HEADER, USAGE_STATE_HEADER_V1, 1)
            .replacen("schema=3", "schema=1", 1);
        assert!(UsageState::decode(&legacy).is_none());
    }

    #[test]
    fn component_usage_state_rejects_positional_extra_cursor_columns() {
        let mut payload = sample_state(true).encode();
        payload = payload.replace(
            "cursor=c\tprojects/p/session.jsonl\t8\t16\n",
            "cursor=c\tprojects/p/session.jsonl\t8\t16\t80\topus\n",
        );
        assert!(UsageState::decode(&payload).is_none());
    }

    #[test]
    fn component_usage_state_rejects_legacy_file_lines() {
        let mut payload = sample_state(true).encode();
        payload.push_str("file=c\tprojects/p/session.jsonl\t8\t16\n");
        assert!(UsageState::decode(&payload).is_none());
    }

    #[test]
    fn component_usage_state_rejects_wrong_header() {
        let payload = sample_state(true).encode().replacen(
            USAGE_STATE_HEADER,
            "rundog-usage-checkpoint-3",
            1,
        );
        assert!(UsageState::decode(&payload).is_none());
    }

    #[test]
    fn component_month_rollover_keeps_cursors_in_schema() {
        let mut state = sample_state(true);
        state.aggregate.month_start = 20_261_001;
        state.aggregate.snapshot.claude.clear_month_cost();
        state.aggregate.snapshot.claude.month_input_tokens = 0;
        let decoded = UsageState::decode(&state.encode()).expect("state");
        assert_eq!(decoded.aggregate.month_start, 20_261_001);
        assert_eq!(decoded.cursors.len(), 1);
        assert_eq!(decoded.cursors[0].offset, 8);
    }

    #[test]
    fn component_registry_checkpoint_migrates_without_extra_columns() {
        let checkpoint = UsageCheckpoint {
            month_start: 20_260_801,
            today: 20_260_823,
            last_collected_ms: 1,
            catch_up_done: true,
            snapshot: UsageSnapshot {
                claude: ProviderUsage {
                    month_cents: 9,
                    month_cost_nanos: 9 * crate::core::NANOS_PER_CENT,
                    ..ProviderUsage::default()
                },
                ..UsageSnapshot::default()
            },
            files: [(
                FileCheckpointKey::Codex("sessions/2026/08/a.jsonl".to_owned()),
                FileCheckpointCursor { offset: 4, size: 4 },
            )]
            .into(),
        };
        let state = UsageState::from_registry_checkpoint(&checkpoint, 1);
        let encoded = state.encode();
        assert!(!encoded.contains("file="));
        assert!(encoded.contains("cursor=x\tsessions/2026/08/a.jsonl\t4\t4\n"));
        assert_eq!(
            UsageState::decode(&encoded).expect("migrated").cursors[0].offset,
            4
        );
    }

    #[test]
    fn component_usage_store_root_must_not_be_under_provider_dirs() {
        let claude = Path::new(r"C:\Users\me\.claude");
        let codex = Path::new(r"C:\Users\me\.codex");
        assert!(usage_store_root_is_forbidden(
            Path::new(r"C:\Users\me\.claude\usage"),
            &[claude, codex]
        ));
        assert!(usage_store_root_is_forbidden(codex, &[claude, codex]));
        assert!(!usage_store_root_is_forbidden(
            Path::new(r"C:\Users\me\AppData\Local\RunDog\usage"),
            &[claude, codex]
        ));
    }
}
