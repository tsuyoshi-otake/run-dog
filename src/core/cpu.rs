/// Cumulative CPU accounting values expressed in Windows FILETIME ticks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SystemTimes {
    pub idle: u64,
    pub kernel: u64,
    pub user: u64,
}

impl SystemTimes {
    #[must_use]
    pub const fn new(idle: u64, kernel: u64, user: u64) -> Self {
        Self { idle, kernel, user }
    }
}

/// A bounded whole-machine CPU utilisation percentage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CpuLoad(f32);

impl CpuLoad {
    #[must_use]
    pub fn percent(value: f32) -> Self {
        Self(if value.is_finite() {
            value.clamp(0.0, 100.0)
        } else {
            0.0
        })
    }

    #[must_use]
    pub const fn value(self) -> f32 {
        self.0
    }
}

/// Computes whole-machine usage from two cumulative snapshots.
///
/// `kernel` already includes `idle` on Windows. A counter reset, an invalid
/// idle delta, or a zero-duration interval deliberately yields `None` rather
/// than manufacturing a sample.
#[must_use]
pub fn usage_between(previous: SystemTimes, current: SystemTimes) -> Option<CpuLoad> {
    let idle = current.idle.checked_sub(previous.idle)?;
    let kernel = current.kernel.checked_sub(previous.kernel)?;
    let user = current.user.checked_sub(previous.user)?;
    let total = kernel.checked_add(user)?;

    if total == 0 || idle > kernel {
        return None;
    }

    let busy = total - idle;
    Some(CpuLoad::percent(
        (busy as f64 * 100.0 / total as f64) as f32,
    ))
}

/// Stateful sampler that retains only the preceding snapshot and performs a
/// light exponential moving average. No allocation occurs while sampling.
#[derive(Clone, Debug)]
pub struct CpuSampler {
    previous: Option<SystemTimes>,
    smoothed: Option<CpuLoad>,
    smoothing_factor: f32,
}

impl Default for CpuSampler {
    fn default() -> Self {
        Self::new(0.35)
    }
}

impl CpuSampler {
    #[must_use]
    pub fn new(smoothing_factor: f32) -> Self {
        Self {
            previous: None,
            smoothed: None,
            smoothing_factor: smoothing_factor.clamp(0.0, 1.0),
        }
    }

    /// Adds a cumulative sample. The first sample only establishes a base.
    #[must_use]
    pub fn push(&mut self, current: SystemTimes) -> Option<CpuLoad> {
        let previous = self.previous.replace(current)?;
        let raw = usage_between(previous, current)?;
        let next = match self.smoothed {
            Some(smoothed) => CpuLoad::percent(
                smoothed.value() + self.smoothing_factor * (raw.value() - smoothed.value()),
            ),
            None => raw,
        };
        self.smoothed = Some(next);
        Some(next)
    }

    #[must_use]
    pub const fn latest(&self) -> Option<CpuLoad> {
        self.smoothed
    }
}

#[cfg(test)]
mod tests {
    use super::{usage_between, CpuSampler, SystemTimes};
    use proptest::prelude::*;

    #[test]
    fn c2_cpu_delta_validation_exercises_every_boolean_condition() {
        let baseline = SystemTimes::new(10, 20, 30);

        assert_eq!(usage_between(baseline, SystemTimes::new(10, 20, 30)), None);
        assert_eq!(usage_between(baseline, SystemTimes::new(9, 21, 31)), None);
        assert_eq!(usage_between(baseline, SystemTimes::new(11, 19, 31)), None);
        assert_eq!(usage_between(baseline, SystemTimes::new(11, 21, 29)), None);
        assert_eq!(usage_between(baseline, SystemTimes::new(15, 22, 31)), None);
        assert_eq!(
            usage_between(SystemTimes::new(0, 0, 0), SystemTimes::new(0, u64::MAX, 1)),
            None
        );
        assert_eq!(
            usage_between(baseline, SystemTimes::new(11, 22, 32)).map(|value| value.value()),
            Some(75.0)
        );
        // `idle == kernel` is a valid, exactly 0% busy interval.  This is
        // distinct from the invalid `idle > kernel` condition.
        assert_eq!(
            usage_between(SystemTimes::new(0, 0, 0), SystemTimes::new(100, 100, 0))
                .map(|value| value.value()),
            Some(0.0)
        );
    }

    #[test]
    fn c2_sampler_handles_first_sample_invalid_delta_and_smoothed_sample() {
        let mut sampler = CpuSampler::new(0.5);
        assert_eq!(sampler.push(SystemTimes::new(0, 0, 0)), None);
        assert_eq!(
            sampler
                .push(SystemTimes::new(0, 100, 0))
                .map(|value| value.value()),
            Some(100.0)
        );
        assert_eq!(sampler.push(SystemTimes::new(100, 1, 1)), None);
        assert_eq!(
            sampler
                .push(SystemTimes::new(100, 101, 1))
                .map(|value| value.value()),
            Some(100.0)
        );
    }

    #[test]
    fn component_sampler_matches_the_requirement_ema_and_retains_latest_value() {
        // The expected value is the domain equation, calculated independently
        // of `CpuSampler::push`: 20 + 0.25 * (80 - 20) = 35.
        let mut sampler = CpuSampler::new(0.25);
        assert_eq!(sampler.push(SystemTimes::new(0, 0, 0)), None);
        assert_eq!(
            sampler
                .push(SystemTimes::new(80, 80, 20))
                .map(|value| value.value()),
            Some(20.0)
        );
        assert_eq!(
            sampler
                .push(SystemTimes::new(100, 100, 100))
                .map(|value| value.value()),
            Some(35.0)
        );
        assert_eq!(sampler.latest().map(|value| value.value()), Some(35.0));
    }

    proptest! {
        #[test]
        fn pbt_valid_monotonic_snapshots_are_always_bounded(
            idle_delta in 0u64..1_000_000,
            busy_kernel_delta in 0u64..1_000_000,
            user_delta in 0u64..1_000_000,
            base_idle in 0u64..1_000_000,
            base_kernel in 0u64..1_000_000,
            base_user in 0u64..1_000_000,
        ) {
            let previous = SystemTimes::new(base_idle, base_kernel, base_user);
            let current = SystemTimes::new(
                base_idle + idle_delta,
                base_kernel + idle_delta + busy_kernel_delta,
                base_user + user_delta,
            );
            let usage = usage_between(previous, current);
            if idle_delta + busy_kernel_delta + user_delta == 0 {
                prop_assert_eq!(usage, None);
            } else {
                let usage = usage.expect("positive total delta is valid");
                prop_assert!((0.0..=100.0).contains(&usage.value()));
            }
        }

        #[test]
        fn pbt_counter_regressions_never_panic_or_produce_a_sample(
            previous_idle in 1u64..1_000_000,
            previous_kernel in 1u64..1_000_000,
            previous_user in 1u64..1_000_000,
        ) {
            let previous = SystemTimes::new(previous_idle, previous_kernel, previous_user);
            let current = SystemTimes::new(previous_idle - 1, previous_kernel, previous_user);
            prop_assert_eq!(usage_between(previous, current), None);
        }

        /// Requirement-level EMA oracle.  Each cumulative pair represents a
        /// 100-tick interval, so the independently selected busy ticks are the
        /// raw CPU percentages without consulting implementation branches.
        #[test]
        fn pbt_ema_matches_independent_closed_form_oracle(
            first_busy in 1u8..100,
            second_busy in 0u8..101,
            alpha_hundredths in 1u8..100,
        ) {
            let first_busy = u64::from(first_busy);
            let second_busy = u64::from(second_busy);
            let alpha = f32::from(alpha_hundredths) / 100.0;
            let first = SystemTimes::new(100 - first_busy, 100 - first_busy, first_busy);
            let second = SystemTimes::new(
                200 - first_busy - second_busy,
                200 - first_busy - second_busy,
                first_busy + second_busy,
            );
            let expected = first_busy as f32 + alpha * (second_busy as f32 - first_busy as f32);
            let mut sampler = CpuSampler::new(alpha);

            prop_assert_eq!(sampler.push(SystemTimes::new(0, 0, 0)), None);
            prop_assert_eq!(
                sampler.push(first).map(|value| value.value()),
                Some(first_busy as f32)
            );
            let observed = sampler
                .push(second)
                .expect("the generated monotonic interval is valid")
                .value();
            prop_assert!((observed - expected).abs() < 0.0001);
            prop_assert_eq!(sampler.latest().map(|value| value.value()), Some(observed));
        }
    }
}
