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
    /// All input-side tokens seen while parsing provider JSONL.
    #[must_use]
    pub const fn processed_input_tokens(self) -> u64 {
        self.input
            .saturating_add(self.cached_input)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write_5m)
            .saturating_add(self.cache_write_1h)
            .saturating_add(self.long_context_input)
            .saturating_add(self.long_context_cached_input)
    }

    /// All output-side tokens seen while parsing provider JSONL.
    #[must_use]
    pub const fn processed_output_tokens(self) -> u64 {
        self.output.saturating_add(self.long_context_output)
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

    /// A window whose reset has already passed is treated as unused.
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
}

/// Nanodollars in one display cent. Accumulators stay in nanos; UI rounds once.
pub const NANOS_PER_CENT: u64 = 10_000_000;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProviderUsage {
    pub today_cents: u32,
    pub month_cents: u32,
    /// Unrounded API-equivalent cost. `today_cents` is display-rounded from this.
    pub today_cost_nanos: u64,
    pub month_cost_nanos: u64,
    pub month_input_tokens: u64,
    pub month_output_tokens: u64,
    pub plan: [u8; 16],
    pub plan_len: u8,
    pub primary: Option<LimitWindow>,
    pub secondary: Option<LimitWindow>,
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

    pub fn add_today_nanos(&mut self, nanos: u64) {
        self.today_cost_nanos = self.today_cost_nanos.saturating_add(nanos);
        self.today_cents = display_cents(self.today_cost_nanos);
    }

    pub fn add_month_nanos(&mut self, nanos: u64) {
        self.month_cost_nanos = self.month_cost_nanos.saturating_add(nanos);
        self.month_cents = display_cents(self.month_cost_nanos);
    }

    pub fn clear_today_cost(&mut self) {
        self.today_cost_nanos = 0;
        self.today_cents = 0;
    }

    pub fn clear_month_cost(&mut self) {
        self.month_cost_nanos = 0;
        self.month_cents = 0;
        self.month_input_tokens = 0;
        self.month_output_tokens = 0;
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

/// Half-up display cents from an unrounded nanodollar total.
#[must_use]
pub fn display_cents(nanos: u64) -> u32 {
    let rounded = (u128::from(nanos) + u128::from(NANOS_PER_CENT) / 2) / u128::from(NANOS_PER_CENT);
    u32::try_from(rounded).unwrap_or(u32::MAX)
}

/// Unrounded nanodollars from a per-million-token price table.
///
/// `effective_day` is a `YYYY-MM-DD` local day used for scheduled price
/// revisions, matching otak-usage `calcCost`.
#[must_use]
pub fn cost_nanos(model: &str, usage: TokenUsage, effective_day: Option<&str>) -> Option<u64> {
    let pricing = resolve_pricing(model, effective_day)?;
    let long_input_premium = pricing.long_context_input_multiplier.unwrap_or(1.0) - 1.0;
    let long_output_premium = pricing.long_context_output_multiplier.unwrap_or(1.0) - 1.0;
    let mut total = 0_u128;
    for (tokens, usd_per_mtok) in [
        (usage.input, pricing.input),
        (usage.cached_input, pricing.cached_input),
        (usage.cache_read, pricing.cache_read),
        (usage.cache_write_5m, pricing.cache_write_5m),
        (usage.cache_write_1h, pricing.cache_write_1h),
        (usage.output, pricing.output),
        (usage.long_context_input, pricing.input * long_input_premium),
        (
            usage.long_context_cached_input,
            pricing.cached_input * long_input_premium,
        ),
        (
            usage.long_context_output,
            pricing.output * long_output_premium,
        ),
    ] {
        let nanos_per_token = usd_per_mtok_to_nanos_per_token(usd_per_mtok)?;
        total =
            total.saturating_add(u128::from(tokens).saturating_mul(u128::from(nanos_per_token)));
    }
    u64::try_from(total).ok()
}

/// USD cents from a per-million-token price table. Display-round of one event.
///
/// Do not sum this across events; accumulate [`cost_nanos`] and call
/// [`display_cents`] once.
#[must_use]
pub fn cost_cents(model: &str, usage: TokenUsage, effective_day: Option<&str>) -> Option<u32> {
    Some(display_cents(cost_nanos(model, usage, effective_day)?))
}

/// $1 / 1MTok = 1000 nanodollars per token.
fn usd_per_mtok_to_nanos_per_token(usd_per_mtok: f64) -> Option<u64> {
    if !usd_per_mtok.is_finite() || usd_per_mtok < 0.0 {
        return None;
    }
    let nanos = (usd_per_mtok * 1_000.0).round();
    if !nanos.is_finite() || nanos < 0.0 || nanos > u64::MAX as f64 {
        return None;
    }
    Some(nanos as u64)
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
        cost_cents, cost_nanos, days_to_ymd, display_cents, format_compact_token_count,
        format_plan_label, is_long_context_request, local_ymd, resolve_codex_model, ymd_key,
        LimitWindow, ProviderUsage, TokenUsage,
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
    fn component_processed_token_totals_include_cache_and_long_context_fields() {
        let usage = TokenUsage {
            input: 10,
            cached_input: 2,
            cache_read: 3,
            cache_write_5m: 4,
            cache_write_1h: 5,
            output: 7,
            long_context_input: 11,
            long_context_cached_input: 13,
            long_context_output: 17,
        };
        assert_eq!(usage.processed_input_tokens(), 48);
        assert_eq!(usage.processed_output_tokens(), 24);
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
    fn component_event_level_cent_rounding_matches_batch_and_restart() {
        // Independent oracle: 10_000 opus-5 input tokens at $5/MTok = $0.05 = 5¢.
        let one = TokenUsage {
            input: 1,
            ..TokenUsage::default()
        };
        let batch = TokenUsage {
            input: 10_000,
            ..TokenUsage::default()
        };
        let rounded_sum: u32 = (0..10_000)
            .filter_map(|_| cost_cents("claude-opus-5", one, None))
            .fold(0_u32, u32::saturating_add);
        assert_eq!(
            cost_cents("claude-opus-5", one, None),
            Some(0),
            "a single sub-cent event display-rounds to 0¢"
        );
        assert_eq!(
            rounded_sum, 0,
            "summing display cents loses the batch total (do not aggregate cents)"
        );

        let nanos_sum: u64 = (0..10_000)
            .filter_map(|_| cost_nanos("claude-opus-5", one, None))
            .fold(0_u64, u64::saturating_add);
        let batch_nanos = cost_nanos("claude-opus-5", batch, None).expect("priced");
        assert_eq!(nanos_sum, batch_nanos);
        assert_eq!(display_cents(nanos_sum), 5);
        assert_eq!(cost_cents("claude-opus-5", batch, None), Some(5));

        let mut running = ProviderUsage::default();
        for _ in 0..10_000 {
            running.add_month_nanos(cost_nanos("claude-opus-5", one, None).expect("priced"));
        }
        let mut restored = ProviderUsage {
            month_cost_nanos: running.month_cost_nanos,
            ..ProviderUsage::default()
        };
        restored.add_month_nanos(0);
        assert_eq!(restored.month_cents, running.month_cents);
        assert_eq!(restored.month_cents, 5);
    }

    proptest::proptest! {
        #[test]
        fn pbt_split_events_nanos_match_one_batch(tokens in 1u64..8_000u64, parts in 1u64..64u64) {
            let batch = TokenUsage {
                input: tokens,
                ..TokenUsage::default()
            };
            let batch_nanos = cost_nanos("claude-opus-5", batch, None).expect("priced");
            let base = tokens / parts;
            let rem = tokens % parts;
            let mut split = 0_u64;
            for index in 0..parts {
                let input = base + u64::from(index < rem);
                if input == 0 {
                    continue;
                }
                split = split.saturating_add(
                    cost_nanos(
                        "claude-opus-5",
                        TokenUsage {
                            input,
                            ..TokenUsage::default()
                        },
                        None,
                    )
                    .expect("priced"),
                );
            }
            proptest::prop_assert_eq!(split, batch_nanos);
            proptest::prop_assert_eq!(display_cents(split), display_cents(batch_nanos));
        }
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
