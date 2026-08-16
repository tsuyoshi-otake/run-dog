/// System-volume snapshot used by the hover flyout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageStatus {
    pub total_bytes: u64,
    pub free_bytes: u64,
}

impl StorageStatus {
    #[must_use]
    pub const fn new(total_bytes: u64, free_bytes: u64) -> Self {
        Self {
            total_bytes,
            free_bytes,
        }
    }

    #[must_use]
    pub const fn used_bytes(self) -> Option<u64> {
        if self.total_bytes == 0 || self.free_bytes > self.total_bytes {
            None
        } else {
            Some(self.total_bytes - self.free_bytes)
        }
    }

    /// Used capacity as a 0–100 percentage.
    ///
    /// A zero-sized volume or an impossible `free > total` reading is rejected
    /// rather than clamped into a fake utilisation.
    #[must_use]
    pub fn used_percent(self) -> Option<f32> {
        let used = self.used_bytes()?;
        let percent = (used as f64 * 100.0 / self.total_bytes as f64) as f32;
        percent.is_finite().then_some(percent.clamp(0.0, 100.0))
    }
}

#[cfg(test)]
mod tests {
    use super::StorageStatus;
    use proptest::prelude::*;

    #[test]
    fn c2_storage_usage_rejects_empty_and_impossible_snapshots() {
        assert_eq!(StorageStatus::new(0, 0).used_percent(), None);
        assert_eq!(StorageStatus::new(0, 1).used_percent(), None);
        assert_eq!(StorageStatus::new(8, 9).used_percent(), None);
        assert_eq!(StorageStatus::new(8, 8).used_percent(), Some(0.0));
        assert_eq!(StorageStatus::new(8, 0).used_percent(), Some(100.0));
        assert_eq!(StorageStatus::new(16, 4).used_percent(), Some(75.0));
        assert_eq!(StorageStatus::new(16, 4).used_bytes(), Some(12));
    }

    proptest! {
        #[test]
        fn pbt_valid_storage_snapshots_are_always_bounded(
            total in 1u64..1_000_000,
            free in 0u64..1_000_000,
        ) {
            let status = StorageStatus::new(total, free);
            if free > total {
                prop_assert_eq!(status.used_percent(), None);
            } else {
                let percent = status.used_percent().expect("free <= total is valid");
                prop_assert!((0.0..=100.0).contains(&percent));
            }
        }
    }
}
