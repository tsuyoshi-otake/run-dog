use core::fmt;

use super::LimitWindow;

/// Compact tray text for a numeric display mode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrayGlyph {
    pub tag: &'static str,
    pub value: String,
}

/// Sampled percentages used to build a [`TrayGlyph`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrayMetrics {
    pub cpu_percent: Option<f32>,
    pub memory_percent: Option<f32>,
    pub gpu_percent: Option<f32>,
    pub claude_session: Option<LimitWindow>,
    pub claude_week: Option<LimitWindow>,
    pub claude_fable: Option<LimitWindow>,
    pub codex_session: Option<LimitWindow>,
    pub codex_week: Option<LimitWindow>,
}

/// What the notification-area icon shows.
///
/// The dog animation is the default. Numeric modes replace the frames with a
/// compact percentage so CPU, memory, GPU, or a live rate-limit window can sit
/// in the tray without opening the hover card.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum TrayDisplayMode {
    #[default]
    Dog,
    Cpu,
    Memory,
    Gpu,
    Claude5h,
    ClaudeWeek,
    FableWeek,
    Codex5h,
    CodexWeek,
}

impl TrayDisplayMode {
    pub const ALL: [Self; 9] = [
        Self::Dog,
        Self::Cpu,
        Self::Memory,
        Self::Gpu,
        Self::Claude5h,
        Self::ClaudeWeek,
        Self::FableWeek,
        Self::Codex5h,
        Self::CodexWeek,
    ];

    #[must_use]
    pub const fn persisted_name(self) -> &'static str {
        match self {
            Self::Dog => "dog",
            Self::Cpu => "cpu",
            Self::Memory => "memory",
            Self::Gpu => "gpu",
            Self::Claude5h => "claude_5h",
            Self::ClaudeWeek => "claude_week",
            Self::FableWeek => "fable_week",
            Self::Codex5h => "codex_5h",
            Self::CodexWeek => "codex_week",
        }
    }

    #[must_use]
    pub fn parse_persisted(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "dog" => Some(Self::Dog),
            "cpu" => Some(Self::Cpu),
            "memory" => Some(Self::Memory),
            "gpu" => Some(Self::Gpu),
            "claude_5h" => Some(Self::Claude5h),
            "claude_week" => Some(Self::ClaudeWeek),
            "fable_week" => Some(Self::FableWeek),
            "codex_5h" => Some(Self::Codex5h),
            "codex_week" => Some(Self::CodexWeek),
            _ => None,
        }
    }

    /// Only the dog runner advances tray frames.
    #[must_use]
    pub const fn uses_animation(self) -> bool {
        matches!(self, Self::Dog)
    }

    #[must_use]
    pub const fn glyph_tag(self) -> Option<&'static str> {
        match self {
            Self::Dog => None,
            Self::Cpu => Some("CPU"),
            Self::Memory => Some("MEM"),
            Self::Gpu => Some("GPU"),
            Self::Claude5h => Some("C5H"),
            Self::ClaudeWeek => Some("C7D"),
            Self::FableWeek => Some("FAB"),
            Self::Codex5h => Some("X5H"),
            Self::CodexWeek => Some("X7D"),
        }
    }

    #[must_use]
    pub const fn limit_window(self, metrics: TrayMetrics) -> Option<LimitWindow> {
        match self {
            Self::Claude5h => metrics.claude_session,
            Self::ClaudeWeek => metrics.claude_week,
            Self::FableWeek => metrics.claude_fable,
            Self::Codex5h => metrics.codex_session,
            Self::CodexWeek => metrics.codex_week,
            Self::Dog | Self::Cpu | Self::Memory | Self::Gpu => None,
        }
    }
}

/// Integer percent for a 32×32 tray icon. Missing or non-finite values are `--`.
#[must_use]
pub fn format_tray_percent(value: Option<f32>) -> String {
    match value {
        Some(percent) if percent.is_finite() => {
            format!("{}", percent.round().clamp(0.0, 100.0) as u8)
        }
        _ => "--".to_owned(),
    }
}

/// Live Codex week used-percent, or `--` when the window is missing or stale.
///
/// Same freshness contract as the hover card: expired / unknown reset times
/// must not be painted as 0%.
#[must_use]
pub fn format_tray_limit(window: Option<LimitWindow>, now_ms: u64) -> String {
    match window {
        Some(window) if window.is_current(now_ms) => {
            format_tray_percent(Some(window.used_percent()))
        }
        _ => "--".to_owned(),
    }
}

#[must_use]
pub fn format_tray_glyph(
    mode: TrayDisplayMode,
    metrics: TrayMetrics,
    now_ms: u64,
) -> Option<TrayGlyph> {
    let tag = mode.glyph_tag()?;
    let value = match mode {
        TrayDisplayMode::Dog => return None,
        TrayDisplayMode::Cpu => format_tray_percent(metrics.cpu_percent),
        TrayDisplayMode::Memory => format_tray_percent(metrics.memory_percent),
        TrayDisplayMode::Gpu => format_tray_percent(metrics.gpu_percent),
        TrayDisplayMode::Claude5h
        | TrayDisplayMode::ClaudeWeek
        | TrayDisplayMode::FableWeek
        | TrayDisplayMode::Codex5h
        | TrayDisplayMode::CodexWeek => format_tray_limit(mode.limit_window(metrics), now_ms),
    };
    Some(TrayGlyph { tag, value })
}

impl fmt::Display for TrayDisplayMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.persisted_name())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        format_tray_glyph, format_tray_limit, format_tray_percent, TrayDisplayMode, TrayMetrics,
    };
    use crate::core::LimitWindow;
    use proptest::prelude::*;

    #[test]
    fn c2_parse_persisted_covers_each_recognised_and_rejected_value() {
        assert_eq!(
            TrayDisplayMode::parse_persisted("dog"),
            Some(TrayDisplayMode::Dog)
        );
        assert_eq!(
            TrayDisplayMode::parse_persisted(" CPU "),
            Some(TrayDisplayMode::Cpu)
        );
        assert_eq!(
            TrayDisplayMode::parse_persisted("Memory"),
            Some(TrayDisplayMode::Memory)
        );
        assert_eq!(
            TrayDisplayMode::parse_persisted("gpu"),
            Some(TrayDisplayMode::Gpu)
        );
        assert_eq!(
            TrayDisplayMode::parse_persisted("CODEX_WEEK"),
            Some(TrayDisplayMode::CodexWeek)
        );
        assert_eq!(
            TrayDisplayMode::parse_persisted("claude_5h"),
            Some(TrayDisplayMode::Claude5h)
        );
        assert_eq!(
            TrayDisplayMode::parse_persisted("fable_week"),
            Some(TrayDisplayMode::FableWeek)
        );
        assert_eq!(TrayDisplayMode::parse_persisted("cat"), None);
        assert_eq!(TrayDisplayMode::parse_persisted(""), None);
    }

    #[test]
    fn c2_only_the_dog_mode_advances_tray_frames() {
        for mode in TrayDisplayMode::ALL {
            assert_eq!(mode.uses_animation(), mode == TrayDisplayMode::Dog);
        }
    }

    #[test]
    fn c2_tray_percent_covers_missing_non_finite_and_rounded_bounds() {
        assert_eq!(format_tray_percent(None), "--");
        assert_eq!(format_tray_percent(Some(f32::NAN)), "--");
        assert_eq!(format_tray_percent(Some(f32::INFINITY)), "--");
        assert_eq!(format_tray_percent(Some(-1.0)), "0");
        assert_eq!(format_tray_percent(Some(0.0)), "0");
        assert_eq!(format_tray_percent(Some(49.4)), "49");
        assert_eq!(format_tray_percent(Some(49.5)), "50");
        assert_eq!(format_tray_percent(Some(100.0)), "100");
        assert_eq!(format_tray_percent(Some(140.0)), "100");
    }

    #[test]
    fn c2_tray_limit_shows_live_percent_and_dashes_stale_windows() {
        let live = LimitWindow {
            used_tenths: 732,
            resets_at_ms: 2_000,
            window_minutes: 10_080,
        };
        let expired = LimitWindow {
            used_tenths: 100,
            resets_at_ms: 500,
            window_minutes: 10_080,
        };
        let unknown = LimitWindow {
            used_tenths: 250,
            resets_at_ms: 0,
            window_minutes: 10_080,
        };
        assert_eq!(format_tray_limit(Some(live), 1_000), "73");
        assert_eq!(format_tray_limit(Some(expired), 1_000), "--");
        assert_eq!(format_tray_limit(Some(unknown), 1_000), "--");
        assert_eq!(format_tray_limit(None, 1_000), "--");
    }

    #[test]
    fn c2_tray_glyph_covers_dog_and_each_numeric_source() {
        assert_eq!(
            format_tray_glyph(TrayDisplayMode::Dog, TrayMetrics::default(), 0),
            None
        );
        let live = LimitWindow {
            used_tenths: 410,
            resets_at_ms: 9_000,
            window_minutes: 10_080,
        };
        let session = LimitWindow {
            used_tenths: 180,
            resets_at_ms: 9_000,
            window_minutes: 300,
        };
        let metrics = TrayMetrics {
            cpu_percent: Some(12.2),
            memory_percent: Some(88.8),
            gpu_percent: Some(3.0),
            claude_session: Some(session),
            claude_week: Some(LimitWindow {
                used_tenths: 620,
                resets_at_ms: 9_000,
                window_minutes: 10_080,
            }),
            claude_fable: Some(LimitWindow {
                used_tenths: 275,
                resets_at_ms: 9_000,
                window_minutes: 10_080,
            }),
            codex_session: Some(LimitWindow {
                used_tenths: 90,
                resets_at_ms: 9_000,
                window_minutes: 300,
            }),
            codex_week: Some(live),
        };
        assert_eq!(
            format_tray_glyph(TrayDisplayMode::Cpu, metrics, 1_000)
                .map(|glyph| (glyph.tag, glyph.value)),
            Some(("CPU", "12".to_owned()))
        );
        assert_eq!(
            format_tray_glyph(TrayDisplayMode::Memory, metrics, 1_000)
                .map(|glyph| (glyph.tag, glyph.value)),
            Some(("MEM", "89".to_owned()))
        );
        assert_eq!(
            format_tray_glyph(TrayDisplayMode::Gpu, metrics, 1_000)
                .map(|glyph| (glyph.tag, glyph.value)),
            Some(("GPU", "3".to_owned()))
        );
        assert_eq!(
            format_tray_glyph(TrayDisplayMode::Claude5h, metrics, 1_000)
                .map(|glyph| (glyph.tag, glyph.value)),
            Some(("C5H", "18".to_owned()))
        );
        assert_eq!(
            format_tray_glyph(TrayDisplayMode::ClaudeWeek, metrics, 1_000)
                .map(|glyph| (glyph.tag, glyph.value)),
            Some(("C7D", "62".to_owned()))
        );
        assert_eq!(
            format_tray_glyph(TrayDisplayMode::FableWeek, metrics, 1_000)
                .map(|glyph| (glyph.tag, glyph.value)),
            Some(("FAB", "28".to_owned()))
        );
        assert_eq!(
            format_tray_glyph(TrayDisplayMode::Codex5h, metrics, 1_000)
                .map(|glyph| (glyph.tag, glyph.value)),
            Some(("X5H", "9".to_owned()))
        );
        assert_eq!(
            format_tray_glyph(TrayDisplayMode::CodexWeek, metrics, 1_000)
                .map(|glyph| (glyph.tag, glyph.value)),
            Some(("X7D", "41".to_owned()))
        );
        assert_eq!(
            format_tray_glyph(TrayDisplayMode::Cpu, TrayMetrics::default(), 0)
                .map(|glyph| glyph.value),
            Some("--".to_owned())
        );
    }

    proptest! {
        #[test]
        fn pbt_parse_round_trip_for_all_valid_display_modes(index in 0usize..9) {
            let mode = TrayDisplayMode::ALL[index];
            prop_assert_eq!(
                TrayDisplayMode::parse_persisted(mode.persisted_name()),
                Some(mode)
            );
        }
    }
}
