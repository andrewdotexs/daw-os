//! The ALSA back end as the other live threads see it (step P1.6b).
//!
//! One live thread holds the card ([`crate::AlsaBackend`]). The others
//! must begin their cycle when it begins its own, and nobody may wake
//! them from the compute phase: that would be a system call where none
//! is allowed (ADR 0021, decision D-26). So each of them sleeps on the
//! card too, with a `poll()` on the descriptor of the input stream: the
//! kernel wakes every sleeper when a period of input is complete, the
//! first thread and these at the same instant.
//!
//! What a `poll()` says is "there is a period to read", not "a period
//! went by", and the number of the cycle is the business of the thread
//! that holds the card. Two things follow:
//!
//! - once awake, a follower takes the cycle from what the first thread
//!   publishes in memory. It may be a moment ahead of it: it looks
//!   again for some microseconds, without entering the kernel;
//! - before sleeping, it makes sure the first thread has taken the
//!   input of the cycle in course: until then the `poll()` would return
//!   at once.
//!
//! When nothing comes from the first thread (the card stopped, an xrun
//! is being recovered, the first thread was replaced) the follower does
//! not spin: it sleeps on the clock for a period and runs a cycle with
//! no number of its own, marked as on the clock. A thread that never
//! slept would be the fault the watchdog is there for.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use ::alsa::poll::{poll, pollfd, Flags};
use dawos_rt::clock::{self, Instant};

use crate::alsa_backend::AlsaShared;
use crate::{Backend, Format, Wake};

/// How long a follower looks for the cycle of the first thread after
/// the card woke it, without entering the kernel. The first thread
/// wakes at the same instant, some microseconds apart at the most
/// (under 10 us in every run of the 285K): past this, it is not coming.
const LOOK_NS: u64 = 50_000;

/// How long it looks, before sleeping, for the first thread to have
/// taken the input: some microseconds after its wake-up, in a cycle
/// that is going well.
const INPUT_TAKEN_NS: u64 = 20_000;

/// The back end of a live thread that does not hold the card.
pub struct AlsaFollower {
    shared: Arc<AlsaShared>,
    format: Format,
    period_ns: u64,
    timeout_ms: i32,
    /// The last cycle taken, and when it was due.
    last: Option<(u64, Instant)>,
    /// Whether the periods have already begun (a spare thread).
    joining: bool,
}

impl AlsaFollower {
    /// A follower of the card `shared`. `joining`: the periods have
    /// already begun, do not wait for their start.
    #[must_use]
    pub fn new(shared: Arc<AlsaShared>, joining: bool) -> Self {
        let format = shared.format();
        let period_ns = format.period_ns();
        Self {
            shared,
            format,
            period_ns,
            // As the first thread: four periods and some.
            timeout_ms: i32::try_from(4 * period_ns / 1_000_000 + 10).unwrap_or(i32::MAX),
            last: None,
            joining,
        }
    }

    /// The cycle of the first thread, if it is one this thread has not
    /// had.
    fn newer(&self) -> Option<(u64, Instant)> {
        let (index, due) = self.shared.published()?;
        match self.last {
            Some((last, _)) if index <= last => None,
            _ => Some((index, due)),
        }
    }

    fn take(&mut self, index: u64, due: Instant, woke: Instant) -> Wake {
        let missed = match self.last {
            Some((last, _)) => index - last - 1,
            None => 0,
        };
        self.last = Some((index, due));
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

    /// Sleep until the card has a period of input. `false`: there is no
    /// card, or the time ran out, or the sleep was cut short.
    fn sleep_on_the_card(&self) -> bool {
        let Some(fd) = self.shared.poll_fd() else {
            return false;
        };
        let mut descriptors = [pollfd {
            fd,
            events: Flags::IN.bits(),
            revents: 0,
        }];
        matches!(poll(&mut descriptors, self.timeout_ms), Ok(ready) if ready > 0)
    }
}

impl Backend for AlsaFollower {
    fn name(&self) -> &'static str {
        "alsa"
    }

    fn format(&self) -> Format {
        self.format
    }

    fn wait(&mut self) -> Wake {
        let shared = &*self.shared;
        // Before the first period: everybody in place.
        if !self.joining && self.last.is_none() {
            clock::sleep_until(shared.start());
            self.joining = true;
        }

        // Behind already: the newest cycle, without sleeping.
        if let Some((index, due)) = self.newer() {
            shared
                .stats()
                .follower_behind
                .fetch_add(1, Ordering::AcqRel);
            return self.take(index, due, clock::now());
        }

        // The first thread takes the input some microseconds after its
        // wake-up; a thread with little to do can be back here before
        // that. No system call: it is still the tail of the compute
        // phase.
        if let Some((last, _)) = self.last {
            let until = clock::now() + INPUT_TAKEN_NS;
            while !shared.has_consumed(last) && clock::now() < until {
                std::hint::spin_loop();
            }
        }

        // ---- The one blocking call of the cycle.
        let woken = self.sleep_on_the_card();

        // The first thread woke at the same instant and publishes its
        // cycle at once.
        if woken {
            let until = clock::now() + LOOK_NS;
            loop {
                let now = clock::now();
                if let Some((index, due)) = self.newer() {
                    shared
                        .stats()
                        .follower_cycles
                        .fetch_add(1, Ordering::AcqRel);
                    return self.take(index, due, now);
                }
                if now >= until {
                    break;
                }
                std::hint::spin_loop();
            }
        }

        // Nothing from the first thread. Asleep until the next period
        // would be due, then a cycle on the clock: the thread stays
        // alive, sees the end of the run, and never spins.
        shared.stats().follower_empty.fetch_add(1, Ordering::AcqRel);
        let now = clock::now();
        let mut due = match self.last {
            Some((_, due)) => due + self.period_ns,
            None => now + self.period_ns,
        };
        while due <= now {
            due += self.period_ns;
        }
        clock::sleep_until(due);
        let woke = clock::now();
        // If the first thread came back meanwhile, its cycle is taken at
        // the next wait. This one has no number of its own.
        let index = self.last.map_or(0, |(index, _)| index);
        if let Some((_, last_due)) = &mut self.last {
            *last_due = due;
        }
        Wake {
            index,
            due,
            woke,
            missed: 0,
            input_ready: 0,
            output_queued: 0,
            events: Wake::ON_THE_CLOCK,
        }
    }
}
