/// Fixed-capacity utilisation history for the hover sparkline.
///
/// Values are stored as whole percents. The buffer is a ring so a sample never
/// allocates, and chronological order is reconstructed on read.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Sparkline {
    values: [u8; SPARKLINE_CAPACITY],
    len: u8,
    start: u8,
}

/// One minute of 2-second samples.
pub const SPARKLINE_CAPACITY: usize = 30;

impl Sparkline {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            values: [0; SPARKLINE_CAPACITY],
            len: 0,
            start: 0,
        }
    }

    pub fn push(&mut self, percent: f32) {
        let value = if percent.is_finite() {
            percent.round().clamp(0.0, 100.0) as u8
        } else {
            0
        };
        let len = self.len as usize;
        if len < SPARKLINE_CAPACITY {
            self.values[len] = value;
            self.len = (len + 1) as u8;
            return;
        }
        self.values[self.start as usize] = value;
        self.start = ((self.start as usize + 1) % SPARKLINE_CAPACITY) as u8;
    }

    #[must_use]
    pub const fn len(self) -> usize {
        self.len as usize
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    /// Chronological points, oldest first. Unused slots past `len` are zero.
    #[must_use]
    pub fn copy_points(self) -> ([u8; SPARKLINE_CAPACITY], usize) {
        let mut points = [0_u8; SPARKLINE_CAPACITY];
        let len = self.len as usize;
        let start = self.start as usize;
        for (index, slot) in points.iter_mut().take(len).enumerate() {
            *slot = self.values[(start + index) % SPARKLINE_CAPACITY];
        }
        (points, len)
    }
}

#[cfg(test)]
mod tests {
    use super::{Sparkline, SPARKLINE_CAPACITY};

    #[test]
    fn component_sparkline_keeps_insertion_order_until_capacity() {
        let mut line = Sparkline::new();
        line.push(10.4);
        line.push(20.5);
        line.push(f32::NAN);
        let (points, len) = line.copy_points();
        assert_eq!(len, 3);
        assert_eq!(&points[..3], &[10, 21, 0]);
    }

    #[test]
    fn component_sparkline_drops_the_oldest_point_after_capacity() {
        let mut line = Sparkline::new();
        for index in 0..=SPARKLINE_CAPACITY {
            line.push(index as f32);
        }
        let (points, len) = line.copy_points();
        assert_eq!(len, SPARKLINE_CAPACITY);
        assert_eq!(points[0], 1);
        assert_eq!(points[SPARKLINE_CAPACITY - 1], SPARKLINE_CAPACITY as u8);
    }

    #[test]
    fn c2_sparkline_clamps_out_of_range_and_non_finite_percents() {
        let mut line = Sparkline::new();
        line.push(-4.0);
        line.push(140.0);
        line.push(f32::INFINITY);
        let (points, len) = line.copy_points();
        assert_eq!(&points[..len], &[0, 100, 0]);
    }
}
