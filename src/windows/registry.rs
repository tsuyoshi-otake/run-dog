use std::{mem::size_of, ptr, sync::Mutex};

use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::{
        LibraryLoader::GetModuleFileNameW,
        Registry::{
            RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
            RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_DWORD,
            REG_OPTION_NON_VOLATILE, REG_SZ,
        },
        SystemInformation::GetTickCount64,
    },
};

use crate::{
    application::{
        execute_commit, recover_pending, CommitGate, CommitOutcome, CommitRequest, DurableStore,
    },
    core::{AppSettings, PendingJournal, ResolvedTheme, SettingsRecord},
};

const DEFAULT_SETTINGS_KEY: &str = "Software\\SystemExe\\RunDog";
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const PERSONALIZE_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
const APP_VALUE: &str = "RunDog";
const SETTINGS_RECORD_VALUE: &str = "SettingsRecord";
const PENDING_JOURNAL_VALUE: &str = "PendingJournal";
const LIFECYCLE_VALUE: &str = "Lifecycle";
const LIFECYCLE_ACTIVE: &str = "active";
const LIFECYCLE_TOMBSTONED: &str = "tombstoned";
const LEGACY_THEME_VALUE: &str = "Theme";
const LEGACY_FPS_LIMIT_VALUE: &str = "FpsLimit";
const LEGACY_STARTUP_VALUE: &str = "LaunchAtStartup";
const SYSTEM_THEME_VALUE: &str = "SystemUsesLightTheme";
const MAX_REGISTRY_STRING_BYTES: u32 = 8_192;

static SETTINGS_KEY_OVERRIDE: Mutex<Option<String>> = Mutex::new(None);

/// Redirects settings persistence to a test-scoped HKCU path. Production code
/// never calls this; integration tests isolate durable state per run.
pub fn set_settings_key_override(path: Option<String>) {
    *SETTINGS_KEY_OVERRIDE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = path;
}

fn settings_key() -> String {
    SETTINGS_KEY_OVERRIDE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .unwrap_or_else(|| DEFAULT_SETTINGS_KEY.to_owned())
}

/// Windows-backed durable store used by the tray adapter.
pub struct RegistryStore {
    settings_key: String,
    gate: CommitGate,
    run_value_name: String,
    use_system_clock: bool,
}

impl RegistryStore {
    #[must_use]
    pub fn production() -> Self {
        Self {
            settings_key: DEFAULT_SETTINGS_KEY.to_owned(),
            gate: CommitGate::with_clock(unsafe { GetTickCount64() }),
            run_value_name: APP_VALUE.to_owned(),
            use_system_clock: true,
        }
    }

    #[must_use]
    pub fn for_test(settings_key: impl Into<String>, run_value_name: impl Into<String>) -> Self {
        Self {
            settings_key: settings_key.into(),
            gate: CommitGate::with_clock(0),
            run_value_name: run_value_name.into(),
            use_system_clock: false,
        }
    }

    fn key(&self) -> &str {
        &self.settings_key
    }

    pub fn gate_mut(&mut self) -> &mut CommitGate {
        &mut self.gate
    }

    pub fn cancel(&mut self, operation_id: u64) {
        self.gate.cancel(operation_id);
    }

    pub fn refresh_clock(&mut self) {
        if self.use_system_clock {
            self.gate.set_now(unsafe { GetTickCount64() });
        }
    }
}

impl DurableStore for RegistryStore {
    fn load_record(&mut self) -> SettingsRecord {
        load_settings_record_at(self.key())
    }

    fn write_record(&mut self, record: SettingsRecord, expected_generation: u64) -> bool {
        write_settings_record_at(self.key(), record, expected_generation)
    }

    fn load_pending(&mut self) -> Option<PendingJournal> {
        load_pending_journal_at(self.key())
    }

    fn write_pending(&mut self, journal: &PendingJournal) -> bool {
        write_pending_journal_at(self.key(), journal)
    }

    fn clear_pending(&mut self) -> bool {
        clear_pending_journal_at(self.key())
    }

    fn is_tombstoned(&mut self) -> bool {
        lifecycle_is_tombstoned_at(self.key())
    }

    fn set_startup(&mut self, enabled: bool) -> bool {
        set_launch_at_startup_named(enabled, &self.run_value_name)
    }

    fn now_millis(&self) -> u64 {
        if self.use_system_clock {
            unsafe { GetTickCount64() }
        } else {
            self.gate.now_millis()
        }
    }

    fn is_cancelled(&self, operation_id: u64) -> bool {
        self.gate.is_cancelled(operation_id)
    }

    fn mark_timed_out(&mut self, operation_id: u64) {
        self.gate.mark_timed_out(operation_id);
    }

    fn mark_cancelled(&mut self, operation_id: u64) {
        self.gate.cancel(operation_id);
    }

    fn is_timed_out(&self, operation_id: u64) -> bool {
        self.gate.is_timed_out(operation_id)
    }
}

impl RegistryStore {
    pub fn execute_commit(&mut self, request: CommitRequest) -> CommitOutcome {
        self.refresh_clock();
        execute_commit(self, request)
    }

    pub fn recover(&mut self) -> CommitOutcome {
        self.refresh_clock();
        recover_pending(self)
    }

    pub fn tombstone(&mut self) -> bool {
        tombstone_settings_key_at(self.key())
    }

    /// Clears an uninstall leftover so a running install can persist again.
    ///
    /// Tombstone is a protocol guard for isolated test hives. If that flag is
    /// left on the production key, every commit fails and Launch at startup
    /// can only roll back to Off.
    pub fn clear_tombstone(&mut self) -> bool {
        clear_tombstone_at(self.key())
    }
}

/// Reads one durable settings generation. Legacy layouts migrate to generation 0.
#[must_use]
pub fn load_settings_record() -> SettingsRecord {
    load_settings_record_at(&settings_key())
}

fn load_settings_record_at(settings_key: &str) -> SettingsRecord {
    if lifecycle_is_tombstoned_at(settings_key) {
        return SettingsRecord::new(0, 0, AppSettings::default());
    }
    let Some(key) = open_key(settings_key, KEY_READ) else {
        return SettingsRecord::new(0, 0, AppSettings::default());
    };

    let record = if let Some(payload) = read_string(key, SETTINGS_RECORD_VALUE) {
        SettingsRecord::decode(&payload)
            .unwrap_or_else(|| SettingsRecord::new(0, 0, AppSettings::default()))
    } else {
        let theme = read_string(key, LEGACY_THEME_VALUE);
        let fps_limit = read_string(key, LEGACY_FPS_LIMIT_VALUE);
        let launch_at_startup = read_dword(key, LEGACY_STARTUP_VALUE).map(|value| value != 0);
        SettingsRecord::new(
            0,
            0,
            AppSettings::from_persisted(theme.as_deref(), fps_limit.as_deref(), launch_at_startup),
        )
    };
    close_key(key);
    record
}

fn write_settings_record_at(
    settings_key: &str,
    record: SettingsRecord,
    expected_generation: u64,
) -> bool {
    if lifecycle_is_tombstoned_at(settings_key) {
        return false;
    }
    let current = load_settings_record_at(settings_key);
    if current.generation != expected_generation {
        return false;
    }
    let Some(key) = open_writable_settings_key_at(settings_key) else {
        return false;
    };
    let wrote = write_string(key, SETTINGS_RECORD_VALUE, &record.encode());
    if wrote {
        let _ = write_string(key, LIFECYCLE_VALUE, LIFECYCLE_ACTIVE);
        delete_value(key, LEGACY_THEME_VALUE);
        delete_value(key, LEGACY_FPS_LIMIT_VALUE);
        delete_value(key, LEGACY_STARTUP_VALUE);
    }
    close_key(key);
    wrote
}

/// Best-effort save used during process exit.
pub fn save_settings(settings: AppSettings) {
    let key = settings_key();
    let current = load_settings_record_at(&key);
    let next = SettingsRecord::new(
        current.generation.saturating_add(1),
        current.last_operation_id,
        settings,
    );
    let _ = write_settings_record_at(&key, next, current.generation);
}

fn load_pending_journal_at(settings_key: &str) -> Option<PendingJournal> {
    let key = open_key(settings_key, KEY_READ)?;
    let payload = read_string(key, PENDING_JOURNAL_VALUE);
    close_key(key);
    payload.as_deref().and_then(PendingJournal::decode)
}

fn write_pending_journal_at(settings_key: &str, journal: &PendingJournal) -> bool {
    if lifecycle_is_tombstoned_at(settings_key) {
        return false;
    }
    let Some(key) = open_writable_settings_key_at(settings_key) else {
        return false;
    };
    let wrote = write_string(key, PENDING_JOURNAL_VALUE, &journal.encode());
    close_key(key);
    wrote
}

fn clear_pending_journal_at(settings_key: &str) -> bool {
    let Some(key) = open_writable_settings_key_at(settings_key) else {
        return false;
    };
    let cleared = delete_value(key, PENDING_JOURNAL_VALUE);
    close_key(key);
    cleared
}

fn lifecycle_is_tombstoned_at(settings_key: &str) -> bool {
    let Some(key) = open_key(settings_key, KEY_READ) else {
        return false;
    };
    let value = read_string(key, LIFECYCLE_VALUE);
    close_key(key);
    value.as_deref() == Some(LIFECYCLE_TOMBSTONED)
}

fn is_production_settings_key(path: &str) -> bool {
    path.eq_ignore_ascii_case(DEFAULT_SETTINGS_KEY)
}

fn clear_tombstone_at(settings_key: &str) -> bool {
    let Some(key) = open_key(settings_key, KEY_READ | KEY_WRITE) else {
        return true;
    };
    let cleared = if read_string(key, LIFECYCLE_VALUE).as_deref() == Some(LIFECYCLE_TOMBSTONED) {
        delete_value(key, LIFECYCLE_VALUE)
    } else {
        true
    };
    close_key(key);
    cleared
}

/// Marks the settings key deleted for protocol purposes. Subsequent writes
/// refuse to recreate durable settings state.
pub fn tombstone_settings_key() -> bool {
    tombstone_settings_key_at(&settings_key())
}

fn tombstone_settings_key_at(settings_key: &str) -> bool {
    if is_production_settings_key(settings_key) {
        return false;
    }
    let Some(key) = open_writable_settings_key_at(settings_key) else {
        return false;
    };
    let wrote = write_string(key, LIFECYCLE_VALUE, LIFECYCLE_TOMBSTONED);
    delete_value(key, SETTINGS_RECORD_VALUE);
    delete_value(key, PENDING_JOURNAL_VALUE);
    close_key(key);
    wrote
}

/// Opens an existing settings key, or creates it only when no tombstone exists.
fn open_writable_settings_key_at(settings_key: &str) -> Option<HKEY> {
    if lifecycle_is_tombstoned_at(settings_key) {
        return None;
    }
    if let Some(key) = open_key(settings_key, KEY_READ | KEY_WRITE) {
        return Some(key);
    }
    create_key(settings_key)
}

/// Reads the current Windows system theme without installing a notification
/// object. `WM_SETTINGCHANGE` triggers this only when the OS reports a change.
#[must_use]
pub fn system_theme() -> ResolvedTheme {
    let Some(key) = open_key(PERSONALIZE_KEY, KEY_READ) else {
        return ResolvedTheme::Dark;
    };
    let value = read_dword(key, SYSTEM_THEME_VALUE);
    close_key(key);
    if value.unwrap_or(0) == 0 {
        ResolvedTheme::Dark
    } else {
        ResolvedTheme::Light
    }
}

pub fn set_launch_at_startup(enabled: bool) -> bool {
    set_launch_at_startup_named(enabled, APP_VALUE)
}

fn set_launch_at_startup_named(enabled: bool, value_name: &str) -> bool {
    let Some(key) = create_key(RUN_KEY) else {
        return false;
    };
    let succeeded = if enabled {
        current_executable_command().is_some_and(|command| write_string(key, value_name, &command))
    } else {
        delete_value(key, value_name)
    };
    close_key(key);
    succeeded
}

/// Makes the Run entry agree with the durable settings flag.
pub fn reconcile_launch_at_startup(settings: AppSettings) {
    let _ = set_launch_at_startup(settings.launch_at_startup);
}

fn open_key(path: &str, access: u32) -> Option<HKEY> {
    let path = wide(path);
    let mut key = ptr::null_mut();
    let result = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &mut key) };
    (result == ERROR_SUCCESS).then_some(key)
}

fn create_key(path: &str) -> Option<HKEY> {
    let path = wide(path);
    let mut key = ptr::null_mut();
    let mut disposition = 0;
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            path.as_ptr(),
            0,
            ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE | KEY_READ,
            ptr::null(),
            &mut key,
            &mut disposition,
        )
    };
    (result == ERROR_SUCCESS).then_some(key)
}

fn close_key(key: HKEY) {
    let _ = unsafe { RegCloseKey(key) };
}

fn delete_value(key: HKEY, name: &str) -> bool {
    let name = wide(name);
    let result = unsafe { RegDeleteValueW(key, name.as_ptr()) };
    result == ERROR_SUCCESS || result == ERROR_FILE_NOT_FOUND
}

fn read_string(key: HKEY, name: &str) -> Option<String> {
    let name = wide(name);
    let mut value_type = 0;
    let mut length = 0;
    let result = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            ptr::null(),
            &mut value_type,
            ptr::null_mut(),
            &mut length,
        )
    };
    if result != ERROR_SUCCESS
        || value_type != REG_SZ
        || length == 0
        || length > MAX_REGISTRY_STRING_BYTES
        || length % 2 != 0
    {
        return None;
    }

    let mut utf16 = vec![0_u16; (length / 2) as usize];
    let result = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            ptr::null(),
            &mut value_type,
            utf16.as_mut_ptr().cast::<u8>(),
            &mut length,
        )
    };
    if result != ERROR_SUCCESS || value_type != REG_SZ {
        return None;
    }
    let terminator = utf16
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(utf16.len());
    String::from_utf16(&utf16[..terminator]).ok()
}

fn read_dword(key: HKEY, name: &str) -> Option<u32> {
    let name = wide(name);
    let mut value_type = 0;
    let mut length = size_of::<u32>() as u32;
    let mut value = 0_u32;
    let result = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            ptr::null(),
            &mut value_type,
            (&mut value as *mut u32).cast::<u8>(),
            &mut length,
        )
    };
    (result == ERROR_SUCCESS && value_type == REG_DWORD && length == size_of::<u32>() as u32)
        .then_some(value)
}

fn write_string(key: HKEY, name: &str, value: &str) -> bool {
    let name = wide(name);
    let value = wide(value);
    let byte_length = (value.len() * size_of::<u16>()) as u32;
    (unsafe {
        RegSetValueExW(
            key,
            name.as_ptr(),
            0,
            REG_SZ,
            value.as_ptr().cast::<u8>(),
            byte_length,
        )
    }) == ERROR_SUCCESS
}

fn current_executable_command() -> Option<String> {
    const MAX_PATH_UNITS: usize = 32_768;
    let mut buffer = vec![0_u16; MAX_PATH_UNITS];
    let length =
        unsafe { GetModuleFileNameW(ptr::null_mut(), buffer.as_mut_ptr(), buffer.len() as u32) }
            as usize;
    if length == 0 || length >= buffer.len() - 1 {
        return None;
    }
    String::from_utf16(&buffer[..length])
        .ok()
        .map(|path| format!("\"{path}\""))
}

#[must_use]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

/// Test helper: unique HKCU path for live hive integration tests.
#[must_use]
pub fn test_hive_path(suffix: &str) -> String {
    format!("Software\\SystemExe\\RunDog\\.test\\{suffix}")
}

#[cfg(test)]
mod tests {
    use super::wide;

    #[test]
    fn component_utf16_encoder_is_nul_terminated_and_preserves_non_ascii() {
        let encoded = wide("RunDog 犬");
        assert_eq!(encoded.last(), Some(&0));
        assert_eq!(
            String::from_utf16(&encoded[..encoded.len() - 1]).ok(),
            Some("RunDog 犬".to_owned())
        );
    }
}
