//! Live HKCU hive integration tests for the durable settings protocol.
//!
//! Each test uses an isolated key under `Software\SystemExe\RunDog\.test\...`
//! and cleans up afterwards. These tests exercise real Registry APIs but never
//! touch the production settings key or the user's RunDog Run value.

#![cfg(windows)]

use std::time::{SystemTime, UNIX_EPOCH};

use run_dog::{
    application::{CommitRequest, CommitStatus, DurableStore},
    core::{AppSettings, FpsLimit, PendingJournal, SettingsRecord, ThemePreference},
    windows::registry::RegistryStore,
};

fn unique_suffix(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("{label}-{nanos}-{}", std::process::id())
}

struct HiveGuard {
    store: RegistryStore,
}

impl HiveGuard {
    fn new(label: &str) -> Self {
        let suffix = unique_suffix(label);
        let path = format!("Software\\SystemExe\\RunDog\\.test\\{suffix}");
        let run_value = format!("RunDogTest-{suffix}");
        let mut store = RegistryStore::for_test(path, run_value);
        store.gate_mut().set_now(0);
        Self { store }
    }
}

impl Drop for HiveGuard {
    fn drop(&mut self) {
        let _ = self.store.clear_pending();
        let _ = self.store.set_startup(false);
        let _ = self.store.tombstone();
    }
}

#[test]
fn live_hive_commit_is_atomic_and_survives_reload() {
    let mut guard = HiveGuard::new("atomic");
    let outcome = guard.store.execute_commit(CommitRequest {
        operation_id: 1,
        expected_generation: 0,
        settings: AppSettings {
            theme: ThemePreference::Dark,
            fps_limit: FpsLimit::Fps30,
            launch_at_startup: false,
        },
        previous: AppSettings::default(),
        sync_run_entry: false,
        deadline_millis: 5_000,
    });
    assert_eq!(outcome.status, CommitStatus::Applied);
    assert_eq!(outcome.generation, 1);

    let reloaded = guard.store.load_record();
    assert_eq!(reloaded.generation, 1);
    assert_eq!(reloaded.last_operation_id, 1);
    assert_eq!(reloaded.settings.theme, ThemePreference::Dark);
    assert_eq!(reloaded.settings.fps_limit, FpsLimit::Fps30);
}

#[test]
fn live_hive_duplicate_operation_id_is_rejected_as_duplicate() {
    let mut guard = HiveGuard::new("dup");
    let settings = AppSettings {
        theme: ThemePreference::Light,
        fps_limit: FpsLimit::Fps10,
        launch_at_startup: false,
    };
    assert_eq!(
        guard
            .store
            .execute_commit(CommitRequest {
                operation_id: 3,
                expected_generation: 0,
                settings,
                previous: AppSettings::default(),
                sync_run_entry: false,
                deadline_millis: 5_000,
            })
            .status,
        CommitStatus::Applied
    );
    let again = guard.store.execute_commit(CommitRequest {
        operation_id: 3,
        expected_generation: 1,
        settings: AppSettings {
            theme: ThemePreference::Dark,
            ..settings
        },
        previous: settings,
        sync_run_entry: false,
        deadline_millis: 5_000,
    });
    assert_eq!(again.status, CommitStatus::Duplicate);
    assert_eq!(
        guard.store.load_record().settings.theme,
        ThemePreference::Light
    );
}

#[test]
fn live_hive_tombstone_blocks_recreate_after_delete() {
    let mut guard = HiveGuard::new("tombstone");
    assert_eq!(
        guard
            .store
            .execute_commit(CommitRequest {
                operation_id: 1,
                expected_generation: 0,
                settings: AppSettings::default(),
                previous: AppSettings::default(),
                sync_run_entry: false,
                deadline_millis: 5_000,
            })
            .status,
        CommitStatus::Applied
    );
    assert!(guard.store.tombstone());
    let blocked = guard.store.execute_commit(CommitRequest {
        operation_id: 2,
        expected_generation: 1,
        settings: AppSettings {
            theme: ThemePreference::Dark,
            ..AppSettings::default()
        },
        previous: AppSettings::default(),
        sync_run_entry: false,
        deadline_millis: 5_000,
    });
    assert_eq!(blocked.status, CommitStatus::Tombstoned);
}

#[test]
fn live_hive_crash_recovery_finishes_pending_run_sync() {
    let mut guard = HiveGuard::new("recover");
    let desired = AppSettings {
        theme: ThemePreference::System,
        fps_limit: FpsLimit::Fps20,
        launch_at_startup: true,
    };
    let written = SettingsRecord::new(1, 8, desired);
    assert!(guard.store.write_record(written, 0));
    assert!(guard.store.write_pending(&PendingJournal {
        operation_id: 8,
        base_generation: 0,
        desired,
        previous: AppSettings::default(),
        sync_run_entry: true,
        deadline_millis: 60_000,
    }));

    let recovered = guard.store.recover();
    assert_eq!(recovered.status, CommitStatus::Applied);
    assert!(guard.store.load_pending().is_none());
}

#[test]
fn live_hive_startup_run_entry_round_trips_on_and_off() {
    let mut guard = HiveGuard::new("startup-roundtrip");
    let off = AppSettings::default();
    let on = AppSettings {
        launch_at_startup: true,
        ..off
    };

    let enabled = guard.store.execute_commit(CommitRequest {
        operation_id: 1,
        expected_generation: 0,
        settings: on,
        previous: off,
        sync_run_entry: true,
        deadline_millis: 5_000,
    });
    assert_eq!(enabled.status, CommitStatus::Applied);
    assert!(guard.store.load_record().settings.launch_at_startup);

    let disabled = guard.store.execute_commit(CommitRequest {
        operation_id: 2,
        expected_generation: 1,
        settings: off,
        previous: on,
        sync_run_entry: true,
        deadline_millis: 5_000,
    });
    assert_eq!(disabled.status, CommitStatus::Applied);
    assert!(!guard.store.load_record().settings.launch_at_startup);

    let enabled_again = guard.store.execute_commit(CommitRequest {
        operation_id: 3,
        expected_generation: 2,
        settings: on,
        previous: off,
        sync_run_entry: true,
        deadline_millis: 5_000,
    });
    assert_eq!(enabled_again.status, CommitStatus::Applied);
    assert!(guard.store.load_record().settings.launch_at_startup);
}

#[test]
fn live_hive_clear_tombstone_allows_startup_commit_again() {
    let mut guard = HiveGuard::new("startup-tombstone");
    let on = AppSettings {
        launch_at_startup: true,
        ..AppSettings::default()
    };
    assert_eq!(
        guard
            .store
            .execute_commit(CommitRequest {
                operation_id: 1,
                expected_generation: 0,
                settings: on,
                previous: AppSettings::default(),
                sync_run_entry: true,
                deadline_millis: 5_000,
            })
            .status,
        CommitStatus::Applied
    );
    assert!(guard.store.tombstone());
    assert_eq!(
        guard
            .store
            .execute_commit(CommitRequest {
                operation_id: 2,
                expected_generation: 1,
                settings: AppSettings::default(),
                previous: on,
                sync_run_entry: true,
                deadline_millis: 5_000,
            })
            .status,
        CommitStatus::Tombstoned
    );

    assert!(guard.store.clear_tombstone());
    let restored = guard.store.execute_commit(CommitRequest {
        operation_id: 1,
        expected_generation: 0,
        settings: on,
        previous: AppSettings::default(),
        sync_run_entry: true,
        deadline_millis: 5_000,
    });
    assert_eq!(restored.status, CommitStatus::Applied);
    assert!(guard.store.load_record().settings.launch_at_startup);
}

#[test]
fn production_settings_key_cannot_be_tombstoned() {
    let mut store = RegistryStore::production();
    assert!(!store.tombstone());
}
