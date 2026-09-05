/// Local Claude Code / Codex CLI usage shown on the hover flyout.
///
/// Amounts are API-equivalent estimates. Subscription rate-limit windows are
/// separate from those dollar figures.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TokenUsage {
    pub input: u64,
    pub cached_input: u64,
    pub cache_read: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub output: u64,
    pub long_context_input: u64,
    pub long_context_cached_input: u64,
    pub long_context_output: u64,
}

impl TokenUsage {
    /// Measured input-side tokens. `long_context_*` classify the same tokens
    /// for pricing and must not be added again.
    #[must_use]
    pub const fn processed_input_tokens(self) -> u64 {
        self.input
            .saturating_add(self.cached_input)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write_5m)
            .saturating_add(self.cache_write_1h)
    }

    /// Measured output-side tokens. `long_context_output` is a pricing class.
    #[must_use]
    pub const fn processed_output_tokens(self) -> u64 {
        self.output
    }
}

/// One 5-hour or weekly rate-limit window.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LimitWindow {
    /// Tenths of a percent, 0–1000 for 0.0%–100.0%.
    pub used_tenths: u16,
    /// Unix epoch milliseconds. Zero means unknown.
    pub resets_at_ms: u64,
    pub window_minutes: u16,
}

impl LimitWindow {
    #[must_use]
    pub fn used_percent(self) -> f32 {
        f32::from(self.used_tenths) / 10.0
    }

    /// Expired data, not a display instruction. Flyout text uses [`is_current`].
    #[must_use]
    pub fn effective(self, now_ms: u64) -> Self {
        if self.resets_at_ms != 0 && self.resets_at_ms <= now_ms {
            Self {
                used_tenths: 0,
                resets_at_ms: 0,
                window_minutes: self.window_minutes,
            }
        } else {
            self
        }
    }

    /// Live percentage may be shown only before `resets_at_ms`.
    ///
    /// Unknown (`resets_at_ms == 0`) and expired windows are not current.
    /// Presentation must not render those as 0%.
    #[must_use]
    pub const fn is_current(self, now_ms: u64) -> bool {
        self.resets_at_ms != 0 && self.resets_at_ms > now_ms
    }
}

/// Flyout label for a rate-limit window.
///
/// Expired / unknown reset times are "—" — never a fabricated 0%.
/// Missing windows use the same dash so a failed fetch cannot look unused.
#[must_use]
pub fn format_limit_label(name: &str, window: Option<LimitWindow>, now_ms: u64) -> String {
    match window {
        Some(window) if window.is_current(now_ms) => {
            format!("{name}: {:.0}%", window.used_percent())
        }
        _ => format!("{name}: —"),
    }
}

/// Flyout label for the Claude Fable weekly bucket.
///
/// Same freshness contract as [`format_limit_label`].
#[must_use]
pub fn format_fable_limit_label(window: LimitWindow, now_ms: u64) -> String {
    format_limit_label("Fable", Some(window), now_ms)
}

/// Codex banked reset count. Hidden when the field is absent or zero.
///
/// This is a credit count, not a third reset timestamp and not a usage %.
#[must_use]
pub fn format_banked_reset_label(available: Option<u16>) -> Option<String> {
    match available {
        Some(count) if count > 0 => Some(format!("Banked Reset: {count}")),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProviderUsage {
    pub today_cents: u32,
    pub month_cents: u32,
    pub month_input_tokens: u64,
    pub month_output_tokens: u64,
    pub plan: [u8; 16],
    pub plan_len: u8,
    pub primary: Option<LimitWindow>,
    pub secondary: Option<LimitWindow>,
    /// Claude weekly Fable-scoped meter. Absent when the provider omitted it.
    pub fable: Option<LimitWindow>,
    /// Codex banked reset credits (`available_count`). `None` means omitted.
    pub banked_reset_available: Option<u16>,
}

impl ProviderUsage {
    #[must_use]
    pub fn plan_label(self) -> Option<String> {
        if self.plan_len == 0 {
            return None;
        }
        let bytes = self.plan.get(..self.plan_len as usize)?;
        core::str::from_utf8(bytes).ok().map(str::to_owned)
    }

    pub fn set_plan(&mut self, label: &str) {
        let formatted = format_plan_label(label);
        let bytes = formatted.as_bytes();
        let len = bytes.len().min(self.plan.len());
        self.plan[..len].copy_from_slice(&bytes[..len]);
        self.plan_len = len as u8;
    }

    /// True when this month's jsonl produced an API-equivalent cost.
    ///
    /// Limit windows alone do not count: leftover credentials can still
    /// return 5h/7d bars without any use this month.
    #[must_use]
    pub const fn has_month_activity(self) -> bool {
        self.month_cents > 0
            || self.today_cents > 0
            || self.month_input_tokens > 0
            || self.month_output_tokens > 0
    }

    #[must_use]
    pub fn session_window(self) -> Option<LimitWindow> {
        self.window_matching(|minutes| minutes > 0 && minutes < 1_440)
    }

    #[must_use]
    pub fn weekly_window(self) -> Option<LimitWindow> {
        self.window_matching(|minutes| minutes >= 1_440)
    }

    fn window_matching(self, pred: impl Fn(u16) -> bool) -> Option<LimitWindow> {
        [self.primary, self.secondary]
            .into_iter()
            .flatten()
            .find(|window| pred(window.window_minutes))
    }

    /// True when the Codex payload reported a positive banked-reset count.
    #[must_use]
    pub const fn shows_banked_reset(self) -> bool {
        matches!(self.banked_reset_available, Some(count) if count > 0)
    }
}

/// `default_claude_max_20x` / `pro` → `Max 20x` / `Pro 20x`.
#[must_use]
pub fn format_plan_label(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    let name = if lower.contains("enterprise") {
        "Enterprise"
    } else if lower.contains("business") {
        "Business"
    } else if lower.contains("team") {
        "Team"
    } else if lower.contains("plus") {
        "Plus"
    } else if lower.contains("max") {
        "Max"
    } else if lower.contains("pro") {
        "Pro"
    } else {
        raw.trim()
    };
    let multiplier = extract_multiplier(&lower).or_else(|| {
        if name.eq_ignore_ascii_case("pro") {
            Some("20x")
        } else {
            None
        }
    });
    match multiplier {
        Some(multiplier) if !name.is_empty() => format!("{name} {multiplier}"),
        _ if !name.is_empty() => name.to_owned(),
        _ => raw.to_owned(),
    }
}

fn extract_multiplier(lower: &str) -> Option<&'static str> {
    ["20x", "5x", "2x"]
        .into_iter()
        .find(|candidate| lower.contains(candidate))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UsageSnapshot {
    pub claude: ProviderUsage,
    pub codex: ProviderUsage,
    /// True while the collector is re-reading JSONL for the current month.
    pub month_scan_in_progress: bool,
}

/// Compact decimal token count for flyout labels (`1.2K`, `3.4M`, `5.6B`).
#[must_use]
pub fn format_compact_token_count(tokens: u64) -> String {
    const K: f64 = 1_000.0;
    const M: f64 = 1_000_000.0;
    const B: f64 = 1_000_000_000.0;
    let value = tokens as f64;
    if value >= B {
        format!("{:.1}B", value / B)
    } else if value >= M {
        format!("{:.1}M", value / M)
    } else if value >= K {
        format!("{:.1}K", value / K)
    } else {
        tokens.to_string()
    }
}

/// USD cents from a per-million-token price table.
///
/// `effective_day` is a `YYYY-MM-DD` local day used for scheduled price
/// revisions, matching otak-usage `calcCost`.
#[must_use]
pub fn cost_cents(model: &str, usage: TokenUsage, effective_day: Option<&str>) -> Option<u32> {
    let pricing = resolve_pricing(model, effective_day)?;
    let long_input_premium = pricing.long_context_input_multiplier.unwrap_or(1.0) - 1.0;
    let long_output_premium = pricing.long_context_output_multiplier.unwrap_or(1.0) - 1.0;
    let usd = (usage.input as f64).mul_add(
        pricing.input,
        (usage.cached_input as f64).mul_add(
            pricing.cached_input,
            (usage.cache_read as f64).mul_add(
                pricing.cache_read,
                (usage.cache_write_5m as f64).mul_add(
                    pricing.cache_write_5m,
                    (usage.cache_write_1h as f64).mul_add(
                        pricing.cache_write_1h,
                        (usage.output as f64).mul_add(
                            pricing.output,
                            (usage.long_context_input as f64).mul_add(
                                pricing.input * long_input_premium,
                                (usage.long_context_cached_input as f64).mul_add(
                                    pricing.cached_input * long_input_premium,
                                    usage.long_context_output as f64
                                        * pricing.output
                                        * long_output_premium,
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        ),
    ) / 1_000_000.0;
    if usd.is_finite() && usd >= 0.0 {
        Some((usd * 100.0).round() as u32)
    } else {
        None
    }
}

struct ModelPricing {
    key: &'static str,
    input: f64,
    output: f64,
    cached_input: f64,
    cache_read: f64,
    cache_write_5m: f64,
    cache_write_1h: f64,
    long_context_threshold: Option<u64>,
    long_context_input_multiplier: Option<f64>,
    long_context_output_multiplier: Option<f64>,
}

fn entry(key: &'static str, input: f64, output: f64) -> ModelPricing {
    ModelPricing {
        key,
        input,
        output,
        cached_input: input * 0.1,
        cache_read: input * 0.1,
        cache_write_5m: input * 1.25,
        cache_write_1h: input * 2.0,
        long_context_threshold: None,
        long_context_input_multiplier: None,
        long_context_output_multiplier: None,
    }
}

fn entry_cached(key: &'static str, input: f64, cached: f64, output: f64) -> ModelPricing {
    let mut pricing = entry(key, input, output);
    pricing.cached_input = cached;
    pricing
}

fn entry_cache_read(key: &'static str, input: f64, cache_read: f64, output: f64) -> ModelPricing {
    let mut pricing = entry(key, input, output);
    pricing.cached_input = cache_read;
    pricing.cache_read = cache_read;
    pricing
}

fn entry_long_context(key: &'static str, input: f64, cached: f64, output: f64) -> ModelPricing {
    let mut pricing = entry_cached(key, input, cached, output);
    pricing.long_context_threshold = Some(272_000);
    pricing.long_context_input_multiplier = Some(2.0);
    pricing.long_context_output_multiplier = Some(1.5);
    pricing
}

fn pricing_table() -> [ModelPricing; 56] {
    [
        entry_cache_read("claude-fable-5-1", 10.0, 0.25, 50.0),
        entry_cache_read("claude-mythos-5-1", 10.0, 0.25, 50.0),
        entry("claude-fable-5", 10.0, 50.0),
        entry("claude-mythos-5", 10.0, 50.0),
        entry("claude-opus-5", 5.0, 25.0),
        entry("claude-opus-4-8", 5.0, 25.0),
        entry("claude-opus-4-7", 5.0, 25.0),
        entry("claude-opus-4-6", 5.0, 25.0),
        entry("claude-opus-4-5", 5.0, 25.0),
        entry("claude-opus-4-1", 15.0, 75.0),
        entry("claude-opus-4", 15.0, 75.0),
        entry("claude-opus-5-fast", 10.0, 50.0),
        entry("claude-opus-4-8-fast", 10.0, 50.0),
        entry("claude-opus-4-7-fast", 30.0, 150.0),
        entry("claude-opus-4-6-fast", 30.0, 150.0),
        entry("claude-sonnet-5", 2.0, 10.0),
        entry("claude-sonnet-4-6", 3.0, 15.0),
        entry("claude-sonnet-4-5", 3.0, 15.0),
        entry("claude-sonnet-4", 3.0, 15.0),
        entry("claude-haiku-4-5", 1.0, 5.0),
        entry("claude-3-7-sonnet", 3.0, 15.0),
        entry("claude-3-5-sonnet", 3.0, 15.0),
        entry("claude-3-5-haiku", 0.8, 4.0),
        entry("claude-3-opus", 15.0, 75.0),
        entry("claude-3-sonnet", 3.0, 15.0),
        entry("claude-3-haiku", 0.25, 1.25),
        entry_long_context("gpt-5.6-sol", 5.0, 0.5, 30.0),
        entry_long_context("gpt-5.6-terra", 2.5, 0.25, 15.0),
        entry_long_context("gpt-5.6-luna", 1.0, 0.1, 6.0),
        entry_long_context("gpt-5.6", 5.0, 0.5, 30.0),
        entry_long_context("gpt-5.5-pro", 30.0, 3.0, 180.0),
        entry_long_context("gpt-5.5", 5.0, 0.5, 30.0),
        entry_long_context("gpt-5.4-pro", 30.0, 3.0, 180.0),
        entry_cached("gpt-5.4-mini", 0.75, 0.075, 4.5),
        entry_cached("gpt-5.4-nano", 0.2, 0.02, 1.25),
        entry_long_context("gpt-5.4", 2.5, 0.25, 15.0),
        entry_cached("gpt-5.3-codex", 1.75, 0.175, 14.0),
        entry_cached("gpt-5.2-codex", 1.75, 0.175, 14.0),
        entry_cached("gpt-5.2", 1.75, 0.175, 14.0),
        entry_cached("gpt-5.1-codex-mini", 0.25, 0.025, 2.0),
        entry_cached("gpt-5.1-codex", 1.25, 0.125, 10.0),
        entry_cached("gpt-5.1", 1.25, 0.125, 10.0),
        entry_cached("gpt-5-codex", 1.25, 0.125, 10.0),
        entry_cached("gpt-5-mini", 0.25, 0.025, 2.0),
        entry_cached("gpt-5-nano", 0.05, 0.005, 0.4),
        entry_cached("gpt-5", 1.25, 0.125, 10.0),
        entry_cached("codex-mini-latest", 1.5, 0.375, 6.0),
        entry("o3-pro", 20.0, 80.0),
        entry_cached("o3-mini", 1.1, 0.55, 4.4),
        entry_cached("o3", 2.0, 0.5, 8.0),
        entry_cached("o4-mini", 1.1, 0.275, 4.4),
        entry_cached("gpt-4.1-mini", 0.4, 0.1, 1.6),
        entry_cached("gpt-4.1-nano", 0.1, 0.025, 0.4),
        entry_cached("gpt-4.1", 2.0, 0.5, 8.0),
        entry_cached("gpt-4o-mini", 0.15, 0.075, 0.6),
        entry_cached("gpt-4o", 2.5, 1.25, 10.0),
    ]
}

fn resolve_pricing(model: &str, effective_day: Option<&str>) -> Option<ModelPricing> {
    let table = pricing_table();
    let mut pricing = if let Some(exact) = table.iter().find(|entry| entry.key == model) {
        copy_pricing(exact)
    } else {
        table
            .iter()
            .filter(|entry| model_matches(model, entry.key))
            .max_by_key(|entry| entry.key.len())
            .map(copy_pricing)?
    };
    apply_revisions(&mut pricing, effective_day);
    Some(pricing)
}

fn apply_revisions(pricing: &mut ModelPricing, effective_day: Option<&str>) {
    let Some(day) = effective_day else {
        return;
    };
    match pricing.key {
        "claude-sonnet-5" if day >= "2026-09-01" => {
            pricing.input = 3.0;
            pricing.output = 15.0;
            refresh_cache_from_input(pricing);
        }
        "gpt-5.6-terra" if day >= "2026-07-30" => {
            pricing.input = 2.0;
            pricing.cached_input = 0.2;
            pricing.output = 12.0;
        }
        "gpt-5.6-luna" if day >= "2026-07-30" => {
            pricing.input = 0.2;
            pricing.cached_input = 0.02;
            pricing.output = 1.2;
        }
        "gpt-5.6-sol" if day >= "2026-08-21" => {
            pricing.input = 4.0;
            pricing.output = 20.0;
            refresh_cache_from_input(pricing);
        }
        _ => {}
    }
}

fn refresh_cache_from_input(pricing: &mut ModelPricing) {
    pricing.cached_input = pricing.input * 0.1;
    pricing.cache_read = pricing.input * 0.1;
    pricing.cache_write_5m = pricing.input * 1.25;
    pricing.cache_write_1h = pricing.input * 2.0;
}

fn copy_pricing(entry: &ModelPricing) -> ModelPricing {
    ModelPricing {
        key: entry.key,
        input: entry.input,
        output: entry.output,
        cached_input: entry.cached_input,
        cache_read: entry.cache_read,
        cache_write_5m: entry.cache_write_5m,
        cache_write_1h: entry.cache_write_1h,
        long_context_threshold: entry.long_context_threshold,
        long_context_input_multiplier: entry.long_context_input_multiplier,
        long_context_output_multiplier: entry.long_context_output_multiplier,
    }
}

#[must_use]
pub fn is_long_context_request(model: &str, input_tokens: u64) -> bool {
    resolve_pricing(model, None)
        .and_then(|pricing| pricing.long_context_threshold)
        .is_some_and(|threshold| input_tokens > threshold)
}

#[must_use]
pub fn resolve_codex_model(model: &str) -> &str {
    if model == "codex-auto-review" {
        "gpt-5.4"
    } else {
        model
    }
}

#[must_use]
pub fn ymd_iso(year: i32, month: u8, day: u8) -> String {
    format!("{year:04}-{month:02}-{day:02}")
}

fn model_matches(model: &str, key: &str) -> bool {
    if model.starts_with(key) {
        return true;
    }
    const FAST: &str = "-fast";
    key.ends_with(FAST)
        && model.ends_with(FAST)
        && model[..model.len() - FAST.len()].starts_with(&key[..key.len() - FAST.len()])
}

/// Windows `TIME_ZONE_INFORMATION` composition.
///
/// `TIME_ZONE_ID_STANDARD` (1) adds `StandardBias`. `TIME_ZONE_ID_DAYLIGHT`
/// (2) adds `DaylightBias`. Unknown / invalid keep `Bias` only — do not invent
/// a DST transition.
#[must_use]
pub fn windows_tz_bias_minutes(
    bias: i32,
    standard_bias: i32,
    daylight_bias: i32,
    zone_id: u32,
) -> i32 {
    match zone_id {
        1 => bias.saturating_add(standard_bias),
        2 => bias.saturating_add(daylight_bias),
        _ => bias,
    }
}

/// Parse an RFC3339-like timestamp to Unix milliseconds (UTC).
///
/// Accepts `Z`, `+HH:MM`, `-HH:MM`, `+HHMM`, and `-HHMM`. A missing offset is
/// not a unique instant and is rejected. The civil clock is never treated as
/// UTC when an offset is present.
#[must_use]
pub fn parse_rfc3339_ms(value: &str) -> Option<u64> {
    if value.len() < 20 {
        return None;
    }
    let bytes = value.as_bytes();
    if bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return None;
    }
    let year: i32 = value.get(0..4)?.parse().ok()?;
    let month: u8 = value.get(5..7)?.parse().ok()?;
    let day: u8 = value.get(8..10)?.parse().ok()?;
    let hour: u8 = value.get(11..13)?.parse().ok()?;
    let minute: u8 = value.get(14..16)?.parse().ok()?;
    let second: u8 = value.get(17..19)?.parse().ok()?;
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut idx = 19;
    if bytes.get(idx) == Some(&b'.') {
        idx += 1;
        while idx < bytes.len() && bytes[idx].is_ascii_digit() {
            idx += 1;
        }
    }
    let offset_minutes = parse_rfc3339_offset(value.get(idx..)?)?;
    let days = ymd_to_epoch_days(year, month, day)?;
    let civil_ms = days
        .saturating_mul(86_400)
        .saturating_add(u64::from(hour) * 3_600)
        .saturating_add(u64::from(minute) * 60)
        .saturating_add(u64::from(second))
        .saturating_mul(1_000);
    let utc = i64::try_from(civil_ms)
        .ok()?
        .checked_sub(i64::from(offset_minutes) * 60_000)?;
    u64::try_from(utc).ok()
}

fn parse_rfc3339_offset(value: &str) -> Option<i32> {
    if value == "Z" || value == "z" {
        return Some(0);
    }
    let sign = match value.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let rest = value.get(1..)?;
    let (hours, minutes) = if rest.len() == 5 && rest.as_bytes().get(2) == Some(&b':') {
        (
            rest.get(0..2)?.parse::<i32>().ok()?,
            rest.get(3..5)?.parse::<i32>().ok()?,
        )
    } else if rest.len() == 4 {
        (
            rest.get(0..2)?.parse::<i32>().ok()?,
            rest.get(2..4)?.parse::<i32>().ok()?,
        )
    } else {
        return None;
    };
    if !(0..=23).contains(&hours) || !(0..=59).contains(&minutes) {
        return None;
    }
    Some(sign * (hours * 60 + minutes))
}

fn ymd_to_epoch_days(year: i32, month: u8, day: u8) -> Option<u64> {
    if !(1..=12).contains(&month) || day == 0 {
        return None;
    }
    let (y, m) = if month <= 2 {
        (year - 1, i32::from(month) + 9)
    } else {
        (year, i32::from(month) - 3)
    };
    let era = y.div_euclid(400);
    let yoe = (y - era * 400) as u64;
    let doy = (153 * m as u64 + 2) / 5 + u64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era as i64 * 146_097 + doe as i64 - 719_468) as u64)
}

/// Civil date from Unix milliseconds shifted by a timezone bias in minutes.
/// Windows `TIME_ZONE_INFORMATION.Bias` is UTC = local + Bias.
#[must_use]
pub fn local_ymd(unix_ms: u64, bias_minutes: i32) -> (i32, u8, u8) {
    let local_ms = unix_ms as i64 - i64::from(bias_minutes) * 60_000;
    let days = local_ms.div_euclid(86_400_000);
    days_to_ymd(days)
}

#[must_use]
pub fn ymd_key(year: i32, month: u8, day: u8) -> u32 {
    (year as u32) * 10_000 + u32::from(month) * 100 + u32::from(day)
}

#[must_use]
pub fn local_hms(unix_ms: u64, bias_minutes: i32) -> (u8, u8) {
    let local_ms = unix_ms as i64 - i64::from(bias_minutes) * 60_000;
    let seconds = local_ms.div_euclid(1_000).rem_euclid(86_400);
    ((seconds / 3_600) as u8, ((seconds % 3_600) / 60) as u8)
}

/// Howard Hinnant's civil-from-days. `days` is the count since 1970-01-01.
#[must_use]
pub fn days_to_ymd(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u8;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::{
        cost_cents, days_to_ymd, format_banked_reset_label, format_compact_token_count,
        format_fable_limit_label, format_limit_label, format_plan_label, is_long_context_request,
        local_ymd, parse_rfc3339_ms, resolve_codex_model, windows_tz_bias_minutes, ymd_iso,
        ymd_key, LimitWindow, ProviderUsage, TokenUsage,
    };

    #[test]
    fn component_compact_token_counts_use_decimal_si_suffixes() {
        assert_eq!(format_compact_token_count(0), "0");
        assert_eq!(format_compact_token_count(999), "999");
        assert_eq!(format_compact_token_count(1_500), "1.5K");
        assert_eq!(format_compact_token_count(2_500_000), "2.5M");
        assert_eq!(format_compact_token_count(3_400_000_000), "3.4B");
    }

    #[test]
    fn component_processed_token_totals_are_measurement_not_pricing_class() {
        let usage = TokenUsage {
            input: 10,
            cached_input: 2,
            cache_read: 3,
            cache_write_5m: 4,
            cache_write_1h: 5,
            output: 7,
            long_context_input: 10,
            long_context_cached_input: 2,
            long_context_output: 7,
        };
        assert_eq!(usage.processed_input_tokens(), 24);
        assert_eq!(usage.processed_output_tokens(), 7);
        let priced = TokenUsage {
            input: 1_000_000,
            output: 1_000_000,
            long_context_input: 1_000_000,
            long_context_output: 1_000_000,
            ..TokenUsage::default()
        };
        let classified = cost_cents("gpt-5.4", priced, None).expect("priced");
        let measurement_only = cost_cents(
            "gpt-5.4",
            TokenUsage {
                long_context_input: 0,
                long_context_output: 0,
                ..priced
            },
            None,
        )
        .expect("base");
        assert!(
            classified > measurement_only,
            "long-context copies still apply the pricing premium"
        );
    }

    proptest::proptest! {
        #[test]
        fn pbt_processed_tokens_ignore_long_context_copies(
            input in 0u64..50_000u64,
            cached in 0u64..50_000u64,
            output in 0u64..10_000u64,
        ) {
            let usage = TokenUsage {
                input,
                cached_input: cached,
                output,
                long_context_input: input,
                long_context_cached_input: cached,
                long_context_output: output,
                ..TokenUsage::default()
            };
            proptest::prop_assert_eq!(usage.processed_input_tokens(), input + cached);
            proptest::prop_assert_eq!(usage.processed_output_tokens(), output);
        }
    }

    #[test]
    fn component_opus_standard_request_matches_published_rates() {
        let usage = TokenUsage {
            input: 1_000_000,
            output: 1_000_000,
            ..TokenUsage::default()
        };
        assert_eq!(cost_cents("claude-opus-5", usage, None), Some(3_000));
        assert_eq!(cost_cents("claude-fable-5", usage, None), Some(6_000));
        assert_eq!(cost_cents("claude-fable-5-1", usage, None), Some(6_000));
        assert_eq!(cost_cents("claude-mythos-5-1", usage, None), Some(6_000));
        assert_eq!(cost_cents("claude-opus-5-fast", usage, None), Some(6_000));
        assert_eq!(
            cost_cents("claude-opus-5-20260120-fast", usage, None),
            Some(6_000)
        );
        assert_eq!(
            cost_cents("claude-opus-4-6-fast", usage, None),
            Some(18_000)
        );
        assert_eq!(cost_cents("claude-opus-4-7", usage, None), Some(3_000));
        assert_eq!(cost_cents("claude-sonnet-5", usage, None), Some(1_200));
        assert_eq!(
            cost_cents("claude-sonnet-5", usage, Some("2026-09-01")),
            Some(1_800)
        );
        assert_eq!(cost_cents("gpt-5.6-sol", usage, None), Some(3_500));
        assert_eq!(
            cost_cents("gpt-5.6-sol", usage, Some("2026-08-20")),
            Some(3_500)
        );
        assert_eq!(
            cost_cents("gpt-5.6-sol", usage, Some("2026-08-21")),
            Some(2_400)
        );
        assert_eq!(cost_cents("gpt-5.6-terra", usage, None), Some(1_750));
        assert_eq!(
            cost_cents("gpt-5.6-terra", usage, Some("2026-08-19")),
            Some(1_400)
        );
        assert_eq!(resolve_codex_model("codex-auto-review"), "gpt-5.4");
        assert!(is_long_context_request("gpt-5.4", 272_001));
        assert!(!is_long_context_request("gpt-5.4", 272_000));
    }

    #[test]
    fn component_fable_51_cache_hits_use_quarter_of_fable_5_rate() {
        let cache_hits = TokenUsage {
            cache_read: 1_000_000,
            ..TokenUsage::default()
        };
        assert_eq!(cost_cents("claude-fable-5", cache_hits, None), Some(100));
        assert_eq!(cost_cents("claude-mythos-5", cache_hits, None), Some(100));
        assert_eq!(cost_cents("claude-fable-5-1", cache_hits, None), Some(25));
        assert_eq!(cost_cents("claude-mythos-5-1", cache_hits, None), Some(25));
        assert_eq!(
            cost_cents("claude-fable-5-1-20260901", cache_hits, None),
            Some(25)
        );
        let cache_write = TokenUsage {
            cache_write_5m: 1_000_000,
            cache_write_1h: 1_000_000,
            ..TokenUsage::default()
        };
        assert_eq!(
            cost_cents("claude-fable-5-1", cache_write, None),
            Some(3_250)
        );
        assert_eq!(cost_cents("claude-fable-5", cache_write, None), Some(3_250));
    }

    #[test]
    fn component_unknown_model_has_no_api_equivalent_cost() {
        assert_eq!(
            cost_cents("mystery-model", TokenUsage::default(), None),
            None
        );
    }

    #[test]
    fn component_expired_limit_window_reads_as_unused() {
        let window = LimitWindow {
            used_tenths: 280,
            resets_at_ms: 1_000,
            window_minutes: 300,
        };
        assert_eq!(window.effective(999).used_tenths, 280);
        assert_eq!(window.effective(1_000).used_tenths, 0);
    }

    #[test]
    fn component_fable_label_does_not_fabricate_zero_after_reset() {
        let window = LimitWindow {
            used_tenths: 410,
            resets_at_ms: 1_000,
            window_minutes: 10_080,
        };
        assert_eq!(format_fable_limit_label(window, 999), "Fable: 41%");
        assert_eq!(format_fable_limit_label(window, 1_000), "Fable: —");
        let unknown = LimitWindow {
            used_tenths: 410,
            resets_at_ms: 0,
            window_minutes: 10_080,
        };
        assert_eq!(format_fable_limit_label(unknown, 1), "Fable: —");
        assert!(!format_fable_limit_label(window, 1_000).contains("0%"));
        assert!(!format_fable_limit_label(unknown, 1).contains("0%"));
        assert_eq!(format_limit_label("5h", Some(window), 999), "5h: 41%");
        assert_eq!(format_limit_label("5h", Some(window), 1_000), "5h: —");
        assert_eq!(format_limit_label("7d", Some(unknown), 1), "7d: —");
        assert_eq!(format_limit_label("5h", None, 1), "5h: —");
        assert_ne!(format_limit_label("5h", Some(window), 1_000), "5h: 0%");
        assert_ne!(format_limit_label("7d", Some(window), 1_000), "7d: 0%");
        assert_eq!(
            format_limit_label("Fable", Some(window), 1_000),
            format_fable_limit_label(window, 1_000)
        );
    }

    #[test]
    fn component_banked_reset_label_hides_absent_and_zero() {
        assert_eq!(format_banked_reset_label(None), None);
        assert_eq!(format_banked_reset_label(Some(0)), None);
        assert_eq!(
            format_banked_reset_label(Some(2)).as_deref(),
            Some("Banked Reset: 2")
        );
    }

    #[test]
    fn component_rfc3339_offsets_are_utc_instants_not_wall_clock() {
        let z = parse_rfc3339_ms("2026-08-16T01:02:03Z").expect("Z");
        let plus = parse_rfc3339_ms("2026-08-16T10:02:03+09:00").expect("+09");
        let minus = parse_rfc3339_ms("2026-08-15T17:02:03-08:00").expect("-08");
        let compact = parse_rfc3339_ms("2026-08-16T10:02:03+0900").expect("compact");
        assert_eq!(z, plus);
        assert_eq!(z, minus);
        assert_eq!(z, compact);
        // Fail-first: the old parser treated +09:00 civil digits as UTC.
        assert_ne!(
            plus,
            parse_rfc3339_ms("2026-08-16T10:02:03Z").expect("wall Z")
        );
        assert!(parse_rfc3339_ms("2026-08-16T01:02:03").is_none());
        assert!(parse_rfc3339_ms("not-a-timestamp-at-all!!").is_none());
        assert!(parse_rfc3339_ms("2026-08-16 01:02:03Z").is_none());
    }

    #[test]
    fn component_rfc3339_month_boundary_keeps_utc_then_local_bucket() {
        let utc = parse_rfc3339_ms("2026-08-31T23:30:00+09:00").expect("boundary");
        assert_eq!(local_ymd(utc, -540), (2026, 8, 31));
        assert_eq!(local_ymd(utc, 0), (2026, 8, 31));
        let usage = TokenUsage {
            input: 1_000_000,
            output: 1_000_000,
            ..TokenUsage::default()
        };
        assert_eq!(
            cost_cents("claude-sonnet-5", usage, Some(&ymd_iso(2026, 8, 31))),
            Some(1_200)
        );
        assert_eq!(
            cost_cents("claude-sonnet-5", usage, Some(&ymd_iso(2026, 9, 1))),
            Some(1_800)
        );
    }

    #[test]
    fn component_windows_tz_bias_uses_standard_or_daylight_addend() {
        // US Pacific: Bias=480, StandardBias=0, DaylightBias=-60
        assert_eq!(windows_tz_bias_minutes(480, 0, -60, 1), 480);
        assert_eq!(windows_tz_bias_minutes(480, 0, -60, 2), 420);
        assert_eq!(windows_tz_bias_minutes(480, 0, -60, 0), 480);
    }

    #[test]
    fn component_ambiguous_and_nonexistent_local_without_offset_are_rejected() {
        assert!(parse_rfc3339_ms("2026-11-01T01:30:00").is_none());
        assert!(parse_rfc3339_ms("2026-03-08T02:30:00").is_none());
    }

    #[test]
    fn component_civil_dates_cover_epoch_leap_and_timezone_bias() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
        assert_eq!(days_to_ymd(19_782), (2024, 2, 29));
        assert_eq!(local_ymd(0, -540), (1970, 1, 1));
        assert_eq!(local_ymd(0, 0), (1970, 1, 1));
        assert_eq!(ymd_key(2026, 8, 16), 20_260_816);
    }

    #[test]
    fn component_plan_labels_capitalise_and_keep_rate_multipliers() {
        assert_eq!(format_plan_label("default_claude_max_20x"), "Max 20x");
        assert_eq!(format_plan_label("max"), "Max");
        assert_eq!(format_plan_label("pro"), "Pro 20x");
        assert_eq!(format_plan_label("plus"), "Plus");
    }

    #[test]
    fn component_month_activity_requires_this_month_cost() {
        assert!(!ProviderUsage::default().has_month_activity());
        assert!(ProviderUsage {
            month_cents: 1,
            ..ProviderUsage::default()
        }
        .has_month_activity());
        assert!(ProviderUsage {
            today_cents: 1,
            ..ProviderUsage::default()
        }
        .has_month_activity());
        assert!(!ProviderUsage {
            primary: Some(LimitWindow {
                used_tenths: 250,
                resets_at_ms: 0,
                window_minutes: 300,
            }),
            ..ProviderUsage::default()
        }
        .has_month_activity());
    }
}
