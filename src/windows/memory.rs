use std::mem::size_of;

use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

use crate::core::MemoryStatus;

/// Production memory source. `GlobalMemoryStatusEx` is a single snapshot call
/// with no counter, query handle, or extra thread.
#[must_use]
pub fn read_memory_status() -> Option<MemoryStatus> {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..MEMORYSTATUSEX::default()
    };
    let succeeded = unsafe { GlobalMemoryStatusEx(&mut status) != 0 };
    succeeded.then_some(
        MemoryStatus::new(status.ullTotalPhys, status.ullAvailPhys)
            .with_commit(status.ullTotalPageFile, status.ullAvailPageFile),
    )
}
