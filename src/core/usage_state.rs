//! vNext usage durable state. Aggregate and cursors share one generation.
//!
//! This is not the Registry `UsageCheckpoint` text. Extra positional columns
//! from experimental reader/Codex patches are rejected, not merged in.

use std::collections::HashMap;

use super::{
    display_cents, hex_decode, ClaudePendingUsage, FileCheckpointKey, ProviderUsage, TokenUsage,
    UsageCheckpoint, UsageDedupeKey, UsageSnapshot, NANOS_PER_CENT, PENDING_CAP_CLAUDE,
    SEEN_CAP_CLAUDE, SEEN_CAP_CODEX,
};

const USAGE_STATE_HEADER_V1: &str = "rundog-usage-state-1";
pub const USAGE_STATE_HEADER: &str = "rundog-usage-state-2";
// Version 6 invalidates v1.1.38 cache markers without losing scan progress.
pub const USAGE_STATE_SCHEMA_VERSION: u32 = 6;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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
    /// Bounded, ordered event fingerprints for this source file.
    pub seen_keys: Vec<UsageDedupeKey>,
    /// The trailing Claude events still eligible for usage superseding.
    pub claude_pending: Vec<ClaudePendingUsage>,
    /// Last month in which new bytes were consumed. Missing legacy values
    /// conservatively use the checkpoint's aggregate month during restore.
    pub active_month: Option<u32>,
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
    /// Current month reconciled with an existing otak-usage cache. Cleared when
    /// the aggregate is rebuilt, so historical usage can be reconciled again.
    pub codex_cache_reconciled_month: Option<u32>,
    pub snapshot: UsageSnapshot,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UsageState {
    pub generation: u64,
    pub schema_version: u32,
    pub aggregate: UsageAggregate,
    pub cursors: Vec<UsageCursor>,
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
                    active_month: None,
                    kind,
                    logical_id,
                    offset: cursor.offset,
                    size: cursor.size,
                    file_id: None,
                    prefix: None,
                    last_model: None,
                    seen_keys: Vec::new(),
                    claude_pending: Vec::new(),
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
                codex_cache_reconciled_month: None,
                snapshot: UsageSnapshot {
                    claude: checkpoint.snapshot.claude,
                    codex: checkpoint.snapshot.codex,
                    month_scan_in_progress: false,
                },
            },
            cursors,
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
                "codex_cache_reconciled_month={codex_cache_reconciled_month}\n",
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
            codex_cache_reconciled_month = self.aggregate.codex_cache_reconciled_month.unwrap_or(0),
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
            if let Some(month) = cursor.active_month {
                out.push_str(&format!(
                    "active_month={kind}\t{}\t{month}\n",
                    cursor.logical_id
                ));
            }
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
            }
            for key in &cursor.seen_keys {
                out.push_str(&format!(
                    "seen={kind}\t{}\t{}\t{}\n",
                    cursor.logical_id,
                    key.encode_hex(),
                    key.month
                ));
            }
            for event in &cursor.claude_pending {
                if cursor.kind != CursorKind::Claude || !is_safe_model(&event.model) {
                    continue;
                }
                let usage = event.usage;
                out.push_str(&format!(
                    concat!(
                        "cpending={kind}\t{logical_id}\t{digest}\t{month}\t{day}\t",
                        "{model}\t{input}\t{cached_input}\t{cache_read}\t{cache_write_5m}\t",
                        "{cache_write_1h}\t{output}\t{long_context_input}\t",
                        "{long_context_cached_input}\t{long_context_output}\n"
                    ),
                    logical_id = cursor.logical_id,
                    kind = kind,
                    digest = event.key.encode_hex(),
                    month = event.key.month,
                    day = event.day,
                    model = event.model,
                    input = usage.input,
                    cached_input = usage.cached_input,
                    cache_read = usage.cache_read,
                    cache_write_5m = usage.cache_write_5m,
                    cache_write_1h = usage.cache_write_1h,
                    output = usage.output,
                    long_context_input = usage.long_context_input,
                    long_context_cached_input = usage.long_context_cached_input,
                    long_context_output = usage.long_context_output,
                ));
            }
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
        let mut codex_cache_reconciled_month = None;
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
        let mut active_months = HashMap::new();
        let mut models: Vec<(CursorKind, String, String)> = Vec::new();
        let mut seen_keys: HashMap<(CursorKind, String), Vec<UsageDedupeKey>> = HashMap::new();
        let mut pending_by_cursor: HashMap<(CursorKind, String), Vec<ClaudePendingUsage>> =
            HashMap::new();
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
            } else if let Some(value) = line.strip_prefix("codex_cache_reconciled_month=") {
                let month = value.parse::<u32>().ok()?;
                if month != 0 && (month % 100 != 1 || !(1..=12).contains(&(month / 100 % 100))) {
                    return None;
                }
                codex_cache_reconciled_month = (month != 0).then_some(month);
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
            } else if let Some(value) = line.strip_prefix("active_month=") {
                let (kind, id, month) = parse_prefix_line(value)?;
                let month = u32::try_from(month).ok()?;
                if month / 10_000 == 0
                    || month % 100 != 1
                    || !(1..=12).contains(&(month / 100 % 100))
                {
                    return None;
                }
                if active_months.insert((kind, id), month).is_some() {
                    return None;
                }
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
            } else if let Some(value) = line.strip_prefix("cpending=") {
                if value.starts_with("c\t") || value.starts_with("x\t") {
                    let (kind, logical_id, event) = parse_claude_pending_line(value)?;
                    let events = pending_by_cursor.entry((kind, logical_id)).or_default();
                    if events.iter().any(|previous| previous.key == event.key)
                        || events.len() >= PENDING_CAP_CLAUDE
                    {
                        return None;
                    }
                    events.push(event);
                }
            } else if let Some(value) = line.strip_prefix("seen=") {
                let (kind, logical_id, key) = parse_seen_line(value)?;
                let keys = seen_keys.entry((kind, logical_id)).or_default();
                let cap = match kind {
                    CursorKind::Claude => SEEN_CAP_CLAUDE,
                    CursorKind::Codex => SEEN_CAP_CODEX,
                };
                if keys.contains(&key) || keys.len() >= cap {
                    return None;
                }
                keys.push(key);
            } else if line.starts_with("codex_total=") || line.starts_with("dkey=") {
                // Older schema state is decoded only so apply_state can request
                // one clean accounting rebuild; these global watermarks are unused.
            } else if line.starts_with("ckey=") {
                // Legacy DefaultHasher u64. Not a disk contract; ignore.
            } else if line.starts_with("file=") {
                return None;
            }
        }
        for cursor in &mut cursors {
            cursor.active_month = active_months.remove(&(cursor.kind, cursor.logical_id.clone()));
        }
        if !active_months.is_empty() {
            return None;
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
        for cursor in &mut cursors {
            let identity = (cursor.kind, cursor.logical_id.clone());
            cursor.seen_keys = seen_keys.remove(&identity).unwrap_or_default();
            cursor.claude_pending = pending_by_cursor.remove(&identity).unwrap_or_default();
            if cursor.kind == CursorKind::Codex && !cursor.claude_pending.is_empty() {
                return None;
            }
            if cursor.claude_pending.iter().any(|event| {
                !cursor.seen_keys.contains(&event.key)
                    || event.key.month != event.day / 100 * 100 + 1
                    || cursor
                        .seen_keys
                        .iter()
                        .position(|key| *key == event.key)
                        .is_none_or(|index| {
                            index < cursor.seen_keys.len().saturating_sub(PENDING_CAP_CLAUDE)
                        })
            }) {
                return None;
            }
        }
        if !seen_keys.is_empty() || !pending_by_cursor.is_empty() {
            return None;
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
                codex_cache_reconciled_month,
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
        })
    }

    pub fn prune_event_keys_before_month(&mut self, month_start: u32) {
        for cursor in &mut self.cursors {
            cursor.seen_keys.retain(|key| key.month >= month_start);
            cursor
                .claude_pending
                .retain(|event| event.key.month >= month_start);
        }
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
        active_month: None,
        kind,
        logical_id: parts[1].to_owned(),
        offset: parts[2].parse().ok()?,
        size: parts[3].parse().ok()?,
        file_id: None,
        prefix: None,
        last_model: None,
        seen_keys: Vec::new(),
        claude_pending: Vec::new(),
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

fn parse_seen_line(value: &str) -> Option<(CursorKind, String, UsageDedupeKey)> {
    let parts: Vec<&str> = value.split('\t').collect();
    if parts.len() != 4 || parts[1].is_empty() {
        return None;
    }
    let kind = parse_cursor_kind(parts[0])?;
    let digest = hex_decode(parts[2])?;
    let month = parts[3].parse().ok()?;
    if !is_month_start(month) {
        return None;
    }
    Some((
        kind,
        parts[1].to_owned(),
        UsageDedupeKey::new(digest, month),
    ))
}

fn parse_claude_pending_line(value: &str) -> Option<(CursorKind, String, ClaudePendingUsage)> {
    let parts: Vec<&str> = value.split('\t').collect();
    if parts.len() != 15 || parts[1].is_empty() || !is_safe_model(parts[5]) {
        return None;
    }
    let kind = parse_cursor_kind(parts[0])?;
    let key = UsageDedupeKey::new(hex_decode(parts[2])?, parts[3].parse().ok()?);
    let day = parts[4].parse().ok()?;
    if kind != CursorKind::Claude || day / 100 * 100 + 1 != key.month {
        return None;
    }
    Some((
        kind,
        parts[1].to_owned(),
        ClaudePendingUsage {
            key,
            day,
            model: parts[5].to_owned(),
            usage: TokenUsage {
                input: parts[6].parse().ok()?,
                cached_input: parts[7].parse().ok()?,
                cache_read: parts[8].parse().ok()?,
                cache_write_5m: parts[9].parse().ok()?,
                cache_write_1h: parts[10].parse().ok()?,
                output: parts[11].parse().ok()?,
                long_context_input: parts[12].parse().ok()?,
                long_context_cached_input: parts[13].parse().ok()?,
                long_context_output: parts[14].parse().ok()?,
            },
        },
    ))
}

fn parse_cursor_kind(value: &str) -> Option<CursorKind> {
    match value {
        "c" => Some(CursorKind::Claude),
        "x" => Some(CursorKind::Codex),
        _ => None,
    }
}

fn is_month_start(month: u32) -> bool {
    month / 10_000 > 0 && month % 100 == 1 && (1..=12).contains(&(month / 100 % 100))
}

fn is_safe_model(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 64
        && model
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
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
        claude_dedupe_digest, ClaudePendingUsage, FileCheckpointCursor, FileCheckpointKey,
        ProviderUsage, TokenUsage, UsageCheckpoint, UsageDedupeKey, UsageSnapshot,
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
                codex_cache_reconciled_month: None,
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
                active_month: None,
                kind: CursorKind::Claude,
                logical_id: "projects/p/session.jsonl".to_owned(),
                offset: 8,
                size: 16,
                file_id: None,
                prefix: Some(9),
                last_model: None,
                seen_keys: vec![UsageDedupeKey::new(
                    claude_dedupe_digest("m", "r"),
                    20_260_901,
                )],
                claude_pending: Vec::new(),
            }],
        }
    }

    fn sample_codex_state() -> UsageState {
        let mut state = sample_state(true);
        state.cursors.push(UsageCursor {
            active_month: None,
            kind: CursorKind::Codex,
            logical_id: "sessions/2026/09/a.jsonl".to_owned(),
            offset: 4,
            size: 8,
            file_id: None,
            prefix: None,
            last_model: Some("gpt-5.4".to_owned()),
            seen_keys: vec![UsageDedupeKey::new(42, 20_260_901)],
            claude_pending: Vec::new(),
        });
        state
    }

    #[test]
    fn codex_cache_reconciliation_marker_round_trips_and_old_states_default_to_none() {
        let mut state = sample_codex_state();
        let old_payload = state
            .encode()
            .replace("codex_cache_reconciled_month=0\n", "");
        assert_eq!(
            UsageState::decode(&old_payload)
                .expect("pre-reconciliation state")
                .aggregate
                .codex_cache_reconciled_month,
            None
        );

        state.aggregate.codex_cache_reconciled_month = Some(20_260_901);
        let restored = UsageState::decode(&state.encode()).expect("reconciled state");
        assert_eq!(
            restored.aggregate.codex_cache_reconciled_month,
            Some(20_260_901)
        );
        assert_eq!(
            restored.aggregate.snapshot.codex,
            state.aggregate.snapshot.codex
        );
        assert_eq!(restored.cursors, state.cursors);
    }

    #[test]
    fn cursor_activity_month_roundtrips_and_is_optional_for_legacy_states() {
        let mut state = sample_codex_state();
        assert!(UsageState::decode(&state.encode())
            .unwrap()
            .cursors
            .iter()
            .all(|cursor| cursor.active_month.is_none()));
        state.cursors[0].active_month = Some(20_260_801);
        state.cursors[1].active_month = Some(20_260_901);
        let decoded = UsageState::decode(&state.encode()).unwrap();
        assert_eq!(decoded, state);
    }

    #[test]
    fn cursor_activity_month_rejects_invalid_duplicate_and_orphan_records() {
        let state = sample_state(true).encode();
        for row in [
            "active_month=c\tprojects/p/session.jsonl\t20261301\n",
            "active_month=c\tprojects/p/session.jsonl\t20260902\n",
            "active_month=c\tprojects/p/session.jsonl\t0\n",
            "active_month=c\tprojects/missing.jsonl\t20260901\n",
            "active_month=c\tprojects/p/session.jsonl\t20260901\nactive_month=c\tprojects/p/session.jsonl\t20260901\n",
        ] {
            assert!(UsageState::decode(&format!("{state}{row}")).is_none(), "{row}");
        }
    }

    #[test]
    fn component_encode_orders_claude_before_codex_then_logical_id() {
        let mut state = sample_state(true);
        state.cursors = vec![
            UsageCursor {
                active_month: None,
                kind: CursorKind::Codex,
                logical_id: "sessions/z.jsonl".to_owned(),
                offset: 1,
                size: 1,
                file_id: None,
                prefix: None,
                last_model: None,
                seen_keys: Vec::new(),
                claude_pending: Vec::new(),
            },
            UsageCursor {
                active_month: None,
                kind: CursorKind::Claude,
                logical_id: "projects/z.jsonl".to_owned(),
                offset: 2,
                size: 2,
                file_id: None,
                prefix: None,
                last_model: None,
                seen_keys: Vec::new(),
                claude_pending: Vec::new(),
            },
            UsageCursor {
                active_month: None,
                kind: CursorKind::Claude,
                logical_id: "projects/a.jsonl".to_owned(),
                offset: 3,
                size: 3,
                file_id: None,
                prefix: None,
                last_model: None,
                seen_keys: Vec::new(),
                claude_pending: Vec::new(),
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
    fn component_usage_state_persists_codex_dedupe_per_file() {
        let encoded = sample_codex_state().encode();
        assert!(encoded.contains("codex_model=x\tsessions/2026/09/a.jsonl\tgpt-5.4\n"));
        assert!(encoded.contains("cursor=x\tsessions/2026/09/a.jsonl\t4\t8\n"));
        assert!(encoded.contains("seen=x\tsessions/2026/09/a.jsonl\t000000000000002a\t20260901\n"));
        assert!(!encoded.contains("codex_total="));
        assert!(!encoded.contains("file="));
        let decoded = UsageState::decode(&encoded).expect("state");
        assert_eq!(decoded.cursors[1].last_model.as_deref(), Some("gpt-5.4"));
        assert_eq!(
            decoded.cursors[1].seen_keys,
            vec![UsageDedupeKey::new(42, 20_260_901)]
        );
    }

    #[test]
    fn component_usage_state_round_trips_claude_revision_window() {
        let mut state = sample_state(true);
        let key = state.cursors[0].seen_keys[0];
        let pending = ClaudePendingUsage {
            key,
            day: 20_260_905,
            model: "claude-opus-5".to_owned(),
            usage: TokenUsage {
                input: 101,
                cache_read: 12,
                cache_write_5m: 4,
                output: 99,
                ..TokenUsage::default()
            },
        };
        state.cursors[0].claude_pending.push(pending.clone());

        let encoded = state.encode();
        let decoded = UsageState::decode(&encoded).expect("state");

        assert_eq!(decoded.cursors[0].claude_pending, vec![pending]);
    }

    #[test]
    fn component_usage_state_named_prefix_is_not_a_positional_file_column() {
        let encoded = sample_state(true).encode();
        let key = UsageDedupeKey::new(claude_dedupe_digest("m", "r"), 20_260_901);
        assert!(encoded.contains("prefix=c\tprojects/p/session.jsonl\t9\n"));
        assert!(encoded.contains("cursor=c\tprojects/p/session.jsonl\t8\t16\n"));
        assert!(encoded.contains(&format!(
            "seen=c\tprojects/p/session.jsonl\t{}\t20260901\n",
            key.encode_hex()
        )));
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
        assert_eq!(decoded.cursors[0].seen_keys.len(), 1);
    }

    #[test]
    fn component_usage_state_rejects_positional_extra_seen_columns() {
        let mut payload = sample_state(true).encode();
        payload = payload.replace(
            "seen=c\tprojects/p/session.jsonl\t",
            "seen=c\tprojects/p/session.jsonl\ttoo-many\t",
        );
        assert!(UsageState::decode(&payload).is_none());
    }

    #[test]
    fn component_usage_state_month_rollover_keeps_current_month_dedupe() {
        let mut state = sample_state(true);
        state.cursors[0]
            .seen_keys
            .push(UsageDedupeKey::new(123, 20_260_801));
        state.prune_event_keys_before_month(20_260_901);
        assert_eq!(state.cursors[0].seen_keys.len(), 1);
        assert_eq!(state.cursors[0].seen_keys[0].month, 20_260_901);
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
