//! Durable cursor for incremental Claude / Codex JSONL usage collection.

use std::collections::{HashMap, HashSet};

use super::{ProviderUsage, UsageSnapshot};

const HEADER_V1: &str = "rundog-usage-checkpoint-1";
const HEADER_V2: &str = "rundog-usage-checkpoint-2";
const HEADER: &str = "rundog-usage-checkpoint-3";

/// Registry migration epoch for `UsageCheckpoint`. Bump when on-disk layout or
/// restore semantics change and stale checkpoints must be discarded.
pub const USAGE_CHECKPOINT_MIGRATION_VERSION: u32 = 4;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum FileCheckpointKey {
    Claude(String),
    Codex(String),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FileCheckpointCursor {
    pub offset: u64,
    pub size: u64,
    pub prefix: u64,
    pub has_prefix: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageCheckpoint {
    pub month_start: u32,
    pub today: u32,
    pub last_collected_ms: u64,
    pub catch_up_done: bool,
    pub snapshot: UsageSnapshot,
    pub files: HashMap<FileCheckpointKey, FileCheckpointCursor>,
    pub claude_keys: HashSet<u64>,
}

impl UsageCheckpoint {
    #[must_use]
    pub fn encode(&self) -> String {
        let mut out = format!(
            concat!(
                "month={}\n",
                "day={}\n",
                "last_ms={}\n",
                "catch_up_done={}\n",
                "claude_today={}\n",
                "claude_month={}\n",
                "claude_in={}\n",
                "claude_out={}\n",
                "codex_today={}\n",
                "codex_month={}\n",
                "codex_in={}\n",
                "codex_out={}\n",
            ),
            self.month_start,
            self.today,
            self.last_collected_ms,
            u32::from(self.catch_up_done),
            self.snapshot.claude.today_cents,
            self.snapshot.claude.month_cents,
            self.snapshot.claude.month_input_tokens,
            self.snapshot.claude.month_output_tokens,
            self.snapshot.codex.today_cents,
            self.snapshot.codex.month_cents,
            self.snapshot.codex.month_input_tokens,
            self.snapshot.codex.month_output_tokens,
        );
        out.insert_str(0, &format!("{HEADER}\n"));
        let mut files: Vec<_> = self.files.iter().collect();
        files.sort_by(|(left, _), (right, _)| {
            file_key_sort_key(left).cmp(&file_key_sort_key(right))
        });
        for (key, cursor) in files {
            let prefix = match key {
                FileCheckpointKey::Claude(_) => 'c',
                FileCheckpointKey::Codex(_) => 'x',
            };
            let path = match key {
                FileCheckpointKey::Claude(path) | FileCheckpointKey::Codex(path) => path,
            };
            if cursor.has_prefix {
                out.push_str(&format!(
                    "file={prefix}\t{path}\t{}\t{}\t{}\n",
                    cursor.offset, cursor.size, cursor.prefix
                ));
            } else {
                out.push_str(&format!(
                    "file={prefix}\t{path}\t{}\t{}\n",
                    cursor.offset, cursor.size
                ));
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
        match lines.next()? {
            HEADER => {}
            HEADER_V1 | HEADER_V2 => return None,
            _ => return None,
        }
        let mut month_start = None;
        let mut today = None;
        let mut last_collected_ms = None;
        let mut catch_up_done = false;
        let mut claude_today = 0_u32;
        let mut claude_month = 0_u32;
        let mut claude_in = 0_u64;
        let mut claude_out = 0_u64;
        let mut codex_today = 0_u32;
        let mut codex_month = 0_u32;
        let mut codex_in = 0_u64;
        let mut codex_out = 0_u64;
        let mut files = HashMap::new();
        let mut claude_keys = HashSet::new();
        for line in lines {
            if let Some(value) = line.strip_prefix("month=") {
                month_start = value.parse().ok();
            } else if let Some(value) = line.strip_prefix("day=") {
                today = value.parse().ok();
            } else if let Some(value) = line.strip_prefix("last_ms=") {
                last_collected_ms = value.parse().ok();
            } else if let Some(value) = line.strip_prefix("catch_up_done=") {
                catch_up_done = value.parse::<u32>().ok()? != 0;
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
            } else if let Some(value) = line.strip_prefix("file=") {
                let (prefix, path, cursor) = parse_file_line(value)?;
                let key = match prefix {
                    'c' => FileCheckpointKey::Claude(path.to_owned()),
                    'x' => FileCheckpointKey::Codex(path.to_owned()),
                    _ => return None,
                };
                files.insert(key, cursor);
            } else if let Some(value) = line.strip_prefix("ckey=") {
                if let Ok(key) = value.parse() {
                    claude_keys.insert(key);
                }
            }
        }
        if !catch_up_done {
            return None;
        }
        Some(Self {
            month_start: month_start?,
            today: today?,
            last_collected_ms: last_collected_ms?,
            catch_up_done,
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
                ..UsageSnapshot::default()
            },
            files,
            claude_keys,
        })
    }
}

fn file_key_sort_key(key: &FileCheckpointKey) -> (&'static str, &str) {
    match key {
        FileCheckpointKey::Claude(path) => ("c", path.as_str()),
        FileCheckpointKey::Codex(path) => ("x", path.as_str()),
    }
}

fn parse_file_line(value: &str) -> Option<(char, &str, FileCheckpointCursor)> {
    let mut parts = value.split('\t');
    let prefix = parts.next()?.chars().next()?;
    let path = parts.next()?;
    let offset = parts.next()?.parse().ok()?;
    let size = parts.next()?.parse().ok()?;
    let mut cursor = FileCheckpointCursor {
        offset,
        size,
        ..FileCheckpointCursor::default()
    };
    if let Some(prefix_hash) = parts.next() {
        cursor.prefix = prefix_hash.parse().ok()?;
        cursor.has_prefix = true;
    }
    Some((prefix, path, cursor))
}

#[cfg(test)]
mod tests {
    use super::{
        FileCheckpointCursor, FileCheckpointKey, UsageCheckpoint, HEADER, HEADER_V1, HEADER_V2,
    };
    use crate::core::{ProviderUsage, UsageSnapshot};
    use std::collections::{HashMap, HashSet};

    #[test]
    fn component_usage_checkpoint_round_trips_costs_and_tokens() {
        let mut checkpoint = UsageCheckpoint {
            month_start: 20_260_801,
            today: 20_260_823,
            last_collected_ms: 1_786_865_940_000,
            catch_up_done: true,
            snapshot: UsageSnapshot {
                claude: ProviderUsage {
                    today_cents: 12,
                    month_cents: 28_449,
                    month_input_tokens: 142_500_000,
                    month_output_tokens: 3_200_000,
                    ..ProviderUsage::default()
                },
                codex: ProviderUsage {
                    today_cents: 56,
                    month_cents: 879_982,
                    month_input_tokens: 9_500_000,
                    month_output_tokens: 250_000,
                    ..ProviderUsage::default()
                },
                ..UsageSnapshot::default()
            },
            files: [(
                FileCheckpointKey::Claude("projects/p1/session.jsonl".to_owned()),
                FileCheckpointCursor {
                    offset: 4096,
                    size: 4096,
                    ..FileCheckpointCursor::default()
                },
            )]
            .into(),
            claude_keys: [0x1111_u64, 0x2222].into_iter().collect(),
        };
        let decoded = UsageCheckpoint::decode(&checkpoint.encode()).expect("checkpoint");
        assert_eq!(decoded, checkpoint);

        checkpoint.files.insert(
            FileCheckpointKey::Codex("sessions/2026/08/a.jsonl".to_owned()),
            FileCheckpointCursor {
                offset: 10,
                size: 20,
                prefix: 42,
                has_prefix: true,
            },
        );
        assert_eq!(
            UsageCheckpoint::decode(&checkpoint.encode()).expect("checkpoint"),
            checkpoint
        );
    }

    #[test]
    fn component_v1_v2_and_incomplete_checkpoints_are_rejected() {
        let complete = UsageCheckpoint {
            month_start: 20_260_801,
            today: 20_260_823,
            last_collected_ms: 0,
            catch_up_done: true,
            snapshot: UsageSnapshot::default(),
            files: HashMap::new(),
            claude_keys: HashSet::new(),
        };
        let mut v1_payload = complete.encode();
        v1_payload = v1_payload.replacen(HEADER, HEADER_V1, 1);
        assert!(UsageCheckpoint::decode(&v1_payload).is_none());

        let mut v2_payload = complete.encode();
        v2_payload = v2_payload.replacen(HEADER, HEADER_V2, 1);
        assert!(UsageCheckpoint::decode(&v2_payload).is_none());

        let incomplete = UsageCheckpoint {
            catch_up_done: false,
            ..complete
        };
        assert!(UsageCheckpoint::decode(&incomplete.encode()).is_none());
    }
}
