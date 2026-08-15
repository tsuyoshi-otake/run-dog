use std::{mem::size_of, ptr};

use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::{
        LibraryLoader::GetModuleFileNameW,
        Registry::{
            RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
            RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_DWORD,
            REG_OPTION_NON_VOLATILE, REG_SZ,
        },
    },
};

use crate::core::{AppSettings, ResolvedTheme};

const SETTINGS_KEY: &str = "Software\\SystemExe\\RunDog";
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const PERSONALIZE_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
const APP_VALUE: &str = "RunDog";
const THEME_VALUE: &str = "Theme";
const FPS_LIMIT_VALUE: &str = "FpsLimit";
const STARTUP_VALUE: &str = "LaunchAtStartup";
const SYSTEM_THEME_VALUE: &str = "SystemUsesLightTheme";
const MAX_REGISTRY_STRING_BYTES: u32 = 8_192;

/// Reads settings once at startup. Malformed or unavailable values are mapped
/// to the defaults in `AppSettings`.
#[must_use]
pub fn load_settings() -> AppSettings {
    let Some(key) = open_key(SETTINGS_KEY, KEY_READ) else {
        return AppSettings::default();
    };
    let theme = read_string(key, THEME_VALUE);
    let fps_limit = read_string(key, FPS_LIMIT_VALUE);
    let launch_at_startup = read_dword(key, STARTUP_VALUE).map(|value| value != 0);
    close_key(key);
    AppSettings::from_persisted(theme.as_deref(), fps_limit.as_deref(), launch_at_startup)
}

/// Writes the small settings payload only after an explicit setting change or
/// clean shutdown; the animation loop never performs registry I/O.
pub fn save_settings(settings: AppSettings) {
    let Some(key) = create_key(SETTINGS_KEY) else {
        return;
    };
    let _ = write_string(key, THEME_VALUE, settings.theme.persisted_name());
    let _ = write_string(key, FPS_LIMIT_VALUE, settings.fps_limit.persisted_name());
    let _ = write_dword(key, STARTUP_VALUE, u32::from(settings.launch_at_startup));
    close_key(key);
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

/// Adds or removes the app's own HKCU Run value. The boolean is returned to
/// the application so the checkbox can commit or roll back atomically.
pub fn set_launch_at_startup(enabled: bool) -> bool {
    let Some(key) = create_key(RUN_KEY) else {
        return false;
    };
    let succeeded = if enabled {
        current_executable_command().is_some_and(|command| write_string(key, APP_VALUE, &command))
    } else {
        let name = wide(APP_VALUE);
        let result = unsafe { RegDeleteValueW(key, name.as_ptr()) };
        result == ERROR_SUCCESS || result == ERROR_FILE_NOT_FOUND
    };
    close_key(key);
    succeeded
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
            KEY_WRITE,
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

fn write_dword(key: HKEY, name: &str, value: u32) -> bool {
    let name = wide(name);
    (unsafe {
        RegSetValueExW(
            key,
            name.as_ptr(),
            0,
            REG_DWORD,
            (&value as *const u32).cast::<u8>(),
            size_of::<u32>() as u32,
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
