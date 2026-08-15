use core::fmt;

/// User preference for the tray icon appearance.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ThemePreference {
    /// Follow the Windows system colour preference.
    #[default]
    System,
    Light,
    Dark,
}

impl ThemePreference {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    #[must_use]
    pub const fn persisted_name(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    #[must_use]
    pub fn parse_persisted(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "system" => Some(Self::System),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    #[must_use]
    pub const fn resolve(self, system_theme: ResolvedTheme) -> ResolvedTheme {
        match self {
            Self::System => system_theme,
            Self::Light => ResolvedTheme::Light,
            Self::Dark => ResolvedTheme::Dark,
        }
    }
}

impl fmt::Display for ThemePreference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.persisted_name())
    }
}

/// Concrete icon set selected after resolving a [`ThemePreference`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ResolvedTheme {
    Light,
    #[default]
    Dark,
}

impl ResolvedTheme {
    #[must_use]
    pub const fn is_light(self) -> bool {
        matches!(self, Self::Light)
    }
}

#[cfg(test)]
mod tests {
    use super::{ResolvedTheme, ThemePreference};
    use proptest::prelude::*;

    #[test]
    fn c2_parse_persisted_covers_each_recognised_and_rejected_value() {
        assert_eq!(
            ThemePreference::parse_persisted("system"),
            Some(ThemePreference::System)
        );
        assert_eq!(
            ThemePreference::parse_persisted(" LIGHT "),
            Some(ThemePreference::Light)
        );
        assert_eq!(
            ThemePreference::parse_persisted("Dark"),
            Some(ThemePreference::Dark)
        );
        assert_eq!(ThemePreference::parse_persisted("sepia"), None);
    }

    #[test]
    fn c2_system_preference_tracks_both_system_theme_conditions() {
        assert_eq!(
            ThemePreference::System.resolve(ResolvedTheme::Light),
            ResolvedTheme::Light
        );
        assert_eq!(
            ThemePreference::System.resolve(ResolvedTheme::Dark),
            ResolvedTheme::Dark
        );
        assert_eq!(
            ThemePreference::Light.resolve(ResolvedTheme::Dark),
            ResolvedTheme::Light
        );
        assert_eq!(
            ThemePreference::Dark.resolve(ResolvedTheme::Light),
            ResolvedTheme::Dark
        );
    }

    proptest! {
        #[test]
        fn pbt_parse_round_trip_for_all_valid_preferences(index in 0usize..3) {
            let preference = ThemePreference::ALL[index];
            prop_assert_eq!(
                ThemePreference::parse_persisted(preference.persisted_name()),
                Some(preference)
            );
        }
    }
}
