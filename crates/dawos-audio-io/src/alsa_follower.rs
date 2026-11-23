//! The ALSA back end as the other live threads see it (step P1.6b).
//!
//! One live thread holds the card ([`crate::AlsaBackend`]). The others
//! must begin their cycle when it begins its own, and nobody may wake
//! them from the compute phase: that would be a system call where none
//! is allowed (ADR 0021, decision D-26). So each of them sleeps on the
//! card too: on a timer of the kernel bound to the input stream
//! ([`dawos_rt::pcm_timer`]), which ticks at the instant a period is
//! complete, the same instant the kernel wakes the first thread.
//!
//! The number of the cycle is the business of the thread that holds the
//! card. Once awake, a follower takes it from what the first thread
//! publishes in memory. It may be a moment ahead of the first: it looks
//! again for some microseconds, without entering the kernel.
//!
//! When nothing comes from the first thread (the card stopped, an xrun
//! is being recovered, the first thread was replaced) the follower does
//! not spin: it sleeps on the clock for a period and runs a cycle with
//! no number of its own, marked as on the clock. A thread that never
//! slept would be the fault the watchdog is there for.
//!
//! # Why not a `poll()` on the stream itself
//!
//! That was the first design, and the 285K said no. A `poll()` holds
//! while there is a period to read. The kernel wakes every sleeper when
//! the period is complete, and each of them looks again; the first
//! thread reads the period some microseconds after it wakes, and a
//! follower that looks after that finds nothing and stays asleep, until
//! its time runs out. With four threads on the Scarlett the followers
//! lost more than half their cycles that way, and the first thread woke in 8 us
//! instead of 3: four threads on the lock of one stream.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use dawos_rt::clock::{self, Instant};
use dawos_rt::pcm_timer::{PcmTimer, Tick};

use crate::alsa_backend::AlsaShared;
use crate::{Backend, Format, Wake};

/// How long a follower looks for the cycle of the first thread after
/// the card woke it, without entering the kernel. The first thread
/// wakes at the same instant, some microseconds apart at the most
/// (under 10 us in every run of the 285K): past this, it is not coming.
const LOOK_NS: u64 = 50_000;

/// Cycles between two attempts to open the timer of a card that
/// refused: half a second of cycles of 2 ms.
const TRY_AGAIN_AFTER: u32 = 250;

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
    /// The timer of the card in use, and which card it belongs to (see
    /// [`AlsaShared::card_in_use`]; 0: none).
    timer: Option<PcmTimer>,
    timer_of: u32,
    /// Cycles to let go by before trying a card that refused again.
    patience: u32,
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
            timer: None,
            timer_of: 0,
            patience: 0,
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

    /// Look for a new cycle of the first thread until `LOOK_NS` from
    /// now, without entering the kernel.
    fn look(&mut self) -> Option<Wake> {
        let until = clock::now() + LOOK_NS;
        loop {
            let now = clock::now();
            if let Some((index, due)) = self.newer() {
                let missed = match self.last {
                    Some((last, _)) => index - last - 1,
                    None => 0,
                };
                self.last = Some((index, due));
                let stats = self.shared.stats();
                stats.follower_cycles.fetch_add(1, Ordering::AcqRel);
                if missed > 0 {
                    stats.follower_behind.fetch_add(1, Ordering::AcqRel);
                }
                return Some(Wake {
                    index,
                    due,
                    woke: now,
                    missed,
                    input_ready: 0,
                    output_queued: 0,
                    events: 0,
                });
            }
            if now >= until {
                return None;
            }
            std::hint::spin_loop();
        }
    }

    /// Have the timer of the card that is in use now: at the first
    /// cycle, and after a card came back. Some system calls, in the
    /// wait phase, a few times in a run.
    fn keep_the_timer(&mut self) {
        let Some((which, (card, device, stream))) = self.shared.card_in_use() else {
            self.timer = None;
            self.timer_of = 0;
            return;
        };
        if self.timer.is_some() && self.timer_of == which {
            return;
        }
        if self.patience > 0 {
            self.patience -= 1;
            return;
        }
        let stats = self.shared.stats();
        match PcmTimer::open(card, device, stream, true) {
            Ok(timer) => {
                self.timer = Some(timer);
                self.timer_of = which;
                stats.timers_opened.fetch_add(1, Ordering::AcqRel);
            }
            Err(_) => {
                self.timer = None;
                self.patience = TRY_AGAIN_AFTER;
                stats.timers_refused.fetch_add(1, Ordering::AcqRel);
            }
        }
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
        // Before the first period: everybody in place.
        if !self.joining && self.last.is_none() {
            clock::sleep_until(self.shared.start());
            self.joining = true;
        }
        self.keep_the_timer();

        // ---- The blocking call of the cycle, and the reading of what
        // woke the thread. A tick is there for every period completed,
        // also for one completed while this thread was still at work:
        // then the sleep returns at once.
        match self.timer.as_ref().map(|timer| timer.wait(self.timeout_ms)) {
            // The first thread woke at the same instant and publishes
            // its cycle at once.
            Some(Tick::Periods(_)) => {
                if let Some(wake) = self.look() {
                    return wake;
                }
            }
            Some(Tick::Nothing) | None => {}
            Some(Tick::Gone) => self.timer = None,
        }

        // Nothing from the card, or nothing from the first thread.
        // Asleep until the next period would be due; the first thread
        // may be going on the clock, and then its cycle is there.
        self.shared
            .stats()
            .follower_empty
            .fetch_add(1, Ordering::AcqRel);
        let now = clock::now();
        let mut due = match self.last {
            Some((_, due)) => due + self.period_ns,
            None => now + self.period_ns,
        };
        while due <= now {
            due += self.period_ns;
        }
        clock::sleep_until(due);
        if let Some(wake) = self.look() {
            return wake;
        }
        // A cycle with no number of its own: the thread stays alive,
        // sees the end of the run, and never spins.
        let woke = clock::now();
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
