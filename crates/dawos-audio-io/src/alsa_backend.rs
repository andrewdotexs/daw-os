//! The ALSA back end: the periods of the engine come from the sound
//! card (step P1.4).
//!
//! # Who touches the card
//!
//! One live thread at a time: the one that holds the first slot of the
//! engine. Its spare shares the same seat ([`AlsaShared`]), so that it
//! finds the card where the thread it replaces left it. The live thread
//! never blocks on a lock: it tries, and a seat it cannot have means a
//! cycle of silence.
//!
//! # When the card stops
//!
//! - An **xrun** (the engine was late and the driver stopped the
//!   streams): the live thread starts the card again, in the wait phase
//!   of its cycle, and goes on. A handful of system calls and some
//!   milliseconds, counted in the report.
//! - **The card is gone** (unplugged, or it refuses to work): the live
//!   thread puts it aside and keeps its cycle going on the clock, with
//!   silence in and nothing out. Closing the old card and opening a new
//!   one is slow work with many system calls: the keeper does it, a
//!   thread outside the live CPUs ([`keeper_loop`]), and leaves the new
//!   card where the live thread picks it up.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dawos_rt::clock::{self, Instant};

use crate::alsa_card::{Card, CardConfig, CardError, CardInfo, Ready};
use crate::{Backend, Disturbance, Format, Wake};

/// Periods without a sign of life after which a running card is taken
/// for gone.
const STALLS_BEFORE_GONE: u32 = 3;

/// Periods of input and output skipped to provoke an xrun on purpose.
const PERIODS_TO_SKIP: u32 = 4;

/// What happened to the card during a run. Written by the live thread
/// and by the keeper, read at the end.
#[derive(Default)]
pub struct CardStats {
    /// Cycles served from the card.
    pub card_cycles: AtomicU64,
    /// Cycles on the clock, without a card behind them.
    pub silent_cycles: AtomicU64,
    /// Times the driver stopped the streams because the engine was late.
    pub xruns: AtomicU64,
    /// Times the streams were started again.
    pub recoveries: AtomicU64,
    /// The longest a restart took, in nanoseconds.
    pub longest_recovery_ns: AtomicU64,
    /// Waits that ended with no period.
    pub stalls: AtomicU64,
    /// Times the card was given up.
    pub lost: AtomicU64,
    /// Times a newly opened card came into use.
    pub back: AtomicU64,
    /// Attempts of the keeper to open the card again.
    pub reopen_attempts: AtomicU64,
    /// Periods of output that did not fit in the buffer whole.
    pub short_writes: AtomicU64,
    /// Cycles with a period stamp that could not be believed.
    pub bad_stamps: AtomicU64,
    /// The error number that made the card be given up, the last time
    /// (0: none).
    pub last_error: AtomicI32,
    /// Times the card was given up because it had stopped giving
    /// periods.
    pub lost_to_stalls: AtomicU64,
}

/// The place of the card, for the one thread that uses it.
struct Seat {
    card: Option<Card>,
    /// A card given up, waiting for the keeper to take it away.
    dead: Option<Card>,
    /// Something went wrong while reading or writing: dealt with at the
    /// next wait, where system calls are allowed.
    pending: Option<CardError>,
    stalls: u32,
    /// Periods still to skip, to provoke an xrun (a test).
    skip: u32,
    /// Whether this cycle has no card behind it.
    silent: bool,
}

/// Between the live thread and the keeper.
#[derive(Default)]
struct Handoff {
    dead: Option<Card>,
    fresh: Option<Card>,
}

/// What the threads of the ALSA back end share.
pub struct AlsaShared {
    config: CardConfig,
    info: CardInfo,
    /// When the periods begin (0: not said yet).
    start: AtomicU64,
    seat: Mutex<Seat>,
    handoff: Mutex<Handoff>,
    /// The live thread wants a new card.
    wanted: AtomicBool,
    /// The keeper has one ready.
    ready: AtomicBool,
    /// The run is over.
    stop: AtomicBool,
    /// Number of the next cycle, and when the last one was due.
    next_index: AtomicU64,
    last_due: AtomicU64,
    stats: CardStats,
}

impl AlsaShared {
    /// Open the card. It does not run until a thread waits on it.
    pub fn open(config: CardConfig) -> Result<Arc<Self>, String> {
        let card = Card::open(&config)?;
        Ok(Arc::new(Self {
            info: card.info().clone(),
            config,
            start: AtomicU64::new(0),
            seat: Mutex::new(Seat {
                card: Some(card),
                dead: None,
                pending: None,
                stalls: 0,
                skip: 0,
                silent: true,
            }),
            handoff: Mutex::new(Handoff::default()),
            wanted: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            next_index: AtomicU64::new(0),
            last_due: AtomicU64::new(0),
            stats: CardStats::default(),
        }))
    }

    /// Say when the periods begin: the first wait sleeps until then, so
    /// that every thread of the engine is in place.
    pub fn set_start(&self, start: Instant) {
        self.start.store(start, Ordering::Release);
    }

    /// What the driver accepted when the card was first opened.
    #[must_use]
    pub const fn info(&self) -> &CardInfo {
        &self.info
    }

    /// The counters of the run.
    #[must_use]
    pub const fn stats(&self) -> &CardStats {
        &self.stats
    }

    /// Tell the keeper that the run is over.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    /// Whether a card is in use right now (also true while a thread is
    /// inside a call on it).
    #[must_use]
    pub fn has_card(&self) -> bool {
        self.seat
            .try_lock()
            .map(|seat| seat.card.is_some())
            .unwrap_or(true)
    }
}

/// The work that must stay off the live CPUs: close a card that was
/// given up, open it again. Runs until [`AlsaShared::stop`].
pub fn keeper_loop(shared: &AlsaShared) {
    let mut rounds_until_attempt = 0_u32;
    while !shared.stop.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(100));
        // Closing a card is slow: take it out under the lock, drop it
        // outside.
        let dead = shared
            .handoff
            .lock()
            .ok()
            .and_then(|mut handoff| handoff.dead.take());
        drop(dead);

        if !shared.wanted.load(Ordering::Acquire) {
            continue;
        }
        if rounds_until_attempt > 0 {
            rounds_until_attempt -= 1;
            continue;
        }
        shared.stats.reopen_attempts.fetch_add(1, Ordering::AcqRel);
        match Card::open(&shared.config) {
            Ok(card) if card.info() == &shared.info => {
                if let Ok(mut handoff) = shared.handoff.lock() {
                    handoff.fresh = Some(card);
                    shared.wanted.store(false, Ordering::Release);
                    shared.ready.store(true, Ordering::Release);
                }
            }
            // Not there yet, or back with another shape: the buffers of
            // the engine were made for the first one. Try again in half
            // a second.
            Ok(_) | Err(_) => rounds_until_attempt = 4,
        }
    }
    // The run is over: whatever is left in the hand-off is closed here.
    if let Ok(mut handoff) = shared.handoff.lock() {
        handoff.dead = None;
        handoff.fresh = None;
    }
}

/// The back end as one live thread holds it.
pub struct AlsaBackend {
    shared: Arc<AlsaShared>,
    format: Format,
    period_ns: u64,
    timeout_ms: u32,
}

impl AlsaBackend {
    /// A handle on the shared card for one thread.
    #[must_use]
    pub fn new(shared: Arc<AlsaShared>) -> Self {
        let format = shared.config.format;
        let period_ns = format.period_ns();
        Self {
            shared,
            format,
            period_ns,
            // Four periods and some: a card that runs never takes that
            // long, and a card that stopped is noticed soon.
            timeout_ms: u32::try_from(4 * period_ns / 1_000_000 + 10).unwrap_or(u32::MAX),
        }
    }

    /// One cycle on the clock: there is no card to wait for.
    fn clock_wake(&self, seat: &mut Seat) -> Wake {
        let last = self.shared.last_due.load(Ordering::Acquire);
        let now = clock::now();
        let mut due = if last == 0 {
            now + self.period_ns
        } else {
            last + self.period_ns
        };
        let mut missed = 0;
        while due <= now {
            due += self.period_ns;
            missed += 1;
        }
        clock::sleep_until(due);
        let woke = clock::now();
        seat.silent = true;
        self.shared
            .stats
            .silent_cycles
            .fetch_add(1, Ordering::AcqRel);
        self.wake(due, woke, missed)
    }

    fn wake(&self, due: Instant, woke: Instant, missed: u64) -> Wake {
        self.shared.last_due.store(due, Ordering::Release);
        let index = self
            .shared
            .next_index
            .fetch_add(1 + missed, Ordering::AcqRel)
            + missed;
        Wake {
            index,
            due,
            woke,
            missed,
        }
    }

    /// Put the card aside and ask the keeper for another.
    fn give_up(&self, seat: &mut Seat, error: Option<::alsa::Error>) {
        if let Some(card) = seat.card.take() {
            seat.dead = Some(card);
        }
        seat.pending = None;
        seat.stalls = 0;
        seat.skip = 0;
        let stats = &self.shared.stats;
        stats.lost.fetch_add(1, Ordering::AcqRel);
        match error {
            Some(error) => stats.last_error.store(error.errno(), Ordering::Release),
            None => {
                stats.lost_to_stalls.fetch_add(1, Ordering::AcqRel);
            }
        }
        self.shared.wanted.store(true, Ordering::Release);
    }

    /// Give the keeper the card that was put aside, and take the new one
    /// if it is ready. Never waits for the keeper.
    fn exchange(&self, seat: &mut Seat) {
        if seat.dead.is_none() && !self.shared.ready.load(Ordering::Acquire) {
            return;
        }
        let Ok(mut handoff) = self.shared.handoff.try_lock() else {
            return;
        };
        if handoff.dead.is_none() {
            handoff.dead = seat.dead.take();
        }
        if seat.card.is_none() {
            if let Some(card) = handoff.fresh.take() {
                seat.card = Some(card);
                self.shared.ready.store(false, Ordering::Release);
                self.shared.stats.back.fetch_add(1, Ordering::AcqRel);
            }
        }
    }

    /// Start the streams again after an xrun.
    fn recover(&self, seat: &mut Seat) {
        let stats = &self.shared.stats;
        stats.xruns.fetch_add(1, Ordering::AcqRel);
        seat.skip = 0;
        let before = clock::now();
        let result = match seat.card.as_mut() {
            Some(card) => card.recover(),
            None => return,
        };
        match result {
            Ok(()) => {
                stats.recoveries.fetch_add(1, Ordering::AcqRel);
                stats
                    .longest_recovery_ns
                    .fetch_max(clock::now().saturating_sub(before), Ordering::AcqRel);
            }
            Err(CardError::Gone(error)) => self.give_up(seat, Some(error)),
            Err(CardError::Xrun) => self.give_up(seat, None),
        }
    }
}

impl Backend for AlsaBackend {
    fn name(&self) -> &'static str {
        "alsa"
    }

    fn format(&self) -> Format {
        self.format
    }

    fn channels(&self) -> (usize, usize) {
        (self.shared.info.inputs, self.shared.info.outputs)
    }

    fn wait(&mut self) -> Wake {
        // Before the first period: everybody in place.
        if self.shared.last_due.load(Ordering::Acquire) == 0 {
            clock::sleep_until(self.shared.start.load(Ordering::Acquire));
        }
        let Ok(mut guard) = self.shared.seat.try_lock() else {
            // The seat is taken: only a thread that went wrong inside
            // this back end could hold it. Keep the cycle going.
            let due = clock::now() + self.period_ns;
            clock::sleep_until(due);
            return self.wake(due, clock::now(), 0);
        };
        let seat = &mut *guard;
        // At most a few rounds: every one either returns or changes the
        // state of the seat.
        for _ in 0..8 {
            self.exchange(seat);
            match seat.pending.take() {
                Some(CardError::Xrun) => self.recover(seat),
                Some(CardError::Gone(error)) => self.give_up(seat, Some(error)),
                None => {}
            }
            let Some(card) = seat.card.as_mut() else {
                return self.clock_wake(seat);
            };
            if !card.started() {
                match card.start() {
                    Ok(()) => {}
                    Err(error) => {
                        seat.pending = Some(error);
                        continue;
                    }
                }
            }
            match card.wait(self.timeout_ms) {
                Ok(Ready::Period(stamp)) => {
                    let woke = clock::now();
                    seat.stalls = 0;
                    seat.silent = false;
                    // The stamp is believed when it is in the past and
                    // not older than the wait itself.
                    let due = match stamp {
                        Some(stamp)
                            if stamp <= woke
                                && woke - stamp < u64::from(self.timeout_ms) * 1_000_000 =>
                        {
                            stamp
                        }
                        Some(_) => {
                            self.shared.stats.bad_stamps.fetch_add(1, Ordering::AcqRel);
                            woke
                        }
                        None => woke,
                    };
                    // Periods that went by unserved since the last wake:
                    // after an xrun, or after a stretch on the clock.
                    let last = self.shared.last_due.load(Ordering::Acquire);
                    let missed = if last == 0 {
                        0
                    } else {
                        ((due.saturating_sub(last) + self.period_ns / 2) / self.period_ns)
                            .saturating_sub(1)
                    };
                    self.shared.stats.card_cycles.fetch_add(1, Ordering::AcqRel);
                    return self.wake(due, woke, missed);
                }
                Ok(Ready::Timeout) => {
                    self.shared.stats.stalls.fetch_add(1, Ordering::AcqRel);
                    seat.stalls += 1;
                    if seat.stalls >= STALLS_BEFORE_GONE {
                        self.give_up(seat, None);
                        continue;
                    }
                    // The thread slept through the timeout: a cycle of
                    // silence, then wait again.
                    let woke = clock::now();
                    seat.silent = true;
                    self.shared
                        .stats
                        .silent_cycles
                        .fetch_add(1, Ordering::AcqRel);
                    return self.wake(woke, woke, 0);
                }
                Err(error) => seat.pending = Some(error),
            }
        }
        self.clock_wake(seat)
    }

    fn capture(&mut self, input: &mut [i32]) {
        let Ok(mut guard) = self.shared.seat.try_lock() else {
            input.fill(0);
            return;
        };
        let seat = &mut *guard;
        if seat.silent || seat.skip > 0 || seat.pending.is_some() {
            input.fill(0);
            return;
        }
        match seat.card.as_mut().map(|card| card.capture_into(input)) {
            Some(Ok(())) => {}
            Some(Err(error)) => {
                seat.pending = Some(error);
                input.fill(0);
            }
            None => input.fill(0),
        }
    }

    fn deliver(&mut self, output: &[i32]) {
        let Ok(mut guard) = self.shared.seat.try_lock() else {
            return;
        };
        let seat = &mut *guard;
        if seat.skip > 0 {
            seat.skip -= 1;
            return;
        }
        if seat.silent || seat.pending.is_some() {
            return;
        }
        let frames = output.len() / self.shared.info.outputs.max(1);
        match seat.card.as_mut().map(|card| card.deliver_from(output)) {
            Some(Ok(written)) if written < frames => {
                self.shared
                    .stats
                    .short_writes
                    .fetch_add(1, Ordering::AcqRel);
            }
            Some(Ok(_)) | None => {}
            Some(Err(error)) => seat.pending = Some(error),
        }
    }

    fn disturb(&mut self, disturbance: Disturbance) {
        let Ok(mut guard) = self.shared.seat.try_lock() else {
            return;
        };
        match disturbance {
            Disturbance::Xrun => guard.skip = PERIODS_TO_SKIP,
            Disturbance::CardLost => {
                if guard.card.is_some() {
                    guard.pending = Some(CardError::Gone(::alsa::Error::unsupported(
                        "test: the card is taken away",
                    )));
                }
            }
        }
    }
}
