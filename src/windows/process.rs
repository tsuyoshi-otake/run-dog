use std::mem::size_of;

use windows_sys::Win32::{
    Foundation::FILETIME,
    System::{
        ProcessStatus::{EmptyWorkingSet, GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::{GetCurrentProcess, GetProcessTimes},
    },
};

use crate::core::{process_share, ProcessStatus, ProcessTimes, SystemTimes};

/// Tracks this process against the same `GetSystemTimes` interval the tray uses.
#[derive(Default)]
pub struct WindowsProcessSource {
    previous_system: Option<SystemTimes>,
    previous_process: Option<ProcessTimes>,
}

impl WindowsProcessSource {
    #[must_use]
    pub fn sample(&mut self, system: SystemTimes) -> Option<ProcessStatus> {
        let process = read_process_times()?;
        let private_bytes = read_private_bytes()?;
        let cpu = match (self.previous_system, self.previous_process) {
            (Some(previous_system), Some(previous_process)) => {
                process_share(previous_system, system, previous_process, process)
            }
            _ => None,
        };
        self.previous_system = Some(system);
        self.previous_process = Some(process);
        Some(ProcessStatus::new(private_bytes, cpu))
    }
}

/// Returns unused pages to the OS after bursty work such as jsonl catch-up.
pub fn trim_working_set() {
    let _ = unsafe { EmptyWorkingSet(GetCurrentProcess()) };
}

fn read_process_times() -> Option<ProcessTimes> {
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let succeeded = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    } != 0;
    succeeded.then_some(ProcessTimes::new(
        filetime_to_u64(kernel),
        filetime_to_u64(user),
    ))
}

fn read_private_bytes() -> Option<u64> {
    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..PROCESS_MEMORY_COUNTERS::default()
    };
    let succeeded = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    } != 0;
    succeeded.then_some(counters.PagefileUsage as u64)
}

#[must_use]
const fn filetime_to_u64(value: FILETIME) -> u64 {
    value.dwLowDateTime as u64 | ((value.dwHighDateTime as u64) << 32)
}
