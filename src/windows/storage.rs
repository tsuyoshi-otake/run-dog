use std::{
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Component, Path, PathBuf},
};

use windows_sys::Win32::{
    System::SystemInformation::GetSystemDirectoryW, UI::Shell::SHGetDiskFreeSpaceExW,
};

use crate::core::StorageStatus;

/// Production storage source. One `SHGetDiskFreeSpaceExW` call for the system
/// volume, with no handle or extra thread.
#[must_use]
pub fn read_storage_status() -> Option<StorageStatus> {
    let system_dir = system_directory()?;
    let root = volume_root_from_windows_dir(&system_dir)?;
    disk_free_on_volume(&root)
}

fn system_directory() -> Option<PathBuf> {
    let mut buffer = vec![0_u16; 260];
    loop {
        let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 {
            return None;
        }
        if length as usize >= buffer.len() {
            buffer.resize(length as usize + 1, 0);
            continue;
        }
        return Some(PathBuf::from(OsString::from_wide(
            &buffer[..length as usize],
        )));
    }
}

/// Drive or UNC share that owns a Windows directory (`D:\Windows\System32` → `D:\`).
#[must_use]
pub(crate) fn volume_root_from_windows_dir(dir: &Path) -> Option<PathBuf> {
    let mut components = dir.components();
    let prefix = match components.next()? {
        Component::Prefix(prefix) => prefix,
        Component::RootDir => return Some(PathBuf::from(std::path::MAIN_SEPARATOR_STR)),
        _ => return None,
    };
    if !matches!(components.next(), Some(Component::RootDir)) {
        return None;
    }
    let mut root = PathBuf::from(prefix.as_os_str());
    root.push(std::path::MAIN_SEPARATOR.to_string());
    Some(root)
}

fn disk_free_on_volume(root: &Path) -> Option<StorageStatus> {
    let path: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free_to_caller = 0_u64;
    let mut total = 0_u64;
    let mut free = 0_u64;
    let succeeded =
        unsafe { SHGetDiskFreeSpaceExW(path.as_ptr(), &mut free_to_caller, &mut total, &mut free) }
            != 0;
    succeeded.then_some(StorageStatus::new(total, free))
}

#[cfg(test)]
mod tests {
    use super::volume_root_from_windows_dir;
    use std::path::{Path, PathBuf};

    #[test]
    fn component_volume_root_resolves_non_c_windows_directory() {
        assert_eq!(
            volume_root_from_windows_dir(Path::new(r"D:\Windows\System32")),
            Some(PathBuf::from(r"D:\"))
        );
        assert_eq!(
            volume_root_from_windows_dir(Path::new(r"E:\Windows")),
            Some(PathBuf::from(r"E:\"))
        );
        assert_eq!(
            volume_root_from_windows_dir(Path::new(r"C:\Windows\System32")),
            Some(PathBuf::from(r"C:\"))
        );
        assert_eq!(
            volume_root_from_windows_dir(Path::new(r"Windows\System32")),
            None
        );
        assert_eq!(volume_root_from_windows_dir(Path::new(r"D:Windows")), None);
    }
}
