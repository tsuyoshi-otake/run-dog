//! vNext usage durable state. Aggregate and cursors share one generation.
//!
//! This is not the Registry `UsageCheckpoint` text. Extra positional columns
//! from experimental reader/Codex patches are rejected, not merged in.

use std::collections::HashSet;

use super::{CodexTokenTotals, FileCheckpointKey, ProviderUsage, UsageCheckpoint, UsageSnapshot};

pub const USAGE_STATE_HEADER: &str = "rundog-usage-state-1";
pub const USAGE_STATE_SCHEMA_VERSION: u32 = 1;

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
    pub claude_keys: HashSet<u64>,
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
                    prefix: None,
                    last_model: None,
                    last_codex_total: None,
                }
            })
            .collect();
        cursors.sort_by(|left, right| cursor_sort_key(left).cmp(&cursor_sort_key(right)));
        Self {
            generation,
            schema_version: USAGE_STATE_SCHEMA_VERSION,
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
                "claude_in={claude_in}\n",
                "claude_out={claude_out}\n",
                "codex_today={codex_today}\n",
                "codex_month={codex_month}\n",
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
            claude_in = self.aggregate.snapshot.claude.month_input_tokens,
            claude_out = self.aggregate.snapshot.claude.month_output_tokens,
            codex_today = self.aggregate.snapshot.codex.today_cents,
            codex_month = self.aggregate.snapshot.codex.month_cents,
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
        let mut keys: Vec<u64> = self.claude_keys.iter().copied().collect();
        keys.sort_unstable();
        for key in keys {
            out.push_str(&format!("ckey={key}\n"));
        }
        out
    }

    #[must_use]
    pub fn decode(payload: &str) -> Option<Self> {
        let mut lines = payload.lines();
        if lines.next()? != USAGE_STATE_HEADER {
            return None;
        }
        let mut schema_version = None;
        let mut generation = None;
        let mut month_start = None;
        let mut today = None;
        let mut last_collected_ms = None;
        let mut catch_up_done = None;
        let mut claude_today = 0_u32;
        let mut claude_month = 0_u32;
        let mut claude_in = 0_u64;
        let mut claude_out = 0_u64;
        let mut codex_today = 0_u32;
        let mut codex_month = 0_u32;
        let mut codex_in = 0_u64;
        let mut codex_out = 0_u64;
        let mut cursors = Vec::new();
        let mut prefixes: Vec<(CursorKind, String, u64)> = Vec::new();
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
            } else if let Some(value) = line.strip_prefix("claude_in=") {
                claude_in = value.parse().ok()?;
            } else if let Some(value) = line.strip_prefix("claude_out=") {
                claude_out = value.parse().ok()?;
            } else if let Some(value) = line.strip_prefix("codex_today=") {
                codex_today = value.parse().ok()?;
            } else if let Some(value) = line.strip_prefix("codex_month=") {
                codex_month = value.parse().ok()?;
            } else if let Some(value) = line.strip_prefix("codex_in=") {
                codex_in = value.parse().ok()?;
            } else if let Some(value) = line.strip_prefix("codex_out=") {
                codex_out = value.parse().ok()?;
            } else if let Some(value) = line.strip_prefix("cursor=") {
                cursors.push(parse_cursor_line(value)?);
            } else if let Some(value) = line.strip_prefix("prefix=") {
                prefixes.push(parse_prefix_line(value)?);
            } else if let Some(value) = line.strip_prefix("codex_model=") {
                models.push(parse_codex_model_line(value)?);
            } else if let Some(value) = line.strip_prefix("codex_total=") {
                totals.push(parse_codex_total_line(value)?);
            } else if let Some(value) = line.strip_prefix("ckey=") {
                claude_keys.insert(value.parse().ok()?);
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
        if schema_version != USAGE_STATE_SCHEMA_VERSION {
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
                    claude: ProviderUsage {
                        today_cents: claude_today,
                        month_cents: claude_month,
                        month_input_tokens: claude_in,
                        month_output_tokens: claude_out,
                        ..ProviderUsage::default()
                    },
                    codex: ProviderUsage {
                        today_cents: codex_today,
                        month_cents: codex_month,
                        month_input_tokens: codex_in,
                        month_output_tokens: codex_out,
                        ..ProviderUsage::default()
                    },
                    month_scan_in_progress: false,
                },
            },
            cursors,
            claude_keys,
        })
    }
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
    };
    use crate::core::{
        CodexTokenTotals, FileCheckpointCursor, FileCheckpointKey, ProviderUsage, UsageCheckpoint,
        UsageSnapshot,
    };
    use std::path::Path;

    fn sample_state(catch_up_done: bool) -> UsageState {
        UsageState {
            generation: 3,
            schema_version: 1,
            aggregate: super::UsageAggregate {
                month_start: 20_260_901,
                today: 20_260_905,
                last_collected_ms: 10,
                catch_up_done,
                snapshot: UsageSnapshot {
                    claude: ProviderUsage {
                        month_cents: 12,
                        month_input_tokens: 100,
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
                prefix: Some(9),
                last_model: None,
                last_codex_total: None,
            }],
            claude_keys: [7].into_iter().collect(),
        }
    }

    fn sample_codex_state() -> UsageState {
        let mut state = sample_state(true);
        state.cursors.push(UsageCursor {
            kind: CursorKind::Codex,
            logical_id: "sessions/2026/09/a.jsonl".to_owned(),
            offset: 4,
            size: 8,
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
        assert!(encoded.contains("prefix=c\tprojects/p/session.jsonl\t9\n"));
        assert!(encoded.contains("cursor=c\tprojects/p/session.jsonl\t8\t16\n"));
        assert!(encoded.contains("ckey=7\n"));
        assert!(!encoded.contains("file="));
    }

    #[test]
    fn component_usage_state_round_trips_catch_up_in_progress() {
        let state = sample_state(false);
        let decoded = UsageState::decode(&state.encode()).expect("state");
        assert_eq!(decoded, state);
        assert!(!decoded.aggregate.catch_up_done);
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
        state.aggregate.snapshot.claude.month_cents = 0;
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
            Path::new(r"C:\Users\me\AppData\Local\SystemExe\RunDog\usage"),
            &[claude, codex]
        ));
    }
}
