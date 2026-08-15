use core::fmt;

use super::CpuLoad;

const MINIMUM_FPS: u16 = 5;
const SPEED_BANDS: [(u16, f32); 6] = [
    (5, 0.0),
    (10, 10.0),
    (15, 30.0),
    (20, 55.0),
    (30, 75.0),
    (40, 90.0),
];
const HYSTERESIS_PERCENT: f32 = 2.0;

/// Maximum animation rate exposed by the RunDog context menu.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum FpsLimit {
    Fps10,
    #[default]
    Fps20,
    Fps30,
    Fps40,
}

impl FpsLimit {
    pub const ALL: [Self; 4] = [Self::Fps10, Self::Fps20, Self::Fps30, Self::Fps40];

    #[must_use]
    pub const fn fps(self) -> u16 {
        match self {
            Self::Fps10 => 10,
            Self::Fps20 => 20,
            Self::Fps30 => 30,
            Self::Fps40 => 40,
        }
    }

    #[must_use]
    pub const fn persisted_name(self) -> &'static str {
        match self {
            Self::Fps10 => "10",
            Self::Fps20 => "20",
            Self::Fps30 => "30",
            Self::Fps40 => "40",
        }
    }

    #[must_use]
    pub fn parse_persisted(value: &str) -> Option<Self> {
        match value.trim() {
            "10" => Some(Self::Fps10),
            "20" => Some(Self::Fps20),
            "30" => Some(Self::Fps30),
            "40" => Some(Self::Fps40),
            _ => None,
        }
    }
}

impl fmt::Display for FpsLimit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.persisted_name())
    }
}

/// A frame-rate change requested by the pure state machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnimationRateChange {
    pub fps: u16,
    pub interval_ms: u32,
}

impl AnimationRateChange {
    #[must_use]
    pub const fn from_fps(fps: u16) -> Self {
        Self {
            fps,
            interval_ms: 1_000 / fps as u32,
        }
    }
}

/// Converts CPU load to a rate-limited animation speed without reconfiguring a
/// timer unless the selected speed actually changes.
#[derive(Clone, Debug)]
pub struct AnimationController {
    limit: FpsLimit,
    current_fps: u16,
    latest_load: CpuLoad,
}

impl AnimationController {
    #[must_use]
    pub fn new(limit: FpsLimit) -> Self {
        Self {
            limit,
            current_fps: MINIMUM_FPS,
            latest_load: CpuLoad::percent(0.0),
        }
    }

    #[must_use]
    pub const fn limit(&self) -> FpsLimit {
        self.limit
    }

    #[must_use]
    pub const fn current_fps(&self) -> u16 {
        self.current_fps
    }

    #[must_use]
    pub const fn current_interval_ms(&self) -> u32 {
        1_000 / self.current_fps as u32
    }

    #[must_use]
    pub fn update(&mut self, load: CpuLoad) -> Option<AnimationRateChange> {
        self.latest_load = load;
        let desired = desired_fps(load, self.limit);
        if desired == self.current_fps {
            return None;
        }

        let threshold = threshold_for_transition(self.current_fps, desired);
        let should_change = if desired > self.current_fps {
            load.value() >= threshold
        } else {
            load.value() < threshold
        };
        if !should_change {
            return None;
        }

        self.current_fps = desired;
        Some(AnimationRateChange::from_fps(desired))
    }

    /// Changes the maximum user-selected speed. Lowering a cap takes effect
    /// immediately; raising it still respects the regular CPU hysteresis.
    #[must_use]
    pub fn set_limit(&mut self, limit: FpsLimit) -> Option<AnimationRateChange> {
        if self.limit == limit {
            return None;
        }
        self.limit = limit;

        let desired = desired_fps(self.latest_load, limit);
        if desired >= self.current_fps {
            return self.update(self.latest_load);
        }

        self.current_fps = desired;
        Some(AnimationRateChange::from_fps(desired))
    }
}

#[must_use]
fn desired_fps(load: CpuLoad, limit: FpsLimit) -> u16 {
    SPEED_BANDS
        .iter()
        .copied()
        .take_while(|(fps, _)| *fps <= limit.fps())
        .filter(|(_, minimum_load)| load.value() >= *minimum_load)
        .map(|(fps, _)| fps)
        .last()
        .unwrap_or(MINIMUM_FPS)
}

#[must_use]
fn threshold_for_transition(current_fps: u16, desired_fps: u16) -> f32 {
    let band_threshold = |fps| {
        SPEED_BANDS
            .iter()
            .find_map(|(band_fps, threshold)| (*band_fps == fps).then_some(*threshold))
            .unwrap_or(0.0)
    };

    if desired_fps > current_fps {
        band_threshold(desired_fps) + HYSTERESIS_PERCENT
    } else {
        (band_threshold(current_fps) - HYSTERESIS_PERCENT).max(0.0)
    }
}

/// Cycles through a non-empty icon frame collection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameCursor {
    frame_count: usize,
    current: usize,
}

impl FrameCursor {
    #[must_use]
    pub fn new(frame_count: usize) -> Option<Self> {
        (frame_count > 0).then_some(Self {
            frame_count,
            current: 0,
        })
    }

    #[must_use]
    pub const fn current(&self) -> usize {
        self.current
    }

    #[must_use]
    pub const fn frame_count(&self) -> usize {
        self.frame_count
    }

    pub fn advance(&mut self) -> usize {
        self.current = (self.current + 1) % self.frame_count;
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::{AnimationController, FpsLimit, FrameCursor};
    use crate::core::CpuLoad;
    use proptest::prelude::*;

    #[test]
    fn c2_fps_persisted_parsing_covers_each_arm() {
        assert_eq!(FpsLimit::parse_persisted("10"), Some(FpsLimit::Fps10));
        assert_eq!(FpsLimit::parse_persisted("20"), Some(FpsLimit::Fps20));
        assert_eq!(FpsLimit::parse_persisted("30"), Some(FpsLimit::Fps30));
        assert_eq!(FpsLimit::parse_persisted("40"), Some(FpsLimit::Fps40));
        assert_eq!(FpsLimit::parse_persisted("5"), None);
    }

    #[test]
    fn c2_animation_hysteresis_covers_hold_raise_lower_and_cap_paths() {
        let mut animation = AnimationController::new(FpsLimit::Fps20);
        assert_eq!(animation.update(CpuLoad::percent(10.0)), None);
        assert_eq!(
            animation
                .update(CpuLoad::percent(12.0))
                .map(|change| change.fps),
            Some(10)
        );
        assert_eq!(animation.update(CpuLoad::percent(30.0)), None);
        assert_eq!(
            animation
                .update(CpuLoad::percent(32.0))
                .map(|change| change.fps),
            Some(15)
        );
        assert_eq!(animation.update(CpuLoad::percent(28.0)), None);
        assert_eq!(
            animation
                .update(CpuLoad::percent(1.0))
                .map(|change| change.fps),
            Some(5)
        );
        assert_eq!(animation.set_limit(FpsLimit::Fps10), None);
        assert_eq!(animation.set_limit(FpsLimit::Fps20), None);
    }

    #[test]
    fn c2_limit_lowering_immediately_reconfigures_a_running_animation() {
        let mut animation = AnimationController::new(FpsLimit::Fps40);
        assert_eq!(
            animation
                .update(CpuLoad::percent(100.0))
                .map(|change| change.fps),
            Some(40)
        );
        assert_eq!(
            animation
                .set_limit(FpsLimit::Fps10)
                .map(|change| change.fps),
            Some(10)
        );
    }

    #[test]
    fn c2_frame_cursor_covers_empty_and_wrap_cases() {
        assert_eq!(FrameCursor::new(0), None);
        let mut cursor = FrameCursor::new(3).expect("three frames are valid");
        assert_eq!(cursor.advance(), 1);
        assert_eq!(cursor.advance(), 2);
        assert_eq!(cursor.advance(), 0);
    }

    proptest! {
        #[test]
        fn pbt_rate_never_exceeds_limit_or_has_an_invalid_interval(
            cap_index in 0usize..4,
            loads in proptest::collection::vec(0.0f32..=100.0, 0..128),
        ) {
            let limit = FpsLimit::ALL[cap_index];
            let mut animation = AnimationController::new(limit);
            for load in loads {
                let _ = animation.update(CpuLoad::percent(load));
                prop_assert!(animation.current_fps() <= limit.fps());
                prop_assert!(animation.current_fps() >= 5);
                prop_assert_eq!(animation.current_interval_ms(), 1_000 / u32::from(animation.current_fps()));
            }
        }

        #[test]
        fn pbt_cursor_stays_in_range_after_any_number_of_steps(
            frame_count in 1usize..64,
            advances in 0usize..1_000,
        ) {
            let mut cursor = FrameCursor::new(frame_count).expect("positive count");
            for _ in 0..advances {
                let _ = cursor.advance();
            }
            prop_assert!(cursor.current() < cursor.frame_count());
            prop_assert_eq!(cursor.current(), advances % frame_count);
        }
    }
}
