use core::fmt;

/// What the notification-area icon shows.
///
/// The dog animation is the default. Numeric modes replace the frames with a
/// compact percentage so CPU, memory, GPU, or the Codex weekly limit can sit
/// in the tray without opening the hover card.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum TrayDisplayMode {
    #[default]
    Dog,
    Cpu,
    Memory,
    Gpu,
    CodexWeek,
}

impl TrayDisplayMode {
    pub const ALL: [Self; 5] = [
        Self::Dog,
        Self::Cpu,
        Self::Memory,
        Self::Gpu,
        Self::CodexWeek,
    ];

    #[must_use]
    pub const fn persisted_name(self) -> &'static str {
        match self {
            Self::Dog => "dog",
            Self::Cpu => "cpu",
            Self::Memory => "memory",
            Self::Gpu => "gpu",
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
            "codex_week" => Some(Self::CodexWeek),
            _ => None,
        }
    }

    /// Only the dog runner advances tray frames.
    #[must_use]
    pub const fn uses_animation(self) -> bool {
        matches!(self, Self::Dog)
    }
}

impl fmt::Display for TrayDisplayMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.persisted_name())
    }
}

#[cfg(test)]
mod tests {
    use super::TrayDisplayMode;
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
        assert_eq!(TrayDisplayMode::parse_persisted("cat"), None);
        assert_eq!(TrayDisplayMode::parse_persisted(""), None);
    }

    #[test]
    fn c2_only_the_dog_mode_advances_tray_frames() {
        assert!(TrayDisplayMode::Dog.uses_animation());
        assert!(!TrayDisplayMode::Cpu.uses_animation());
        assert!(!TrayDisplayMode::Memory.uses_animation());
        assert!(!TrayDisplayMode::Gpu.uses_animation());
        assert!(!TrayDisplayMode::CodexWeek.uses_animation());
    }

    proptest! {
        #[test]
        fn pbt_parse_round_trip_for_all_valid_display_modes(index in 0usize..5) {
            let mode = TrayDisplayMode::ALL[index];
            prop_assert_eq!(
                TrayDisplayMode::parse_persisted(mode.persisted_name()),
                Some(mode)
            );
        }
    }
}
