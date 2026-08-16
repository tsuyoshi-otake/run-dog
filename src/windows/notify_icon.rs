//! First-run promotion of the shell notify icon out of the overflow flyout.
//!
//! Windows 11 stores per-icon visibility at
//! `HKCU\Control Panel\NotifyIconSettings\<id>\IsPromoted`. There is no public
//! API. Explorer creates the subkey only after `NIM_ADD`, and a missing
//! `IsPromoted` value means hidden. This adapter writes `1` only when that
//! value is absent, so a later user choice of `0` is left alone.
//!
//! The schema is undocumented. A missing root key, access failure, or unknown
//! value is a silent no-op.

use std::{mem::size_of, ptr};

use windows_sys::Win32::{
    Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
    System::{
        LibraryLoader::GetModuleFileNameW,
        Registry::{
            RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
            HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_DWORD, REG_SZ,
        },
    },
};

const SETTINGS_KEY: &str = "Control Panel\\NotifyIconSettings";
const EXECUTABLE_PATH_VALUE: &str = "ExecutablePath";
const IS_PROMOTED_VALUE: &str = "IsPromoted";
const MAX_REGISTRY_STRING_BYTES: u32 = 8_192;
const MAX_SUBKEY_UNITS: usize = 256;

/// Result of scanning Explorer's notify-icon settings for this executable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromoteScan {
    /// No subkey with this executable path exists yet.
    NotFound,
    /// The matching subkey is already configured (`0` or `1`).
    Identified,
    /// The matching subkey exists and `IsPromoted` is still absent.
    Promote,
}

/// One Explorer-owned notify-icon record, used by component tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NotifyIconRecord<'a> {
    pub executable_path: Option<&'a str>,
    pub is_promoted: Option<u32>,
}

/// Classifies Explorer records for `exe_path` without touching the hive.
#[must_use]
pub fn scan_records(exe_path: &str, records: &[NotifyIconRecord<'_>]) -> PromoteScan {
    let mut matched = false;
    let mut needs_promote = false;
    for record in records {
        if !path_matches(exe_path, record.executable_path) {
            continue;
        }
        matched = true;
        if record.is_promoted.is_none() {
            needs_promote = true;
        }
    }
    if !matched {
        PromoteScan::NotFound
    } else if needs_promote {
        PromoteScan::Promote
    } else {
        PromoteScan::Identified
    }
}

#[must_use]
pub fn path_matches(exe_path: &str, stored: Option<&str>) -> bool {
    let Some(stored) = stored else {
        return false;
    };
    normalize_path(exe_path).eq_ignore_ascii_case(&normalize_path(stored))
}

/// Tries to pin this process's notify icon. `Retry` means Explorer has not
/// populated the subkey yet; the caller may try again a bounded number of times.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromoteAttempt {
    Done,
    Retry,
}

#[must_use]
pub fn try_promote_current_executable() -> PromoteAttempt {
    let Some(exe_path) = current_executable_path() else {
        return PromoteAttempt::Done;
    };
    try_promote_executable(&exe_path)
}

fn try_promote_executable(exe_path: &str) -> PromoteAttempt {
    let Some(root) = open_key(SETTINGS_KEY, KEY_READ) else {
        return PromoteAttempt::Done;
    };

    let mut names = Vec::new();
    let mut stored_paths = Vec::new();
    let mut promoted_values = Vec::new();
    let mut index = 0;
    while let Some(name) = enum_subkey(root, index) {
        let path = format!("{SETTINGS_KEY}\\{name}");
        let (stored, promoted) = if let Some(sub) = open_key(&path, KEY_READ) {
            let stored = read_string(sub, EXECUTABLE_PATH_VALUE);
            let promoted = read_dword(sub, IS_PROMOTED_VALUE);
            close_key(sub);
            (stored, promoted)
        } else {
            (None, None)
        };
        names.push(name);
        stored_paths.push(stored);
        promoted_values.push(promoted);
        index += 1;
    }
    close_key(root);

    let records: Vec<NotifyIconRecord<'_>> = stored_paths
        .iter()
        .zip(promoted_values.iter())
        .map(|(path, promoted)| NotifyIconRecord {
            executable_path: path.as_deref(),
            is_promoted: *promoted,
        })
        .collect();

    match scan_records(exe_path, &records) {
        PromoteScan::NotFound => PromoteAttempt::Retry,
        PromoteScan::Identified => PromoteAttempt::Done,
        PromoteScan::Promote => {
            for (index, record) in records.iter().enumerate() {
                if record.is_promoted.is_some() || !path_matches(exe_path, record.executable_path) {
                    continue;
                }
                let path = format!("{SETTINGS_KEY}\\{}", names[index]);
                let Some(writable) = open_key(&path, KEY_READ | KEY_SET_VALUE) else {
                    continue;
                };
                let _ = write_dword(writable, IS_PROMOTED_VALUE, 1);
                close_key(writable);
            }
            PromoteAttempt::Done
        }
    }
}

fn normalize_path(value: &str) -> String {
    value.trim().trim_matches('"').replace('/', "\\")
}

fn current_executable_path() -> Option<String> {
    const MAX_PATH_UNITS: usize = 32_768;
    let mut buffer = vec![0_u16; MAX_PATH_UNITS];
    let length =
        unsafe { GetModuleFileNameW(ptr::null_mut(), buffer.as_mut_ptr(), buffer.len() as u32) }
            as usize;
    if length == 0 || length >= buffer.len() - 1 {
        return None;
    }
    String::from_utf16(&buffer[..length]).ok()
}

fn open_key(path: &str, access: u32) -> Option<HKEY> {
    let path = wide(path);
    let mut key = ptr::null_mut();
    let result = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &mut key) };
    (result == ERROR_SUCCESS).then_some(key)
}

fn close_key(key: HKEY) {
    let _ = unsafe { RegCloseKey(key) };
}

fn enum_subkey(key: HKEY, index: u32) -> Option<String> {
    let mut name = [0_u16; MAX_SUBKEY_UNITS];
    let mut name_len = name.len() as u32;
    let result = unsafe {
        RegEnumKeyExW(
            key,
            index,
            name.as_mut_ptr(),
            &mut name_len,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if result == ERROR_NO_MORE_ITEMS {
        return None;
    }
    if result != ERROR_SUCCESS || name_len == 0 {
        return None;
    }
    String::from_utf16(&name[..name_len as usize]).ok()
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

fn write_dword(key: HKEY, name: &str, value: u32) -> bool {
    let name = wide(name);
    let bytes = value.to_le_bytes();
    (unsafe {
        RegSetValueExW(
            key,
            name.as_ptr(),
            0,
            REG_DWORD,
            bytes.as_ptr(),
            size_of::<u32>() as u32,
        )
    }) == ERROR_SUCCESS
}

#[must_use]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{path_matches, scan_records, NotifyIconRecord, PromoteScan};

    const EXE: &str = r"C:\Users\me\AppData\Local\Programs\RunDog\RunDog.exe";

    #[test]
    fn component_promote_scan_covers_path_and_is_promoted_partitions() {
        assert_eq!(scan_records(EXE, &[]), PromoteScan::NotFound);
        assert_eq!(
            scan_records(
                EXE,
                &[NotifyIconRecord {
                    executable_path: None,
                    is_promoted: None,
                }]
            ),
            PromoteScan::NotFound
        );
        assert_eq!(
            scan_records(
                EXE,
                &[NotifyIconRecord {
                    executable_path: Some(r"C:\Windows\explorer.exe"),
                    is_promoted: None,
                }]
            ),
            PromoteScan::NotFound
        );
        assert_eq!(
            scan_records(
                EXE,
                &[NotifyIconRecord {
                    executable_path: Some(EXE),
                    is_promoted: None,
                }]
            ),
            PromoteScan::Promote
        );
        assert_eq!(
            scan_records(
                EXE,
                &[NotifyIconRecord {
                    executable_path: Some(EXE),
                    is_promoted: Some(0),
                }]
            ),
            PromoteScan::Identified
        );
        assert_eq!(
            scan_records(
                EXE,
                &[NotifyIconRecord {
                    executable_path: Some(EXE),
                    is_promoted: Some(1),
                }]
            ),
            PromoteScan::Identified
        );
        assert_eq!(
            scan_records(
                EXE,
                &[NotifyIconRecord {
                    executable_path: Some(EXE),
                    is_promoted: Some(2),
                }]
            ),
            PromoteScan::Identified
        );
        assert_eq!(
            scan_records(
                EXE,
                &[
                    NotifyIconRecord {
                        executable_path: Some(EXE),
                        is_promoted: Some(0),
                    },
                    NotifyIconRecord {
                        executable_path: Some(EXE),
                        is_promoted: None,
                    },
                ]
            ),
            PromoteScan::Promote
        );
    }

    #[test]
    fn component_promote_scan_promotes_only_unconfigured_matches() {
        let records = [
            NotifyIconRecord {
                executable_path: Some(r"C:\Windows\System32\OneDrive.exe"),
                is_promoted: None,
            },
            NotifyIconRecord {
                executable_path: Some(EXE),
                is_promoted: None,
            },
        ];
        assert_eq!(scan_records(EXE, &records), PromoteScan::Promote);
    }

    #[test]
    fn component_path_match_ignores_quotes_slash_and_case() {
        assert!(path_matches(
            EXE,
            Some(r#"C:\Users\me\AppData\Local\Programs\RunDog\RunDog.exe"#)
        ));
        assert!(path_matches(
            EXE,
            Some(r#""C:\Users\me\AppData\Local\Programs\RunDog\RunDog.exe""#)
        ));
        assert!(path_matches(
            EXE,
            Some(r"c:/users/me/appdata/local/programs/rundog/rundog.exe")
        ));
        assert!(!path_matches(EXE, None));
        assert!(!path_matches(EXE, Some("")));
    }
}
