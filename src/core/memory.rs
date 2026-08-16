/// Physical-memory snapshot used only for the tray tooltip.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryStatus {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub commit_limit_bytes: u64,
    pub commit_available_bytes: u64,
}

impl MemoryStatus {
    #[must_use]
    pub const fn new(total_bytes: u64, available_bytes: u64) -> Self {
        Self {
            total_bytes,
            available_bytes,
            commit_limit_bytes: 0,
            commit_available_bytes: 0,
        }
    }

    #[must_use]
    pub const fn with_commit(self, commit_limit_bytes: u64, commit_available_bytes: u64) -> Self {
        Self {
            commit_limit_bytes,
            commit_available_bytes,
            ..self
        }
    }

    #[must_use]
    pub const fn used_bytes(self) -> Option<u64> {
        if self.total_bytes == 0 || self.available_bytes > self.total_bytes {
            None
        } else {
            Some(self.total_bytes - self.available_bytes)
        }
    }

    /// Whole-machine physical memory in use, as a 0–100 percentage.
    ///
    /// A zero-sized machine or an impossible `available > total` reading is
    /// rejected rather than clamped into a fake utilisation.
    #[must_use]
    pub fn usage_percent(self) -> Option<f32> {
        percent_of(self.used_bytes()?, self.total_bytes)
    }

    #[must_use]
    pub fn commit_percent(self) -> Option<f32> {
        if self.commit_limit_bytes == 0 || self.commit_available_bytes > self.commit_limit_bytes {
            return None;
        }
        percent_of(
            self.commit_limit_bytes - self.commit_available_bytes,
            self.commit_limit_bytes,
        )
    }
}

#[must_use]
fn percent_of(part: u64, total: u64) -> Option<f32> {
    if total == 0 {
        return None;
    }
    let percent = (part as f64 * 100.0 / total as f64) as f32;
    percent.is_finite().then_some(percent.clamp(0.0, 100.0))
}

#[cfg(test)]
mod tests {
    use super::MemoryStatus;
    use proptest::prelude::*;

    #[test]
    fn c2_memory_usage_rejects_empty_and_impossible_snapshots() {
        assert_eq!(MemoryStatus::new(0, 0).usage_percent(), None);
        assert_eq!(MemoryStatus::new(0, 1).usage_percent(), None);
        assert_eq!(MemoryStatus::new(8, 9).usage_percent(), None);
        assert_eq!(MemoryStatus::new(8, 8).usage_percent(), Some(0.0));
        assert_eq!(MemoryStatus::new(8, 0).usage_percent(), Some(100.0));
        assert_eq!(MemoryStatus::new(16, 8).usage_percent(), Some(50.0));
        assert_eq!(MemoryStatus::new(16, 8).used_bytes(), Some(8));
    }

    #[test]
    fn c2_commit_percent_rejects_empty_and_impossible_snapshots() {
        assert_eq!(MemoryStatus::new(16, 8).commit_percent(), None);
        assert_eq!(
            MemoryStatus::new(16, 8).with_commit(0, 0).commit_percent(),
            None
        );
        assert_eq!(
            MemoryStatus::new(16, 8).with_commit(8, 9).commit_percent(),
            None
        );
        assert_eq!(
            MemoryStatus::new(16, 8).with_commit(10, 4).commit_percent(),
            Some(60.0)
        );
    }

    proptest! {
        #[test]
        fn pbt_valid_memory_snapshots_are_always_bounded(
            total in 1u64..1_000_000,
            available in 0u64..1_000_000,
        ) {
            let status = MemoryStatus::new(total, available);
            if available > total {
                prop_assert_eq!(status.usage_percent(), None);
            } else {
                let percent = status.usage_percent().expect("available <= total is valid");
                prop_assert!((0.0..=100.0).contains(&percent));
            }
        }
    }
}
