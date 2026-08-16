use windows_sys::Win32::UI::Shell::SHGetDiskFreeSpaceExW;

use crate::core::StorageStatus;

/// Production storage source. One `SHGetDiskFreeSpaceExW` call for the system
/// volume, with no handle or extra thread.
#[must_use]
pub fn read_storage_status() -> Option<StorageStatus> {
    let path: Vec<u16> = "C:\\".encode_utf16().chain(Some(0)).collect();
    let mut free_to_caller = 0_u64;
    let mut total = 0_u64;
    let mut free = 0_u64;
    let succeeded =
        unsafe { SHGetDiskFreeSpaceExW(path.as_ptr(), &mut free_to_caller, &mut total, &mut free) }
            != 0;
    succeeded.then_some(StorageStatus::new(total, free))
}
