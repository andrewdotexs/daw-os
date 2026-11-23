//! The back end without a sound card.

use dawos_rt::clock::{self, Instant};

use crate::{Backend, Format, Wake};

/// Periods timed by the monotonic clock: every `wait` sleeps until the
/// next multiple of the period since the first call.
pub struct NullBackend {
    format: Format,
    start: Option<Instant>,
    next: u64,
}

impl NullBackend {
    /// A back end for `format`. Nothing runs until the first `wait`.
    #[must_use]
    pub const fn new(format: Format) -> Self {
        Self {
            format,
            start: None,
            next: 0,
        }
    }

    /// A back end whose periods are counted from `start`: several
    /// threads, each with one of its own, then wake up for the same
    /// periods (question D-26: every live thread makes its own wait).
    #[must_use]
    pub const fn with_start(format: Format, start: Instant) -> Self {
        Self {
            format,
            start: Some(start),
            next: 0,
        }
    }

    /// Like `with_start`, for a thread that joins while the periods are
    /// already running: the periods gone by before now are not reported
    /// as missed.
    #[must_use]
    pub fn joining(format: Format, start: Instant) -> Self {
        let elapsed = clock::now().saturating_sub(start);
        let next = u128::from(elapsed) * u128::from(format.rate)
            / (u128::from(format.frames) * 1_000_000_000);
        Self {
            format,
            start: Some(start),
            next: next as u64,
        }
    }

    fn due(&self, start: Instant, index: u64) -> Instant {
        clock::period_start(start, index, self.format.frames, self.format.rate)
    }
}

impl Backend for NullBackend {
    fn name(&self) -> &'static str {
        "null"
    }

    fn format(&self) -> Format {
        self.format
    }

    fn wait(&mut self) -> Wake {
        // The first period is due one period after the first call, so
        // that the first wake-up is a real one.
        let start = *self.start.get_or_insert_with(clock::now);
        let mut index = self.next;
        let mut due = self.due(start, index + 1);

        // Periods already over when we get here were missed: the caller
        // took too long. Skip to the first one still to come, as a sound
        // card would have gone on without us.
        let now = clock::now();
        let mut missed = 0;
        while due <= now {
            missed += 1;
            index += 1;
            due = self.due(start, index + 1);
        }

        clock::sleep_until(due);
        let woke = clock::now();
        self.next = index + 1;
        Wake {
            index,
            due,
            woke,
            missed,
            input_ready: 0,
            output_queued: 0,
            events: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A short period, so that the tests take milliseconds.
    const FAST: Format = Format {
        rate: 48_000,
        frames: 48,
    };

    #[test]
    fn periods_come_in_order_and_on_time() {
        let mut backend = NullBackend::new(FAST);
        let mut last: Option<(u64, u64)> = None;
        for _ in 0..20 {
            let wake = backend.wait();
            assert!(wake.woke >= wake.due);
            // A machine that is busy with something else can keep this
            // thread away for more than a period of one millisecond:
            // then periods are missed, and said to be. What must hold
            // either way is that numbers and instants go together.
            match last {
                None => assert_eq!((wake.index, wake.missed), (0, 0)),
                Some((index, due)) => {
                    assert_eq!(wake.index, index + 1 + wake.missed);
                    // Exactly one millisecond a period: 48 samples at
                    // 48 kHz.
                    assert_eq!(wake.due - due, (1 + wake.missed) * 1_000_000);
                }
            }
            last = Some((wake.index, wake.due));
        }
    }

    #[test]
    fn a_slow_caller_is_told_what_it_missed() {
        let mut backend = NullBackend::new(FAST);
        let first = backend.wait();
        // Stay away for three and a half periods.
        clock::sleep_until(first.due + 3_500_000);
        let second = backend.wait();
        assert_eq!(second.missed, 3);
        assert_eq!(second.index, 4);
        assert_eq!(second.due, first.due + 4_000_000);
    }

    #[test]
    fn two_back_ends_with_one_start_share_their_periods() {
        let start = clock::now();
        let mut first = NullBackend::with_start(FAST, start);
        let mut second = NullBackend::with_start(FAST, start);
        let a = first.wait();
        let b = second.wait();
        // The second call comes after the first period is over: it gets
        // the next one, on the same grid.
        assert_eq!(a.due, start + 1_000_000);
        assert_eq!((b.due - start) % 1_000_000, 0);
    }

    #[test]
    fn a_late_joiner_has_missed_nothing() {
        let start = clock::now();
        clock::sleep_until(start + 5_500_000);
        let mut backend = NullBackend::joining(FAST, start);
        let wake = backend.wait();
        assert_eq!(wake.missed, 0);
        assert_eq!(wake.index, 5);
        assert_eq!(wake.due, start + 6_000_000);
    }

    #[test]
    fn the_default_quantum_lasts_the_same_at_every_rate() {
        for (rate, frames) in [
            (44_100, 96),
            (48_000, 96),
            (88_200, 192),
            (96_000, 192),
            (192_000, 384),
        ] {
            assert_eq!(Format::default_frames(rate), frames);
        }
        assert_eq!(
            Format {
                rate: 96_000,
                frames: 256
            }
            .period_ns(),
            2_666_666
        );
    }
}
