/// GPU adapter snapshot used by the hover flyout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuStatus {
    pub dedicated_budget_bytes: u64,
    pub dedicated_usage_bytes: u64,
    pub shared_budget_bytes: u64,
    pub shared_usage_bytes: u64,
    utilization_percent: Option<f32>,
}

impl GpuStatus {
    #[must_use]
    pub const fn new(
        dedicated_budget_bytes: u64,
        dedicated_usage_bytes: u64,
        shared_budget_bytes: u64,
        shared_usage_bytes: u64,
    ) -> Self {
        Self {
            dedicated_budget_bytes,
            dedicated_usage_bytes,
            shared_budget_bytes,
            shared_usage_bytes,
            utilization_percent: None,
        }
    }

    #[must_use]
    pub const fn with_utilization(self, utilization_percent: Option<f32>) -> Self {
        Self {
            utilization_percent,
            ..self
        }
    }

    #[must_use]
    pub fn utilization_percent(self) -> Option<f32> {
        self.utilization_percent
            .filter(|percent| percent.is_finite())
            .map(|percent| percent.clamp(0.0, 100.0))
    }

    #[must_use]
    pub fn dedicated_percent(self) -> Option<f32> {
        percent_of(self.dedicated_usage_bytes, self.dedicated_budget_bytes)
    }

    #[must_use]
    pub fn shared_percent(self) -> Option<f32> {
        percent_of(self.shared_usage_bytes, self.shared_budget_bytes)
    }

    #[must_use]
    pub const fn has_dedicated(self) -> bool {
        self.dedicated_budget_bytes > 0
    }

    #[must_use]
    pub const fn has_shared(self) -> bool {
        self.shared_budget_bytes > 0
    }

    #[must_use]
    pub const fn in_use_bytes(self) -> u64 {
        self.dedicated_usage_bytes
            .saturating_add(self.shared_usage_bytes)
    }

    #[must_use]
    pub const fn available_bytes(self) -> Option<u64> {
        let dedicated = remaining(self.dedicated_usage_bytes, self.dedicated_budget_bytes);
        let shared = remaining(self.shared_usage_bytes, self.shared_budget_bytes);
        match (dedicated, shared) {
            (None, None) => None,
            (Some(dedicated), Some(shared)) => Some(dedicated.saturating_add(shared)),
            (Some(value), None) | (None, Some(value)) => Some(value),
        }
    }
}

#[must_use]
const fn remaining(used: u64, budget: u64) -> Option<u64> {
    if budget == 0 {
        None
    } else {
        Some(budget.saturating_sub(used))
    }
}

#[must_use]
fn percent_of(part: u64, total: u64) -> Option<f32> {
    if total == 0 {
        return None;
    }
    let percent = (part.min(total) as f64 * 100.0 / total as f64) as f32;
    percent.is_finite().then_some(percent.clamp(0.0, 100.0))
}

#[cfg(test)]
mod tests {
    use super::GpuStatus;
    use proptest::prelude::*;

    #[test]
    fn c2_gpu_percents_reject_empty_and_impossible_snapshots() {
        let empty = GpuStatus::new(0, 0, 0, 0);
        assert_eq!(empty.dedicated_percent(), None);
        assert_eq!(empty.shared_percent(), None);
        assert_eq!(empty.utilization_percent(), None);
        assert_eq!(empty.available_bytes(), None);
        assert_eq!(empty.in_use_bytes(), 0);
        assert!(!empty.has_dedicated());
        assert!(!empty.has_shared());

        let dedicated = GpuStatus::new(8, 2, 0, 0).with_utilization(Some(12.5));
        assert_eq!(dedicated.dedicated_percent(), Some(25.0));
        assert_eq!(dedicated.shared_percent(), None);
        assert_eq!(dedicated.utilization_percent(), Some(12.5));
        assert!(dedicated.has_dedicated());

        let shared = GpuStatus::new(0, 0, 16, 4);
        assert_eq!(shared.shared_percent(), Some(25.0));
        assert_eq!(shared.available_bytes(), Some(12));
        assert_eq!(shared.in_use_bytes(), 4);
        assert!(shared.has_shared());

        let both = GpuStatus::new(8, 2, 16, 4);
        assert_eq!(both.available_bytes(), Some(18));

        assert_eq!(GpuStatus::new(8, 9, 0, 0).dedicated_percent(), Some(100.0));
        assert_eq!(
            GpuStatus::new(8, 0, 0, 0)
                .with_utilization(Some(f32::NAN))
                .utilization_percent(),
            None
        );
        assert_eq!(
            GpuStatus::new(8, 0, 0, 0)
                .with_utilization(Some(140.0))
                .utilization_percent(),
            Some(100.0)
        );
    }

    proptest! {
        #[test]
        fn pbt_valid_gpu_memory_snapshots_are_always_bounded(
            budget in 1u64..1_000_000,
            usage in 0u64..1_000_000,
        ) {
            let status = GpuStatus::new(budget, usage, budget, usage);
            if usage > budget {
                prop_assert_eq!(status.dedicated_percent(), Some(100.0));
                prop_assert_eq!(status.shared_percent(), Some(100.0));
            } else {
                let dedicated = status.dedicated_percent().expect("usage <= budget is valid");
                let shared = status.shared_percent().expect("usage <= budget is valid");
                prop_assert!((0.0..=100.0).contains(&dedicated));
                prop_assert!((0.0..=100.0).contains(&shared));
            }
        }
    }
}
