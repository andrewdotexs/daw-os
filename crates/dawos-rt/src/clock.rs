//! Time, in nanoseconds of the monotonic clock.

use crate::sys;

/// An instant of the monotonic clock, in nanoseconds since boot.
pub type Instant = u64;

/// The current instant. Does not enter the kernel: allowed in the compute
/// phase of the cycle.
#[inline]
pub fn now() -> Instant {
    sys::monotonic_ns()
}

/// Sleep until `deadline`; returns at once if it has passed. A blocking
/// system call: wait phase only.
pub fn sleep_until(deadline: Instant) {
    sys::sleep_until_ns(deadline);
}

/// The instant at which period number `index` begins, for periods of
/// `frames` samples at `rate` hertz starting at `start`.
///
/// Computed from the index and not by adding a rounded period each time:
/// 128 samples at 48 kHz last 2 666 666.67 ns, and the two thirds of a
/// nanosecond would add up to a drift.
#[must_use]
pub fn period_start(start: Instant, index: u64, frames: u32, rate: u32) -> Instant {
    let elapsed = u128::from(index) * u128::from(frames) * 1_000_000_000 / u128::from(rate);
    start + elapsed as u64
}

/// The length of one period in nanoseconds, rounded down.
#[must_use]
pub fn period_ns(frames: u32, rate: u32) -> u64 {
    u64::from(frames) * 1_000_000_000 / u64::from(rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_advances() {
        let first = now();
        let second = now();
        assert!(second >= first);
    }

    #[test]
    fn periods_do_not_drift() {
        // 375 periods of 128 samples at 48 kHz are exactly one second.
        assert_eq!(period_start(1_000, 375, 128, 48_000), 1_000 + 1_000_000_000);
        // One period, rounded down.
        assert_eq!(period_start(0, 1, 128, 48_000), 2_666_666);
        assert_eq!(period_ns(128, 48_000), 2_666_666);
        assert_eq!(period_ns(256, 96_000), 2_666_666);
        assert_eq!(period_ns(512, 192_000), 2_666_666);
    }

    #[test]
    fn sleeping_until_the_past_returns_at_once() {
        let before = now();
        sleep_until(before.saturating_sub(1_000_000));
        assert!(now() - before < 50_000_000);
    }

    #[test]
    fn sleeping_reaches_the_deadline() {
        let deadline = now() + 2_000_000;
        sleep_until(deadline);
        assert!(now() >= deadline);
    }
}
