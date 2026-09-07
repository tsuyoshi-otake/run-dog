use super::{FpsLimit, ThemePreference, TrayDisplayMode};

/// Persisted user settings. Every field has a safe default so a malformed
/// registry value can never prevent the tray process from starting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppSettings {
    pub theme: ThemePreference,
    pub fps_limit: FpsLimit,
    pub launch_at_startup: bool,
    pub display_mode: TrayDisplayMode,
    pub auto_update_on_startup: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemePreference::System,
            fps_limit: FpsLimit::default(),
            launch_at_startup: false,
            display_mode: TrayDisplayMode::default(),
            auto_update_on_startup: true,
        }
    }
}

impl AppSettings {
    #[must_use]
    pub fn from_persisted(
        theme: Option<&str>,
        fps_limit: Option<&str>,
        launch_at_startup: Option<bool>,
        display_mode: Option<&str>,
        auto_update_on_startup: Option<bool>,
    ) -> Self {
        Self {
            theme: theme
                .and_then(ThemePreference::parse_persisted)
                .unwrap_or_default(),
            fps_limit: fps_limit
                .and_then(FpsLimit::parse_persisted)
                .unwrap_or_default(),
            launch_at_startup: launch_at_startup.unwrap_or(false),
            display_mode: display_mode
                .and_then(TrayDisplayMode::parse_persisted)
                .unwrap_or_default(),
            auto_update_on_startup: auto_update_on_startup.unwrap_or(true),
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
const RECORD_HEADER_V3: &str = "rundog-settings-3";
const RECORD_HEADER_V4: &str = "rundog-settings-4";

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
            "{RECORD_HEADER_V4}\ngeneration={}\noperation_id={}\ntheme={}\nfps={}\nstartup={}\ndisplay={}\nauto_update={}\n",
            self.generation,
            self.last_operation_id,
            self.settings.theme.persisted_name(),
            self.settings.fps_limit.persisted_name(),
            u8::from(self.settings.launch_at_startup),
            self.settings.display_mode.persisted_name(),
            u8::from(self.settings.auto_update_on_startup),
        )
    }

    #[must_use]
    pub fn decode(payload: &str) -> Option<Self> {
        let mut lines = payload.lines();
        let header = lines.next()?;
        match header {
            RECORD_HEADER_V4 => {
                let generation = parse_field(lines.next()?, "generation")?.parse().ok()?;
                let last_operation_id = parse_field(lines.next()?, "operation_id")?.parse().ok()?;
                let theme = ThemePreference::parse_persisted(parse_field(lines.next()?, "theme")?)?;
                let fps_limit = FpsLimit::parse_persisted(parse_field(lines.next()?, "fps")?)?;
                let startup = parse_bool01(parse_field(lines.next()?, "startup")?)?;
                let display_mode =
                    TrayDisplayMode::parse_persisted(parse_field(lines.next()?, "display")?)?;
                let auto_update = parse_bool01(parse_field(lines.next()?, "auto_update")?)?;
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
                        display_mode,
                        auto_update_on_startup: auto_update,
                    },
                })
            }
            RECORD_HEADER_V3 => {
                let generation = parse_field(lines.next()?, "generation")?.parse().ok()?;
                let last_operation_id = parse_field(lines.next()?, "operation_id")?.parse().ok()?;
                let theme = ThemePreference::parse_persisted(parse_field(lines.next()?, "theme")?)?;
                let fps_limit = FpsLimit::parse_persisted(parse_field(lines.next()?, "fps")?)?;
                let startup = parse_bool01(parse_field(lines.next()?, "startup")?)?;
                let display_mode =
                    TrayDisplayMode::parse_persisted(parse_field(lines.next()?, "display")?)?;
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
                        display_mode,
                        auto_update_on_startup: true,
                    },
                })
            }
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
                        display_mode: TrayDisplayMode::Dog,
                        auto_update_on_startup: true,
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
                        display_mode: TrayDisplayMode::Dog,
                        auto_update_on_startup: true,
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

const PENDING_HEADER_V1: &str = "rundog-pending-1";
const PENDING_HEADER_V2: &str = "rundog-pending-2";
const PENDING_HEADER_V3: &str = "rundog-pending-3";

impl PendingJournal {
    #[must_use]
    pub fn encode(self) -> String {
        format!(
            "{PENDING_HEADER_V3}\noperation_id={}\nbase_generation={}\nsync_run={}\ndeadline={}\ndesired_theme={}\ndesired_fps={}\ndesired_startup={}\ndesired_display={}\ndesired_auto_update={}\nprevious_theme={}\nprevious_fps={}\nprevious_startup={}\nprevious_display={}\nprevious_auto_update={}\n",
            self.operation_id,
            self.base_generation,
            u8::from(self.sync_run_entry),
            self.deadline_millis,
            self.desired.theme.persisted_name(),
            self.desired.fps_limit.persisted_name(),
            u8::from(self.desired.launch_at_startup),
            self.desired.display_mode.persisted_name(),
            u8::from(self.desired.auto_update_on_startup),
            self.previous.theme.persisted_name(),
            self.previous.fps_limit.persisted_name(),
            u8::from(self.previous.launch_at_startup),
            self.previous.display_mode.persisted_name(),
            u8::from(self.previous.auto_update_on_startup),
        )
    }

    #[must_use]
    pub fn decode(payload: &str) -> Option<Self> {
        let mut lines = payload.lines();
        let header = lines.next()?;
        let operation_id = parse_field(lines.next()?, "operation_id")?.parse().ok()?;
        let base_generation = parse_field(lines.next()?, "base_generation")?
            .parse()
            .ok()?;
        let sync_run_entry = parse_bool01(parse_field(lines.next()?, "sync_run")?)?;
        let deadline_millis = parse_field(lines.next()?, "deadline")?.parse().ok()?;
        match header {
            PENDING_HEADER_V3 => {
                let desired = AppSettings {
                    theme: ThemePreference::parse_persisted(parse_field(
                        lines.next()?,
                        "desired_theme",
                    )?)?,
                    fps_limit: FpsLimit::parse_persisted(parse_field(
                        lines.next()?,
                        "desired_fps",
                    )?)?,
                    launch_at_startup: parse_bool01(parse_field(
                        lines.next()?,
                        "desired_startup",
                    )?)?,
                    display_mode: TrayDisplayMode::parse_persisted(parse_field(
                        lines.next()?,
                        "desired_display",
                    )?)?,
                    auto_update_on_startup: parse_bool01(parse_field(
                        lines.next()?,
                        "desired_auto_update",
                    )?)?,
                };
                let previous = AppSettings {
                    theme: ThemePreference::parse_persisted(parse_field(
                        lines.next()?,
                        "previous_theme",
                    )?)?,
                    fps_limit: FpsLimit::parse_persisted(parse_field(
                        lines.next()?,
                        "previous_fps",
                    )?)?,
                    launch_at_startup: parse_bool01(parse_field(
                        lines.next()?,
                        "previous_startup",
                    )?)?,
                    display_mode: TrayDisplayMode::parse_persisted(parse_field(
                        lines.next()?,
                        "previous_display",
                    )?)?,
                    auto_update_on_startup: parse_bool01(parse_field(
                        lines.next()?,
                        "previous_auto_update",
                    )?)?,
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
            PENDING_HEADER_V2 => {
                let desired = AppSettings {
                    theme: ThemePreference::parse_persisted(parse_field(
                        lines.next()?,
                        "desired_theme",
                    )?)?,
                    fps_limit: FpsLimit::parse_persisted(parse_field(
                        lines.next()?,
                        "desired_fps",
                    )?)?,
                    launch_at_startup: parse_bool01(parse_field(
                        lines.next()?,
                        "desired_startup",
                    )?)?,
                    display_mode: TrayDisplayMode::parse_persisted(parse_field(
                        lines.next()?,
                        "desired_display",
                    )?)?,
                    auto_update_on_startup: true,
                };
                let previous = AppSettings {
                    theme: ThemePreference::parse_persisted(parse_field(
                        lines.next()?,
                        "previous_theme",
                    )?)?,
                    fps_limit: FpsLimit::parse_persisted(parse_field(
                        lines.next()?,
                        "previous_fps",
                    )?)?,
                    launch_at_startup: parse_bool01(parse_field(
                        lines.next()?,
                        "previous_startup",
                    )?)?,
                    display_mode: TrayDisplayMode::parse_persisted(parse_field(
                        lines.next()?,
                        "previous_display",
                    )?)?,
                    auto_update_on_startup: true,
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
            PENDING_HEADER_V1 => {
                let desired = AppSettings {
                    theme: ThemePreference::parse_persisted(parse_field(
                        lines.next()?,
                        "desired_theme",
                    )?)?,
                    fps_limit: FpsLimit::parse_persisted(parse_field(
                        lines.next()?,
                        "desired_fps",
                    )?)?,
                    launch_at_startup: parse_bool01(parse_field(
                        lines.next()?,
                        "desired_startup",
                    )?)?,
                    display_mode: TrayDisplayMode::Dog,
                    auto_update_on_startup: true,
                };
                let previous = AppSettings {
                    theme: ThemePreference::parse_persisted(parse_field(
                        lines.next()?,
                        "previous_theme",
                    )?)?,
                    fps_limit: FpsLimit::parse_persisted(parse_field(
                        lines.next()?,
                        "previous_fps",
                    )?)?,
                    launch_at_startup: parse_bool01(parse_field(
                        lines.next()?,
                        "previous_startup",
                    )?)?,
                    display_mode: TrayDisplayMode::Dog,
                    auto_update_on_startup: true,
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
            _ => None,
        }
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
    use crate::core::{FpsLimit, ThemePreference, TrayDisplayMode};
    use proptest::prelude::*;

    #[test]
    fn c2_persisted_settings_cover_valid_and_invalid_optional_values() {
        assert_eq!(
            AppSettings::from_persisted(None, None, None, None, None),
            AppSettings::default()
        );
        assert!(AppSettings::default().auto_update_on_startup);
        assert_eq!(
            AppSettings::from_persisted(
                Some("dark"),
                Some("30"),
                Some(true),
                Some("cpu"),
                Some(false)
            ),
            AppSettings {
                theme: ThemePreference::Dark,
                fps_limit: FpsLimit::Fps30,
                launch_at_startup: true,
                display_mode: TrayDisplayMode::Cpu,
                auto_update_on_startup: false,
            }
        );
        assert_eq!(
            AppSettings::from_persisted(
                Some("invalid"),
                Some("999"),
                Some(false),
                Some("parrot"),
                None
            ),
            AppSettings::default()
        );
    }

    #[test]
    fn c2_settings_record_round_trips_v4_and_accepts_older_upgrades() {
        let record = SettingsRecord::new(
            7,
            11,
            AppSettings {
                theme: ThemePreference::Light,
                fps_limit: FpsLimit::Fps40,
                launch_at_startup: true,
                display_mode: TrayDisplayMode::CodexWeek,
                auto_update_on_startup: false,
            },
        );
        assert_eq!(SettingsRecord::decode(&record.encode()), Some(record));
        assert!(record.encode().starts_with("rundog-settings-4\n"));

        let legacy_v3 = "rundog-settings-3\ngeneration=5\noperation_id=9\ntheme=dark\nfps=20\nstartup=0\ndisplay=cpu\n";
        assert_eq!(
            SettingsRecord::decode(legacy_v3),
            Some(SettingsRecord::new(
                5,
                9,
                AppSettings {
                    theme: ThemePreference::Dark,
                    fps_limit: FpsLimit::Fps20,
                    launch_at_startup: false,
                    display_mode: TrayDisplayMode::Cpu,
                    auto_update_on_startup: true,
                }
            ))
        );

        let legacy_v2 =
            "rundog-settings-2\ngeneration=4\noperation_id=8\ntheme=light\nfps=10\nstartup=1\n";
        assert_eq!(
            SettingsRecord::decode(legacy_v2),
            Some(SettingsRecord::new(
                4,
                8,
                AppSettings {
                    theme: ThemePreference::Light,
                    fps_limit: FpsLimit::Fps10,
                    launch_at_startup: true,
                    display_mode: TrayDisplayMode::Dog,
                    auto_update_on_startup: true,
                }
            ))
        );

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
                    display_mode: TrayDisplayMode::Dog,
                    auto_update_on_startup: true,
                }
            ))
        );
        assert_eq!(
            SettingsRecord::decode("rundog-settings-3\ngeneration=1\n"),
            None
        );
        assert_eq!(
            SettingsRecord::decode("rundog-settings-4\ngeneration=1\n"),
            None
        );
    }

    #[test]
    fn c2_pending_journal_round_trips_and_accepts_v1() {
        let journal = PendingJournal {
            operation_id: 9,
            base_generation: 4,
            desired: AppSettings {
                theme: ThemePreference::Dark,
                fps_limit: FpsLimit::Fps30,
                launch_at_startup: true,
                display_mode: TrayDisplayMode::Memory,
                auto_update_on_startup: false,
            },
            previous: AppSettings::default(),
            sync_run_entry: true,
            deadline_millis: 1_000,
        };
        assert_eq!(PendingJournal::decode(&journal.encode()), Some(journal));

        let legacy = "rundog-pending-1\noperation_id=2\nbase_generation=1\nsync_run=0\ndeadline=50\ndesired_theme=light\ndesired_fps=10\ndesired_startup=1\nprevious_theme=system\nprevious_fps=40\nprevious_startup=0\n";
        assert_eq!(
            PendingJournal::decode(legacy),
            Some(PendingJournal {
                operation_id: 2,
                base_generation: 1,
                desired: AppSettings {
                    theme: ThemePreference::Light,
                    fps_limit: FpsLimit::Fps10,
                    launch_at_startup: true,
                    display_mode: TrayDisplayMode::Dog,
                    auto_update_on_startup: true,
                },
                previous: AppSettings::default(),
                sync_run_entry: false,
                deadline_millis: 50,
            })
        );
    }

    proptest! {
        #[test]
        fn pbt_defaults_are_stable_for_unknown_persisted_strings(
            theme in "[^\\x00]{0,40}",
            fps in "[^\\x00]{0,40}",
            startup in any::<bool>(),
            display in "[^\\x00]{0,40}",
            auto_update in any::<bool>(),
        ) {
            let settings = AppSettings::from_persisted(
                Some(&theme),
                Some(&fps),
                Some(startup),
                Some(&display),
                Some(auto_update),
            );
            prop_assert!(ThemePreference::ALL.contains(&settings.theme));
            prop_assert!(FpsLimit::ALL.contains(&settings.fps_limit));
            prop_assert_eq!(settings.launch_at_startup, startup);
            prop_assert!(TrayDisplayMode::ALL.contains(&settings.display_mode));
            prop_assert_eq!(settings.auto_update_on_startup, auto_update);
        }

        #[test]
        fn pbt_settings_record_encode_decode_is_lossless(
            generation in any::<u64>(),
            operation_id in any::<u64>(),
            theme in prop::sample::select(ThemePreference::ALL.to_vec()),
            fps_limit in prop::sample::select(FpsLimit::ALL.to_vec()),
            launch_at_startup in any::<bool>(),
            display_mode in prop::sample::select(TrayDisplayMode::ALL.to_vec()),
            auto_update_on_startup in any::<bool>(),
        ) {
            let record = SettingsRecord::new(
                generation,
                operation_id,
                AppSettings {
                    theme,
                    fps_limit,
                    launch_at_startup,
                    display_mode,
                    auto_update_on_startup,
                },
            );
            prop_assert_eq!(SettingsRecord::decode(&record.encode()), Some(record));
        }
    }
}
