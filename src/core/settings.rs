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

/// One durable configuration generation. Readers observe either a complete
/// record or fall back to defaults; partial field tuples are not representable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsRecord {
    pub generation: u64,
    pub last_operation_id: u64,
    pub settings: AppSettings,
}

const RECORD_HEADER_V1: &str = "rundog-settings-1";
const RECORD_HEADER_V2: &str = "rundog-settings-2";

impl SettingsRecord {
    #[must_use]
    pub const fn new(generation: u64, last_operation_id: u64, settings: AppSettings) -> Self {
        Self {
            generation,
            last_operation_id,
            settings,
        }
    }

    #[must_use]
    pub fn encode(self) -> String {
        format!(
            "{RECORD_HEADER_V2}\ngeneration={}\noperation_id={}\ntheme={}\nfps={}\nstartup={}\n",
            self.generation,
            self.last_operation_id,
            self.settings.theme.persisted_name(),
            self.settings.fps_limit.persisted_name(),
            u8::from(self.settings.launch_at_startup),
        )
    }

    #[must_use]
    pub fn decode(payload: &str) -> Option<Self> {
        let mut lines = payload.lines();
        let header = lines.next()?;
        match header {
            RECORD_HEADER_V2 => {
                let generation = parse_field(lines.next()?, "generation")?.parse().ok()?;
                let last_operation_id = parse_field(lines.next()?, "operation_id")?.parse().ok()?;
                let theme = ThemePreference::parse_persisted(parse_field(lines.next()?, "theme")?)?;
                let fps_limit = FpsLimit::parse_persisted(parse_field(lines.next()?, "fps")?)?;
                let startup = parse_bool01(parse_field(lines.next()?, "startup")?)?;
                if lines.next().is_some() {
                    return None;
                }
                Some(Self {
                    generation,
                    last_operation_id,
                    settings: AppSettings {
                        theme,
                        fps_limit,
                        launch_at_startup: startup,
                    },
                })
            }
            RECORD_HEADER_V1 => {
                let generation = parse_field(lines.next()?, "generation")?.parse().ok()?;
                let theme = ThemePreference::parse_persisted(parse_field(lines.next()?, "theme")?)?;
                let fps_limit = FpsLimit::parse_persisted(parse_field(lines.next()?, "fps")?)?;
                let startup = parse_bool01(parse_field(lines.next()?, "startup")?)?;
                if lines.next().is_some() {
                    return None;
                }
                Some(Self {
                    generation,
                    last_operation_id: 0,
                    settings: AppSettings {
                        theme,
                        fps_limit,
                        launch_at_startup: startup,
                    },
                })
            }
            _ => None,
        }
    }
}

/// Durable in-flight Run/settings saga. Crash recovery finishes or rolls back
/// using this journal before the tray loop accepts new commits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingJournal {
    pub operation_id: u64,
    pub base_generation: u64,
    pub desired: AppSettings,
    pub previous: AppSettings,
    pub sync_run_entry: bool,
    pub deadline_millis: u64,
}

const PENDING_HEADER: &str = "rundog-pending-1";

impl PendingJournal {
    #[must_use]
    pub fn encode(self) -> String {
        format!(
            "{PENDING_HEADER}\noperation_id={}\nbase_generation={}\nsync_run={}\ndeadline={}\ndesired_theme={}\ndesired_fps={}\ndesired_startup={}\nprevious_theme={}\nprevious_fps={}\nprevious_startup={}\n",
            self.operation_id,
            self.base_generation,
            u8::from(self.sync_run_entry),
            self.deadline_millis,
            self.desired.theme.persisted_name(),
            self.desired.fps_limit.persisted_name(),
            u8::from(self.desired.launch_at_startup),
            self.previous.theme.persisted_name(),
            self.previous.fps_limit.persisted_name(),
            u8::from(self.previous.launch_at_startup),
        )
    }

    #[must_use]
    pub fn decode(payload: &str) -> Option<Self> {
        let mut lines = payload.lines();
        if lines.next()? != PENDING_HEADER {
            return None;
        }
        let operation_id = parse_field(lines.next()?, "operation_id")?.parse().ok()?;
        let base_generation = parse_field(lines.next()?, "base_generation")?
            .parse()
            .ok()?;
        let sync_run_entry = parse_bool01(parse_field(lines.next()?, "sync_run")?)?;
        let deadline_millis = parse_field(lines.next()?, "deadline")?.parse().ok()?;
        let desired = AppSettings {
            theme: ThemePreference::parse_persisted(parse_field(lines.next()?, "desired_theme")?)?,
            fps_limit: FpsLimit::parse_persisted(parse_field(lines.next()?, "desired_fps")?)?,
            launch_at_startup: parse_bool01(parse_field(lines.next()?, "desired_startup")?)?,
        };
        let previous = AppSettings {
            theme: ThemePreference::parse_persisted(parse_field(lines.next()?, "previous_theme")?)?,
            fps_limit: FpsLimit::parse_persisted(parse_field(lines.next()?, "previous_fps")?)?,
            launch_at_startup: parse_bool01(parse_field(lines.next()?, "previous_startup")?)?,
        };
        if lines.next().is_some() {
            return None;
        }
        Some(Self {
            operation_id,
            base_generation,
            desired,
            previous,
            sync_run_entry,
            deadline_millis,
        })
    }
}

fn parse_field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let (key, value) = line.split_once('=')?;
    (key == name).then_some(value)
}

fn parse_bool01(value: &str) -> Option<bool> {
    match value {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{AppSettings, PendingJournal, SettingsRecord};
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

    #[test]
    fn c2_settings_record_round_trips_v2_and_accepts_v1_upgrade() {
        let record = SettingsRecord::new(
            7,
            11,
            AppSettings {
                theme: ThemePreference::Light,
                fps_limit: FpsLimit::Fps40,
                launch_at_startup: true,
            },
        );
        assert_eq!(SettingsRecord::decode(&record.encode()), Some(record));

        let legacy = "rundog-settings-1\ngeneration=3\ntheme=dark\nfps=20\nstartup=0\n";
        assert_eq!(
            SettingsRecord::decode(legacy),
            Some(SettingsRecord::new(
                3,
                0,
                AppSettings {
                    theme: ThemePreference::Dark,
                    fps_limit: FpsLimit::Fps20,
                    launch_at_startup: false,
                }
            ))
        );
        assert_eq!(
            SettingsRecord::decode("rundog-settings-2\ngeneration=1\n"),
            None
        );
    }

    #[test]
    fn c2_pending_journal_round_trips() {
        let journal = PendingJournal {
            operation_id: 9,
            base_generation: 4,
            desired: AppSettings {
                theme: ThemePreference::Dark,
                fps_limit: FpsLimit::Fps30,
                launch_at_startup: true,
            },
            previous: AppSettings::default(),
            sync_run_entry: true,
            deadline_millis: 1_000,
        };
        assert_eq!(PendingJournal::decode(&journal.encode()), Some(journal));
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

        #[test]
        fn pbt_settings_record_encode_decode_is_lossless(
            generation in any::<u64>(),
            operation_id in any::<u64>(),
            theme in prop::sample::select(ThemePreference::ALL.to_vec()),
            fps_limit in prop::sample::select(FpsLimit::ALL.to_vec()),
            launch_at_startup in any::<bool>(),
        ) {
            let record = SettingsRecord::new(
                generation,
                operation_id,
                AppSettings {
                    theme,
                    fps_limit,
                    launch_at_startup,
                },
            );
            prop_assert_eq!(SettingsRecord::decode(&record.encode()), Some(record));
        }
    }
}
