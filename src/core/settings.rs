use super::{FpsLimit, ThemePreference};

/// Persisted user settings. Every field has a safe default so a malformed
/// registry value can never prevent the tray process from starting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppSettings {
    pub theme: ThemePreference,
    pub fps_limit: FpsLimit,
    pub launch_at_startup: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemePreference::System,
            fps_limit: FpsLimit::default(),
            launch_at_startup: false,
        }
    }
}

impl AppSettings {
    #[must_use]
    pub fn from_persisted(
        theme: Option<&str>,
        fps_limit: Option<&str>,
        launch_at_startup: Option<bool>,
    ) -> Self {
        Self {
            theme: theme
                .and_then(ThemePreference::parse_persisted)
                .unwrap_or_default(),
            fps_limit: fps_limit
                .and_then(FpsLimit::parse_persisted)
                .unwrap_or_default(),
            launch_at_startup: launch_at_startup.unwrap_or(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppSettings;
    use crate::core::{FpsLimit, ThemePreference};
    use proptest::prelude::*;

    #[test]
    fn c2_persisted_settings_cover_valid_and_invalid_optional_values() {
        assert_eq!(
            AppSettings::from_persisted(None, None, None),
            AppSettings::default()
        );
        assert_eq!(
            AppSettings::from_persisted(Some("dark"), Some("30"), Some(true)),
            AppSettings {
                theme: ThemePreference::Dark,
                fps_limit: FpsLimit::Fps30,
                launch_at_startup: true,
            }
        );
        assert_eq!(
            AppSettings::from_persisted(Some("invalid"), Some("999"), Some(false)),
            AppSettings::default()
        );
    }

    proptest! {
        #[test]
        fn pbt_defaults_are_stable_for_unknown_persisted_strings(
            theme in "[^\\x00]{0,40}",
            fps in "[^\\x00]{0,40}",
            startup in any::<bool>(),
        ) {
            let settings = AppSettings::from_persisted(Some(&theme), Some(&fps), Some(startup));
            prop_assert!(ThemePreference::ALL.contains(&settings.theme));
            prop_assert!(FpsLimit::ALL.contains(&settings.fps_limit));
            prop_assert_eq!(settings.launch_at_startup, startup);
        }
    }
}
