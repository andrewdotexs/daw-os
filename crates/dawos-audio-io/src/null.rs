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
        let mut last_due = 0;
        for expected in 0..20 {
            let wake = backend.wait();
            assert_eq!(wake.index, expected);
            assert_eq!(wake.missed, 0);
            assert!(wake.woke >= wake.due);
            assert!(wake.due > last_due);
            if expected > 0 {
                // Exactly one millisecond apart: 48 samples at 48 kHz.
                assert_eq!(wake.due - last_due, 1_000_000);
            }
            last_due = wake.due;
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
    fn the_default_quantum_lasts_the_same_at_every_rate() {
        for (rate, frames) in [
            (44_100, 128),
            (48_000, 128),
            (88_200, 256),
            (96_000, 256),
            (192_000, 512),
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
