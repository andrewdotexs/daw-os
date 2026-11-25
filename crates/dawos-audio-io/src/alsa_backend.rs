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
//! The other live threads never touch the card: each sleeps on a timer
//! of the kernel that ticks when the card completes a period, to wake
//! when the first wakes, and takes the number of the cycle from what
//! the first publishes here ([`crate::AlsaFollower`], step P1.6b).
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

use crate::alsa_card::{Card, CardConfig, CardError, CardInfo, Ready, XrunView};
use crate::{Backend, Disturbance, Format, Wake};

/// Periods without a sign of life after which a running card is taken
/// for gone.
const STALLS_BEFORE_GONE: u32 = 3;

/// How long to leave the card alone before starting it again, when the
/// last start gave not one period of input. An interface that is
/// changing its clock stops sending for a second or so: starting it
/// again at once, to see it stop two periods later, is a storm of xruns
/// (ninety in 0.8 s on the Scarlett 18i20 at 192 kHz, step P1.4).
const PAUSE_BEFORE_RESTART_NS: u64 = 50_000_000;

/// How many xruns are kept one by one, from the first.
const XRUNS_KEPT: usize = 12;

/// Periods the live thread sleeps through, away from the card, to
/// provoke an xrun on purpose, on top of those the buffers hold.
const PERIODS_TO_SKIP: u64 = 2;

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
    /// Those of them that fell in the warm-up.
    pub early_xruns: AtomicU64,
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
    /// Restarts made after a pause, because the start before had given
    /// no input at all.
    pub paused_restarts: AtomicU64,
    /// Periods of output that did not fit in the buffer whole.
    pub short_writes: AtomicU64,
    /// Those of them that fell in the warm-up.
    pub early_short_writes: AtomicU64,
    /// The first of them: nanoseconds from the start of the run (0:
    /// there was none).
    pub first_short_ns: AtomicU64,
    /// The first of them: periods served since the streams were last
    /// started (1: it was the first).
    pub first_short_period: AtomicU64,
    /// The first of them: frames the card took of the period.
    pub first_short_frames: AtomicU64,
    /// Cycles with a period stamp that could not be believed.
    pub bad_stamps: AtomicU64,
    /// The error number that made the card be given up, the last time
    /// (0: none).
    pub last_error: AtomicI32,
    /// Times the card was given up because it had stopped giving
    /// periods.
    pub lost_to_stalls: AtomicU64,
    /// Cycles the other live threads took after sleeping on the timer
    /// of the card.
    pub follower_cycles: AtomicU64,
    /// Those of them that came with periods gone by: the thread was
    /// behind.
    pub follower_behind: AtomicU64,
    /// Times they were woken, or their time ran out, with no new cycle
    /// from the first thread: the card had stopped, or the first thread
    /// was not there. A cycle on the clock followed.
    pub follower_empty: AtomicU64,
    /// Times one of them opened the timer of the card: once each, and
    /// once more for every card that came back.
    pub timers_opened: AtomicU64,
    /// Times the kernel refused that.
    pub timers_refused: AtomicU64,
}

/// The cycle the first thread is in, for the others to read: a number
/// and the instant it was due, written so that the two are never read
/// from two different cycles. One writer, the thread that holds the
/// seat; any number of readers, none of which waits.
#[derive(Default)]
struct Published {
    /// Odd while the writer is at it.
    sequence: AtomicU64,
    /// The number of the cycle, plus one; 0: none yet.
    index: AtomicU64,
    due: AtomicU64,
}

impl Published {
    fn write(&self, index: u64, due: Instant) {
        let sequence = self.sequence.load(Ordering::Relaxed);
        self.sequence.store(sequence + 1, Ordering::Release);
        self.index.store(index + 1, Ordering::Release);
        self.due.store(due, Ordering::Release);
        self.sequence.store(sequence + 2, Ordering::Release);
    }

    /// The cycle and when it was due; `None`: none yet, or the writer
    /// was at it every time this looked (a few tries, never a wait).
    fn read(&self) -> Option<(u64, Instant)> {
        for _ in 0..8 {
            let before = self.sequence.load(Ordering::Acquire);
            if before & 1 == 0 {
                let index = self.index.load(Ordering::Acquire);
                let due = self.due.load(Ordering::Acquire);
                if self.sequence.load(Ordering::Acquire) == before {
                    return index.checked_sub(1).map(|index| (index, due));
                }
            }
            std::hint::spin_loop();
        }
        None
    }
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
    /// Periods of input since the card was last started.
    served: u64,
    /// Until when to stay away from the card, to provoke an xrun (a
    /// test); 0: not asked.
    skip_until: Instant,
    /// Whether this cycle has no card behind it.
    silent: bool,
    /// What happened since the last wake-up, for the next one to tell
    /// (the flags of [`Wake`]).
    events: u8,
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
    /// How long after that the card is still taken to be settling, in
    /// nanoseconds: xruns in there are counted apart.
    warm_up_ns: AtomicU64,
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
    /// The cycle in course, for the threads that follow.
    published: Published,
    /// The card in use, for the threads that open its timer: how many
    /// cards came into use so far in the high half (0: none now), and
    /// in the low half sound card, device and stream of the input as
    /// the kernel numbers them, 10 bits each.
    in_use: AtomicU64,
    stats: CardStats,
    /// The first xruns: when, in nanoseconds from the start, and how the
    /// streams stood.
    xrun_log: Mutex<Vec<(u64, XrunView)>>,
}

impl AlsaShared {
    /// Open the card. It does not run until a thread waits on it.
    pub fn open(config: CardConfig) -> Result<Arc<Self>, String> {
        let card = Card::open(&config)?;
        Ok(Arc::new(Self {
            in_use: AtomicU64::new(in_use(1, card.place())),
            published: Published::default(),
            info: card.info().clone(),
            config,
            start: AtomicU64::new(0),
            warm_up_ns: AtomicU64::new(0),
            seat: Mutex::new(Seat {
                card: Some(card),
                dead: None,
                pending: None,
                stalls: 0,
                served: 0,
                skip_until: 0,
                silent: true,
                events: 0,
            }),
            handoff: Mutex::new(Handoff::default()),
            wanted: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            next_index: AtomicU64::new(0),
            last_due: AtomicU64::new(0),
            stats: CardStats::default(),
            xrun_log: Mutex::new(Vec::with_capacity(XRUNS_KEPT)),
        }))
    }

    /// Say when the periods begin: the first wait sleeps until then, so
    /// that every thread of the engine is in place.
    pub fn set_start(&self, start: Instant) {
        self.start.store(start, Ordering::Release);
    }

    /// Say for how long after the start the card is taken to be
    /// settling (an interface that changes its clock needs some
    /// seconds): the xruns of that time are counted apart.
    pub fn set_warm_up(&self, nanoseconds: u64) {
        self.warm_up_ns.store(nanoseconds, Ordering::Release);
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

    /// Rate and quantum the card was opened with.
    pub(crate) const fn format(&self) -> Format {
        self.config.format
    }

    /// When the periods begin (0: not said yet).
    pub(crate) fn start(&self) -> Instant {
        self.start.load(Ordering::Acquire)
    }

    /// The cycle the first thread is in, and when it was due.
    pub(crate) fn published(&self) -> Option<(u64, Instant)> {
        self.published.read()
    }

    /// The card in use: which one it is since the start (1, 2, ...),
    /// and sound card, device and stream of its input. `None`: no card
    /// just now, or one whose place the driver did not tell.
    pub(crate) fn card_in_use(&self) -> Option<(u32, (u32, u32, u32))> {
        let word = self.in_use.load(Ordering::Acquire);
        let which = (word >> 32) as u32;
        let place = word as u32;
        (which != 0 && place & PLACE_KNOWN != 0).then_some((
            which,
            ((place >> 20) & 0x3ff, (place >> 10) & 0x3ff, place & 0x3ff),
        ))
    }

    /// Whether the other live threads can wake with this card: its
    /// place is known, and the timer of its input stream opens. Some
    /// system calls: to be asked before the cycle starts.
    pub fn followers_can_wake(&self) -> Result<(), String> {
        let Some((_, (card, device, stream))) = self.card_in_use() else {
            return Err("the driver did not tell where the stream is".to_string());
        };
        dawos_rt::pcm_timer::PcmTimer::open(card, device, stream, true)
            .map(drop)
            .map_err(|error| format!("the timer of the input stream does not open: {error}"))
    }

    /// The first xruns of the run: when, in nanoseconds from the start
    /// of the periods, and how the two streams stood.
    #[must_use]
    pub fn xrun_log(&self) -> Vec<(u64, XrunView)> {
        self.xrun_log
            .lock()
            .map(|log| log.clone())
            .unwrap_or_default()
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

/// Set in the low half of `AlsaShared::in_use` when the place is known.
const PLACE_KNOWN: u32 = 1 << 30;

/// The word of `AlsaShared::in_use` for the card number `which` since
/// the start, at `place`. A place that does not fit 10 bits a number is
/// a place not known.
fn in_use(which: u32, place: Option<(u32, u32, u32)>) -> u64 {
    let low = match place {
        Some((card, device, stream)) if card < 1024 && device < 1024 && stream < 1024 => {
            PLACE_KNOWN | card << 20 | device << 10 | stream
        }
        _ => 0,
    };
    u64::from(which) << 32 | u64::from(low)
}

/// Whether the kernel has the sound card an ALSA device name like
/// `hw:USB,0` speaks of. A name of another shape is taken as present:
/// opening it will tell.
fn card_is_there(device: &str) -> bool {
    let Some(rest) = device
        .strip_prefix("hw:")
        .or_else(|| device.strip_prefix("plughw:"))
    else {
        return true;
    };
    let card = rest.split(',').next().unwrap_or_default();
    if card.is_empty() {
        return true;
    }
    let directory = if card.bytes().all(|byte| byte.is_ascii_digit()) {
        format!("/proc/asound/card{card}")
    } else {
        format!("/proc/asound/{card}")
    };
    std::path::Path::new(&directory).exists()
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
        // Not there at all: nothing to try, and alsa-lib would write its
        // complaint on the terminal at every attempt.
        if !card_is_there(&shared.config.playback) {
            rounds_until_attempt = 1;
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
        let events = std::mem::take(&mut seat.events) | Wake::ON_THE_CLOCK;
        self.wake(due, woke, missed, (0, 0), events)
    }

    /// `fill`: frames of input ready and of output still to play.
    fn wake(&self, due: Instant, woke: Instant, missed: u64, fill: (u32, u32), events: u8) -> Wake {
        self.shared.last_due.store(due, Ordering::Release);
        let index = self
            .shared
            .next_index
            .fetch_add(1 + missed, Ordering::AcqRel)
            + missed;
        // For the threads that follow, at once: they woke with this one
        // and are looking for it.
        self.shared.published.write(index, due);
        Wake {
            index,
            due,
            woke,
            missed,
            input_ready: fill.0,
            output_queued: fill.1,
            events,
        }
    }

    /// Put the card aside and ask the keeper for another.
    fn give_up(&self, seat: &mut Seat, error: Option<::alsa::Error>) {
        if let Some(card) = seat.card.take() {
            seat.dead = Some(card);
        }
        // No card now; the count of the cards that came stays.
        let which = self.shared.in_use.load(Ordering::Acquire) >> 32;
        self.shared.in_use.store(which << 32, Ordering::Release);
        seat.pending = None;
        seat.stalls = 0;
        seat.skip_until = 0;
        seat.events |= Wake::CARD_LOST;
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
                let which = (self.shared.in_use.load(Ordering::Acquire) >> 32) as u32;
                self.shared
                    .in_use
                    .store(in_use(which + 1, card.place()), Ordering::Release);
                seat.card = Some(card);
                seat.events |= Wake::CARD_BACK;
                self.shared.ready.store(false, Ordering::Release);
                self.shared.stats.back.fetch_add(1, Ordering::AcqRel);
            }
        }
    }

    /// Start the streams again after an xrun.
    fn recover(&self, seat: &mut Seat) {
        let stats = &self.shared.stats;
        stats.xruns.fetch_add(1, Ordering::AcqRel);
        let since_start = clock::now().saturating_sub(self.shared.start.load(Ordering::Acquire));
        if since_start < self.shared.warm_up_ns.load(Ordering::Acquire) {
            stats.early_xruns.fetch_add(1, Ordering::AcqRel);
        }
        seat.skip_until = 0;
        seat.events |= Wake::AFTER_XRUN;
        // The start before this xrun gave nothing: the card is not
        // ready. Give it time, asleep, before the next start.
        if seat.served == 0 && stats.recoveries.load(Ordering::Acquire) > 0 {
            stats.paused_restarts.fetch_add(1, Ordering::AcqRel);
            clock::sleep_until(clock::now() + PAUSE_BEFORE_RESTART_NS);
        }
        seat.served = 0;
        let before = clock::now();
        let result = match seat.card.as_mut() {
            Some(card) => {
                // Keep the first ones, to tell which stream ran out.
                // The room is there already: nothing grows here.
                if let Ok(mut log) = self.shared.xrun_log.try_lock() {
                    if log.len() < log.capacity() {
                        let since =
                            before.saturating_sub(self.shared.start.load(Ordering::Acquire));
                        log.push((since, card.look()));
                    }
                }
                card.recover()
            }
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
            return self.wake(due, clock::now(), 0, (0, 0), Wake::ON_THE_CLOCK);
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
            // A test: sleep while the card runs unserved. Waiting on the
            // card would not do: it has input to give, and would not
            // let the thread sleep.
            if seat.skip_until != 0 {
                clock::sleep_until(seat.skip_until);
                seat.skip_until = 0;
            }
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
                Ok(Ready::Period {
                    stamp,
                    input_ready,
                    output_queued,
                }) => {
                    let woke = clock::now();
                    seat.stalls = 0;
                    seat.served += 1;
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
                    let events = std::mem::take(&mut seat.events);
                    return self.wake(due, woke, missed, (input_ready, output_queued), events);
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
                    let events = std::mem::take(&mut seat.events) | Wake::STALLED;
                    return self.wake(woke, woke, 0, (0, 0), events);
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
        if seat.silent || seat.pending.is_some() {
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
        if seat.silent || seat.pending.is_some() {
            return;
        }
        let frames = output.len() / self.shared.info.outputs.max(1);
        match seat.card.as_mut().map(|card| card.deliver_from(output)) {
            Some(Ok(written)) if written < frames => {
                // The frames that did not fit are not played: said, and
                // said when, because at the start of the streams it is
                // the interface that is not taking samples yet.
                let stats = &self.shared.stats;
                let since_start = clock::now()
                    .saturating_sub(self.shared.start.load(Ordering::Acquire))
                    .max(1);
                if stats.short_writes.fetch_add(1, Ordering::AcqRel) == 0 {
                    stats.first_short_ns.store(since_start, Ordering::Release);
                    stats
                        .first_short_period
                        .store(seat.served, Ordering::Release);
                    stats
                        .first_short_frames
                        .store(written as u64, Ordering::Release);
                }
                if since_start < self.shared.warm_up_ns.load(Ordering::Acquire) {
                    stats.early_short_writes.fetch_add(1, Ordering::AcqRel);
                }
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
            Disturbance::Xrun => {
                let periods = u64::from(self.shared.config.periods) + PERIODS_TO_SKIP;
                guard.skip_until = clock::now() + periods * self.period_ns;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_published_before_the_first_cycle() {
        let published = Published::default();
        assert_eq!(published.read(), None);
        published.write(0, 5_000);
        assert_eq!(published.read(), Some((0, 5_000)));
        published.write(41, 9_000);
        assert_eq!(published.read(), Some((41, 9_000)));
    }

    #[test]
    fn the_place_of_a_card_fits_in_a_word() {
        assert_eq!(in_use(1, None) >> 32, 1);
        assert_eq!(in_use(1, None) as u32, 0);
        let word = in_use(7, Some((3, 0, 1)));
        assert_eq!(word >> 32, 7);
        assert_eq!(word as u32, PLACE_KNOWN | 3 << 20 | 1);
        // A number too large for its place: not known, rather than
        // another stream's.
        assert_eq!(in_use(2, Some((1024, 0, 0))) as u32, 0);
    }

    #[test]
    fn a_cycle_and_its_instant_are_read_together() {
        // The instant is seven times the number: a reader that got the
        // two from different cycles would see it.
        let published = Arc::new(Published::default());
        let writer = {
            let published = Arc::clone(&published);
            std::thread::spawn(move || {
                for index in 0..200_000_u64 {
                    published.write(index, index * 7);
                }
            })
        };
        let mut read = 0_u64;
        while !writer.is_finished() {
            if let Some((index, due)) = published.read() {
                assert_eq!(due, index * 7);
                read += 1;
            }
        }
        writer.join().expect("the writer ended badly");
        assert_eq!(published.read(), Some((199_999, 199_999 * 7)));
        assert!(read > 0);
    }
}
