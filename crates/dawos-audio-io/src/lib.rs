//! The audio back ends of the engine, behind one interface (question
//! D-17, decided on 21 November 2026).
//!
//! A back end is what gives the cycle its rhythm: it blocks the calling
//! thread until the next period is due. That one call is the wait phase
//! of the cycle (ADR 0021).
//!
//! - `null`: no sound card, a timer of the calling CPU. For the logic of
//!   the engine and for the protection against a thread that does not
//!   yield; not for latency figures, because its wake-up path is not the
//!   one of a sound card.
//! - `alsa`: a sound card, through alsa-lib (question D-27): the device
//!   itself, memory-mapped, capture and playback linked, two periods of
//!   buffer. One live thread waits on it; how the others will is decided
//!   with the measurements of step P1.6.
//! - `file`: step P1.7.
//!
//! Samples cross this interface as the card has them: 32-bit signed
//! integers, all the channels of a frame one after the other.

#![forbid(unsafe_code)]

mod alsa_backend;
mod alsa_card;
mod null;

pub use alsa_backend::{keeper_loop, AlsaBackend, AlsaShared, CardStats};
pub use alsa_card::{CardConfig, CardInfo, XrunView};
pub use null::NullBackend;

use dawos_rt::clock::Instant;

/// Sample rate and length of the quantum: what fixes the period.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    /// Samples per second.
    pub rate: u32,
    /// Samples per period, the quantum of the engine.
    pub frames: u32,
}

impl Format {
    /// The quantum that lasts 2.667 ms at `rate`: 128 samples up to
    /// 48 kHz, 256 up to 96 kHz, 512 above (requirement R-01).
    #[must_use]
    pub const fn default_frames(rate: u32) -> u32 {
        if rate <= 48_000 {
            128
        } else if rate <= 96_000 {
            256
        } else {
            512
        }
    }

    /// Length of one period in nanoseconds, rounded down.
    #[must_use]
    pub fn period_ns(self) -> u64 {
        dawos_rt::clock::period_ns(self.frames, self.rate)
    }
}

/// What a back end reports when a period begins.
#[derive(Clone, Copy, Debug)]
pub struct Wake {
    /// Number of the period, from 0.
    pub index: u64,
    /// When the period was due.
    pub due: Instant,
    /// When the thread was running again.
    pub woke: Instant,
    /// Periods that went by unserved since the last wake-up: more than
    /// zero means the cycle before this one took longer than a period.
    pub missed: u64,
}

impl Wake {
    /// How long after the period was due the thread was running, in
    /// nanoseconds.
    #[must_use]
    pub const fn lateness_ns(&self) -> u64 {
        self.woke.saturating_sub(self.due)
    }
}

/// A source of periods.
pub trait Backend {
    /// Short name, as on the command line.
    fn name(&self) -> &'static str;

    /// Rate and quantum in use.
    fn format(&self) -> Format;

    /// Block until the next period is due. The only blocking call of the
    /// cycle: wait phase.
    fn wait(&mut self) -> Wake;

    /// Channels of input and of output; none for a back end without
    /// sound.
    fn channels(&self) -> (usize, usize) {
        (0, 0)
    }

    /// The input of the period that just began, one quantum, all
    /// channels interleaved. Silence when there is none.
    fn capture(&mut self, input: &mut [i32]) {
        input.fill(0);
    }

    /// The output of this period, one quantum, all channels interleaved.
    /// To be called before the period ends.
    fn deliver(&mut self, _output: &[i32]) {}

    /// Make something go wrong on purpose (tests of the recovery).
    fn disturb(&mut self, _disturbance: Disturbance) {}
}

/// What can be made to go wrong in a back end, to see it recover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disturbance {
    /// Sleep through some periods without serving the card: the driver
    /// stops the streams, as when the engine is late.
    Xrun,
    /// Behave as if the card had been unplugged.
    CardLost,
}
