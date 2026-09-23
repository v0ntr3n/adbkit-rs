//! A counter that wraps around.
//!
//! Ported from `src/adb/tcpusb/rollingcounter.ts`.

/// A counter that rolls over back to its minimum value.
///
/// Ported from the `RollingCounter` class of `src/adb/tcpusb/rollingcounter.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollingCounter {
    now: u32,
    max: u32,
    min: u32,
}

impl RollingCounter {
    /// Creates a counter that rolls over between `min` and `max`.
    pub fn new(max: u32, min: u32) -> Self {
        RollingCounter { now: min, max, min }
    }

    /// Returns the next value of the counter.
    ///
    /// Ported from `RollingCounter.next()`.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u32 {
        if self.now >= self.max {
            self.now = self.min;
        }
        self.now += 1;
        self.now
    }

    /// The value the counter would return next.
    pub fn peek(&self) -> u32 {
        self.now + 1
    }
}

impl Default for RollingCounter {
    fn default() -> Self {
        RollingCounter::new(u32::MAX, 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counts_up() {
        let mut counter = RollingCounter::new(3, 1);
        assert_eq!(counter.next(), 2);
        assert_eq!(counter.next(), 3);
    }

    #[test]
    fn test_rolls_over() {
        let mut counter = RollingCounter::new(3, 1);
        assert_eq!(counter.next(), 2);
        assert_eq!(counter.next(), 3);
        assert_eq!(counter.next(), 2);
        assert_eq!(counter.next(), 3);
    }

    #[test]
    fn test_rolls_over_at_max() {
        let mut counter = RollingCounter::new(u32::MAX, 1);
        assert_eq!(counter.next(), 2);
        counter.now = u32::MAX - 1;
        assert_eq!(counter.next(), u32::MAX);
        assert_eq!(counter.next(), 2);
    }

    #[test]
    fn test_default() {
        let counter = RollingCounter::default();
        assert_eq!(counter.min, 1);
        assert_eq!(counter.max, u32::MAX);
    }
}
