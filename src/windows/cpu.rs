use windows_sys::Win32::{Foundation::FILETIME, System::Threading::GetSystemTimes};

use crate::{application::CpuSource, core::SystemTimes};

/// Production CPU source. `GetSystemTimes` exposes monotonically cumulative
/// values and has lower resident overhead than a PDH/PerformanceCounter setup.
#[derive(Default)]
pub struct WindowsCpuSource;

impl CpuSource for WindowsCpuSource {
    fn read_system_times(&mut self) -> Option<SystemTimes> {
        let mut idle = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let succeeded = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) != 0 };
        succeeded.then_some(SystemTimes::new(
            filetime_to_u64(idle),
            filetime_to_u64(kernel),
            filetime_to_u64(user),
        ))
    }
}

#[must_use]
const fn filetime_to_u64(value: FILETIME) -> u64 {
    value.dwLowDateTime as u64 | ((value.dwHighDateTime as u64) << 32)
}

#[cfg(test)]
mod tests {
    use super::filetime_to_u64;
    use windows_sys::Win32::Foundation::FILETIME;

    #[test]
    fn component_filetime_conversion_preserves_low_and_high_words() {
        assert_eq!(
            filetime_to_u64(FILETIME {
                dwLowDateTime: 0x89AB_CDEF,
                dwHighDateTime: 0x0123_4567,
            }),
            0x0123_4567_89AB_CDEF
        );
    }
}
