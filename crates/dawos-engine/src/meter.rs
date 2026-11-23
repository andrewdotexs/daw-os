//! The record of every cycle (step P1.6a).
//!
//! Every live thread writes one record at the end of every cycle into a
//! ring of its own ([`dawos_rt::ring`]): no lock, no system call, nothing
//! that waits. A thread outside the live CPUs, the meter, empties the
//! rings some fifty times a second and does everything that costs:
//! histograms, and a judgement on every cycle that is not like the
//! others, with the cycle before it next to it.
//!
//! What a record answers, cycle by cycle:
//!
//! - when the card said the period was ready, and how far that was from
//!   the period before (the clock of the card, as the engine sees it);
//! - how long after that the thread was running (the wake-up delay);
//! - how long each part of the compute phase took: the input and what is
//!   made of it, the nodes, the output;
//! - how much the card had ready and how much it still had to play at
//!   the wake-up, read without a system call;
//! - what happened in the wait before it: an xrun, a card lost or back.
//!
//! From these the meter tells a period that went by because the thread
//! was late from one that went by because the card told late, and an
//! xrun that follows a late thread from one that does not.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use dawos_audio_io::Wake;
use dawos_rt::ring::{Reader, Record};
use dawos_rt::stats::Durations;

use crate::entries::{self, Entries, TraceFigures};
use crate::ktrace::{Symbols, Tracer};

/// Records a ring holds: eight seconds of cycles of 2 ms.
pub const RING_RECORDS: usize = 4096;

/// How often the meter empties the rings.
const READ_EVERY: Duration = Duration::from_millis(20);

/// A wake-up later than this is looked at one by one (acceptance of the
/// phase: under 50 us, with those above 20 us classified).
const LATE_WAKE_NS: u64 = 20_000;

/// The limit of the phase for a wake-up.
const WAKE_LIMIT_NS: u64 = 50_000;

/// Cycles to look at that are kept one by one, for each kind: after the
/// warm-up and in it. All of them are counted. Of the late wake-ups the
/// worst are kept, of every other kind the first.
const KEPT: usize = 6;
const KEPT_EARLY: usize = 2;

/// Minutes of a run for which the periods the card told late are counted
/// one minute at a time.
const MINUTES: usize = 180;

/// Cycles in course the meter can hold while it waits for every thread
/// to have written its record of them: the rings are emptied one after
/// the other, so the records of one cycle arrive some cycles apart.
const CYCLES_HELD: usize = 1024;

/// The compute budget cut the cycle short.
pub const INCOMPLETE: u16 = 0x100;
/// A cycle with sound whose patch could not be had.
pub const NO_PATCH: u16 = 0x200;
/// A fault was asked for in this cycle (a test).
pub const DISTURBED: u16 = 0x400;

/// One cycle of one live thread, as the thread wrote it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cycle {
    /// Number of the period.
    pub index: u64,
    /// When the period was due: the stamp of the card, or the timer.
    pub due: u64,
    /// When the thread was running again.
    pub woke: u64,
    /// From `woke`, in nanoseconds: the input taken and the patch done,
    /// the nodes done, the output given.
    pub captured_ns: u32,
    pub computed_ns: u32,
    pub finished_ns: u32,
    /// Periods that went by unserved before this one.
    pub missed: u32,
    /// Frames the card had ready, and frames it still had to play, at
    /// the wake-up.
    pub input_ready: u32,
    pub output_queued: u32,
    /// Nodes run.
    pub nodes: u16,
    /// The flags of [`Wake`] in the low byte, and those of this module.
    pub flags: u16,
    /// The slot the thread held: a spare thread learns it when it is
    /// called in.
    pub slot: u16,
    /// The longest step between two readings of the clock inside the
    /// nodes, and when it ended, from `woke`; both 0 when no node read
    /// the clock. Time lost with or without the kernel: what the trace
    /// of the kernel shows at that instant tells which.
    pub gap_ns: u32,
    pub gap_end_ns: u32,
}

impl Cycle {
    /// The record as the ring carries it.
    #[must_use]
    pub const fn to_record(self) -> Record {
        [
            self.index,
            self.due,
            self.woke,
            self.captured_ns as u64 | (self.computed_ns as u64) << 32,
            self.finished_ns as u64 | (self.missed as u64) << 32,
            self.input_ready as u64 | (self.output_queued as u64) << 32,
            self.nodes as u64 | (self.flags as u64) << 16 | (self.slot as u64) << 32,
            self.gap_ns as u64 | (self.gap_end_ns as u64) << 32,
        ]
    }

    /// The cycle a record of the ring carries.
    #[must_use]
    pub const fn from_record(record: &Record) -> Self {
        Self {
            index: record[0],
            due: record[1],
            woke: record[2],
            captured_ns: record[3] as u32,
            computed_ns: (record[3] >> 32) as u32,
            finished_ns: record[4] as u32,
            missed: (record[4] >> 32) as u32,
            input_ready: record[5] as u32,
            output_queued: (record[5] >> 32) as u32,
            nodes: record[6] as u16,
            flags: (record[6] >> 16) as u16,
            slot: (record[6] >> 32) as u16,
            gap_ns: record[7] as u32,
            gap_end_ns: (record[7] >> 32) as u32,
        }
    }

    const fn has(&self, flag: u16) -> bool {
        self.flags & flag != 0
    }

    const fn wake_up_ns(&self) -> u64 {
        self.woke.saturating_sub(self.due)
    }

    /// When the cycle ended.
    const fn finished(&self) -> u64 {
        self.woke + self.finished_ns as u64
    }
}

/// Why a cycle is looked at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An xrun was recovered in the wait before it.
    AfterXrun(XrunCause),
    /// The card was given up, or one was taken into use.
    CardLost,
    CardBack,
    /// Periods went by, and the cycle before had ended in time: the card
    /// told of this period late.
    CardLate,
    /// Periods went by because the cycle before ended after its period.
    ThreadLate,
    /// It ended after its own period.
    Overrun,
    /// It woke more than 20 us after the period was due.
    WokeLate,
}

/// What can be said of an xrun from the cycles around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XrunCause {
    /// A fault asked for on the command line.
    AskedFor,
    /// The cycle before ended after the card had run out of buffer.
    Thread,
    /// The cycle before ended in time: the fault is on the side of the
    /// card, its driver or the bus.
    NotTheThread,
    /// No cycle before it on this thread: a thread that has just taken
    /// the place of another, or the first cycle.
    Unknown,
}

impl Kind {
    const ALL: [Self; 10] = [
        Self::AfterXrun(XrunCause::AskedFor),
        Self::AfterXrun(XrunCause::Thread),
        Self::AfterXrun(XrunCause::NotTheThread),
        Self::AfterXrun(XrunCause::Unknown),
        Self::CardLost,
        Self::CardBack,
        Self::CardLate,
        Self::ThreadLate,
        Self::Overrun,
        Self::WokeLate,
    ];

    fn place(self) -> usize {
        Self::ALL.iter().position(|&kind| kind == self).unwrap_or(0)
    }

    /// In a few words, for the report.
    #[must_use]
    pub const fn words(self) -> &'static str {
        match self {
            Self::AfterXrun(XrunCause::AskedFor) => "after an xrun asked for",
            Self::AfterXrun(XrunCause::Thread) => "after an xrun, the thread was late",
            Self::AfterXrun(XrunCause::NotTheThread) => "after an xrun, the thread was in time",
            Self::AfterXrun(XrunCause::Unknown) => "after an xrun, no cycle before it",
            Self::CardLost => "the card was given up",
            Self::CardBack => "a card was taken into use",
            Self::CardLate => "the card told of the period late",
            Self::ThreadLate => "periods went by, the cycle before ran over",
            Self::Overrun => "ended after its period",
            Self::WokeLate => "woke more than 20 us late",
        }
    }
}

/// A cycle that is looked at, with the one before it.
#[derive(Clone, Copy, Debug)]
pub struct Notable {
    pub slot: usize,
    pub spare: bool,
    pub kind: Kind,
    pub cycle: Cycle,
    pub before: Option<Cycle>,
}

/// What the meter keeps for one thread, about the cycles after the
/// warm-up.
#[derive(Clone)]
pub struct ThreadFigures {
    pub slot: usize,
    pub spare: bool,
    /// Cycles after the warm-up, and in it.
    pub cycles: u64,
    pub early_cycles: u64,
    /// Cycles with no card behind them (asleep on the clock).
    pub on_the_clock: u64,
    /// From one stamp of the card to the next, less the nominal period,
    /// without the sign.
    pub period_off: Durations,
    /// The shortest and the longest stretch from one stamp to the next.
    pub period_least_ns: u64,
    pub period_most_ns: u64,
    pub wake_up: Durations,
    /// The parts of the compute phase, and the whole of it.
    pub input: Durations,
    pub nodes: Durations,
    pub output: Durations,
    pub compute: Durations,
    /// Wake-ups under 5 us, 5 to 10, 10 to 20, 20 to 50, 50 and over.
    pub wake_bands: [u64; 5],
    /// The least time left before the end of the period, in
    /// nanoseconds; negative: the worst overrun.
    pub least_margin_ns: i64,
    /// The least and the most the card had ready, and still to play.
    pub input_ready: (u32, u32),
    pub output_queued: (u32, u32),
    pub missed: u64,
    pub incomplete: u64,
    pub without_patch: u64,
    /// The worst wake-up of the warm-up.
    pub early_worst_wake_ns: u64,
    /// The fullest the ring was found, and records it had no room for.
    pub fullest: u64,
    pub dropped: u64,
}

impl ThreadFigures {
    fn new(slot: usize, spare: bool) -> Self {
        Self {
            slot,
            spare,
            cycles: 0,
            early_cycles: 0,
            on_the_clock: 0,
            period_off: Durations::new(),
            period_least_ns: u64::MAX,
            period_most_ns: 0,
            wake_up: Durations::new(),
            input: Durations::new(),
            nodes: Durations::new(),
            output: Durations::new(),
            compute: Durations::new(),
            wake_bands: [0; 5],
            least_margin_ns: i64::MAX,
            input_ready: (u32::MAX, 0),
            output_queued: (u32::MAX, 0),
            missed: 0,
            incomplete: 0,
            without_patch: 0,
            early_worst_wake_ns: 0,
            fullest: 0,
            dropped: 0,
        }
    }
}

/// Everything the meter found.
pub struct MeterReport {
    pub threads: Vec<ThreadFigures>,
    /// Cycles to look at, by kind (the order of `Kind::ALL`): after the
    /// warm-up and in it.
    pub counts: [u64; 10],
    pub early_counts: [u64; 10],
    /// The first of them, one by one.
    pub kept: Vec<Notable>,
    pub early_kept: Vec<Notable>,
    pub ring_records: u64,
    /// Whether the run had a sound card.
    pub card: bool,
    /// Periods the card told late, minute by minute from the first
    /// period, warm-up and all, up to the last minute that had cycles:
    /// how an interface settles shows here.
    pub card_late_by_minute: Vec<u32>,
    /// From the first thread awake to the last, cycle by cycle, for the
    /// cycles every live thread took part in; empty with one thread.
    pub wake_spread: Durations,
    /// Live threads of the run.
    pub live_threads: usize,
    /// What the trace of the kernel says of the live CPUs, when it was
    /// asked for.
    pub kernel: Option<KernelReport>,
}

/// The trace of the kernel on the live CPUs, read and placed.
pub struct KernelReport {
    pub entries: Entries,
    pub trace: TraceFigures,
    pub symbols: Symbols,
}

struct Source {
    reader: Reader,
    figures: ThreadFigures,
    before: Option<Cycle>,
}

/// The reading end of every ring, and what was made of the records so
/// far.
pub struct Meter {
    sources: Vec<Source>,
    period_ns: u64,
    /// Frames in the buffers of the card; 0 without a card.
    buffer_frames: u32,
    frames: u32,
    warm_up_end_ns: u64,
    counts: [u64; 10],
    early_counts: [u64; 10],
    kept: Vec<Notable>,
    early_kept: Vec<Notable>,
    start_ns: u64,
    card_late_by_minute: Vec<u32>,
    minutes: usize,
    threads: usize,
    /// For the cycles in course: number plus one (0: free), the first
    /// and the last wake-up, and how many threads wrote of it.
    held: Vec<(u64, u64, u64, usize)>,
    wake_spread: Durations,
    /// The trace of the kernel on the live CPUs, and what is made of
    /// it.
    kernel: Option<(Tracer, Entries)>,
}

/// What the meter must know about the run.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub period_ns: u64,
    /// Frames in a period.
    pub frames: u32,
    /// Periods in the buffers of the card; 0 without a card.
    pub periods: u32,
    /// When the first period is due, and how long the warm-up lasts.
    pub start_ns: u64,
    pub warm_up_ns: u64,
    /// Live threads.
    pub threads: usize,
}

impl Meter {
    /// A meter for the rings of these threads: (slot, spare, reader).
    /// The slot of a spare thread is not known until it writes.
    #[must_use]
    pub fn new(readers: Vec<(usize, bool, Reader)>, shape: Shape) -> Self {
        Self {
            sources: readers
                .into_iter()
                .map(|(slot, spare, reader)| Source {
                    reader,
                    figures: ThreadFigures::new(slot, spare),
                    before: None,
                })
                .collect(),
            period_ns: shape.period_ns,
            buffer_frames: shape.frames * shape.periods,
            frames: shape.frames,
            warm_up_end_ns: shape.start_ns + shape.warm_up_ns,
            counts: [0; 10],
            early_counts: [0; 10],
            kept: Vec::with_capacity(KEPT * Kind::ALL.len()),
            early_kept: Vec::with_capacity(KEPT_EARLY * Kind::ALL.len()),
            start_ns: shape.start_ns,
            card_late_by_minute: vec![0; MINUTES],
            minutes: 0,
            threads: shape.threads,
            held: vec![(0, 0, 0, 0); CYCLES_HELD],
            wake_spread: Durations::new(),
            kernel: None,
        }
    }

    /// Read the trace of the kernel too: `tracer` is tracing `cpus`,
    /// the CPUs of the live threads slot by slot.
    #[must_use]
    pub fn with_trace(mut self, tracer: Tracer, cpus: &[usize]) -> Self {
        self.kernel = Some((tracer, Entries::new(cpus, self.warm_up_end_ns)));
        self
    }

    /// Empty every ring, and then the trace of the kernel: in this
    /// order, so that no event older than the last cycle read is still
    /// to come.
    pub fn drain(&mut self) {
        for place in 0..self.sources.len() {
            let waiting = self.sources[place].reader.waiting();
            let figures = &mut self.sources[place].figures;
            figures.fullest = figures.fullest.max(waiting);
            while let Some(record) = self.sources[place].reader.pop() {
                let cycle = Cycle::from_record(&record);
                self.take(place, cycle);
            }
        }
        if let Some((tracer, entries)) = &mut self.kernel {
            tracer.read(|event| entries.event(event));
            entries.settle();
        }
    }

    /// One cycle of the thread at `place`.
    fn take(&mut self, place: usize, cycle: Cycle) {
        let period = self.period_ns;
        let early = cycle.woke < self.warm_up_end_ns;
        let before = self.sources[place].before;
        let kind = self.judge(&cycle, before.as_ref());
        if let Some((_, entries)) = &mut self.kernel {
            entries.cycle(usize::from(cycle.slot), &cycle);
        }
        let source = &mut self.sources[place];
        source.before = Some(cycle);
        let figures = &mut source.figures;
        figures.slot = usize::from(cycle.slot);

        if early {
            figures.early_cycles += 1;
            figures.early_worst_wake_ns = figures.early_worst_wake_ns.max(cycle.wake_up_ns());
        } else {
            figures.cycles += 1;
            figures.missed += u64::from(cycle.missed);
            figures.incomplete += u64::from(cycle.has(INCOMPLETE));
            figures.without_patch += u64::from(cycle.has(NO_PATCH));
            if cycle.has(u16::from(Wake::ON_THE_CLOCK | Wake::STALLED)) {
                // No card behind this cycle: its times say nothing of
                // the card.
                figures.on_the_clock += 1;
            } else {
                let wake_up = cycle.wake_up_ns();
                figures.wake_up.add(wake_up);
                let band = match wake_up {
                    0..5_000 => 0,
                    5_000..10_000 => 1,
                    10_000..LATE_WAKE_NS => 2,
                    LATE_WAKE_NS..WAKE_LIMIT_NS => 3,
                    _ => 4,
                };
                figures.wake_bands[band] += 1;
                figures.input.add(u64::from(cycle.captured_ns));
                figures.nodes.add(u64::from(
                    cycle.computed_ns.saturating_sub(cycle.captured_ns),
                ));
                figures.output.add(u64::from(
                    cycle.finished_ns.saturating_sub(cycle.computed_ns),
                ));
                figures.compute.add(u64::from(cycle.finished_ns));
                let margin = (cycle.due + period) as i64 - cycle.finished() as i64;
                figures.least_margin_ns = figures.least_margin_ns.min(margin);
                if self.buffer_frames != 0 {
                    figures.input_ready.0 = figures.input_ready.0.min(cycle.input_ready);
                    figures.input_ready.1 = figures.input_ready.1.max(cycle.input_ready);
                    figures.output_queued.0 = figures.output_queued.0.min(cycle.output_queued);
                    figures.output_queued.1 = figures.output_queued.1.max(cycle.output_queued);
                }
                // From stamp to stamp, when both are stamps of a card
                // that went on without a stop between them.
                let stopped = u16::from(
                    Wake::AFTER_XRUN | Wake::CARD_LOST | Wake::CARD_BACK | Wake::ON_THE_CLOCK,
                );
                if let Some(before) = before {
                    if !cycle.has(stopped)
                        && !before.has(u16::from(Wake::ON_THE_CLOCK | Wake::STALLED))
                    {
                        let gap = cycle.due.saturating_sub(before.due);
                        figures.period_off.add(gap.abs_diff(period));
                        figures.period_least_ns = figures.period_least_ns.min(gap);
                        figures.period_most_ns = figures.period_most_ns.max(gap);
                    }
                }
            }
        }

        // How far apart the threads woke for this cycle: once every one
        // of them has written of it. Cycles on the clock have no number
        // of their own, and are left out.
        if self.threads > 1 && !early && !cycle.has(u16::from(Wake::ON_THE_CLOCK | Wake::STALLED)) {
            let held = &mut self.held[(cycle.index % CYCLES_HELD as u64) as usize];
            if held.0 != cycle.index + 1 {
                *held = (cycle.index + 1, cycle.woke, cycle.woke, 0);
            }
            held.1 = held.1.min(cycle.woke);
            held.2 = held.2.max(cycle.woke);
            held.3 += 1;
            if held.3 == self.threads {
                self.wake_spread.add(held.2 - held.1);
            }
        }

        let minute = (cycle.woke.saturating_sub(self.start_ns) / 60_000_000_000) as usize;
        if minute < MINUTES {
            self.minutes = self.minutes.max(minute + 1);
            if kind == Some(Kind::CardLate) {
                self.card_late_by_minute[minute] += 1;
            }
        }

        if let Some(kind) = kind {
            let notable = Notable {
                slot: figures.slot,
                spare: figures.spare,
                kind,
                cycle,
                before,
            };
            if early {
                self.early_counts[kind.place()] += 1;
                keep(&mut self.early_kept, notable, KEPT_EARLY);
            } else {
                self.counts[kind.place()] += 1;
                keep(&mut self.kept, notable, KEPT);
            }
        }
    }

    /// Is this cycle one to look at, and why? One reason for a cycle:
    /// the first that applies, from what stops the sound to what only
    /// delays it.
    fn judge(&self, cycle: &Cycle, before: Option<&Cycle>) -> Option<Kind> {
        let period = self.period_ns;
        // The cycle before ended in time for the card to be served.
        let before_in_time = before.map(|before| before.finished() <= before.due + period);
        if cycle.has(u16::from(Wake::AFTER_XRUN)) {
            let cause = match before {
                None => XrunCause::Unknown,
                Some(before) if before.has(DISTURBED) => XrunCause::AskedFor,
                // What the card had left to play when the cycle before
                // began is the time that cycle had: past it, the output
                // ran dry. Without a card to tell, one period.
                Some(before) => {
                    let room_ns = if self.frames == 0 || self.buffer_frames == 0 {
                        period
                    } else {
                        u64::from(before.output_queued) * period / u64::from(self.frames)
                    };
                    if before.finished() > before.due + room_ns.max(period) {
                        XrunCause::Thread
                    } else {
                        XrunCause::NotTheThread
                    }
                }
            };
            return Some(Kind::AfterXrun(cause));
        }
        if cycle.has(u16::from(Wake::CARD_LOST)) {
            return Some(Kind::CardLost);
        }
        if cycle.has(u16::from(Wake::CARD_BACK)) {
            return Some(Kind::CardBack);
        }
        if cycle.has(u16::from(Wake::ON_THE_CLOCK | Wake::STALLED)) {
            return None;
        }
        if cycle.missed > 0 {
            return Some(match before_in_time {
                Some(false) => Kind::ThreadLate,
                _ => Kind::CardLate,
            });
        }
        if cycle.finished() > cycle.due + period {
            return Some(Kind::Overrun);
        }
        if cycle.wake_up_ns() > LATE_WAKE_NS {
            return Some(Kind::WokeLate);
        }
        None
    }

    /// What was found so far.
    #[must_use]
    pub fn report(mut self) -> MeterReport {
        let ring_records = self
            .sources
            .first()
            .map_or(0, |source| source.reader.capacity());
        for source in &mut self.sources {
            source.figures.dropped = source.reader.dropped();
        }
        // In the order they happened, whatever their kind.
        self.kept.sort_by_key(|notable| notable.cycle.woke);
        self.early_kept.sort_by_key(|notable| notable.cycle.woke);
        // By slot, the first thread of each before the spare ones.
        let mut threads: Vec<ThreadFigures> = self
            .sources
            .into_iter()
            .map(|source| source.figures)
            .filter(|figures| figures.cycles + figures.early_cycles > 0 || figures.dropped > 0)
            .collect();
        threads.sort_by_key(|figures| (figures.slot, figures.spare));
        // The trace ends here: the tracer takes its instance away.
        let kernel = self.kernel.map(|(tracer, entries)| KernelReport {
            entries,
            trace: TraceFigures {
                records: tracer.records,
                events: tracer.events,
                lost_pages: tracer.lost_pages,
                malformed_pages: tracer.malformed_pages,
                missing: tracer.missing.clone(),
            },
            symbols: Symbols::load(),
        });
        MeterReport {
            threads,
            counts: self.counts,
            early_counts: self.early_counts,
            kept: self.kept,
            early_kept: self.early_kept,
            ring_records,
            card: self.buffer_frames != 0,
            card_late_by_minute: self.card_late_by_minute[..self.minutes].to_vec(),
            wake_spread: self.wake_spread,
            live_threads: self.threads,
            kernel,
        }
    }
}

/// Keep `notable` if there is room for its kind (`room` for each kind);
/// of the late wake-ups, keep the worst.
fn keep(kept: &mut Vec<Notable>, notable: Notable, room: usize) {
    let same = kept
        .iter()
        .filter(|other| other.kind == notable.kind)
        .count();
    if same < room {
        kept.push(notable);
    } else if notable.kind == Kind::WokeLate {
        let mildest = kept
            .iter_mut()
            .filter(|other| other.kind == Kind::WokeLate)
            .min_by_key(|other| other.cycle.wake_up_ns());
        if let Some(mildest) = mildest {
            if notable.cycle.wake_up_ns() > mildest.cycle.wake_up_ns() {
                *mildest = notable;
            }
        }
    }
}

/// Seconds from the first period to `instant`.
fn seconds(start_ns: u64, instant: u64) -> f64 {
    instant.saturating_sub(start_ns) as f64 / 1e9
}

/// The body of the meter thread: empty the rings until `done` is raised,
/// then once more, and say what was found.
#[must_use]
pub fn collect(mut meter: Meter, done: &AtomicBool) -> MeterReport {
    loop {
        // Read the flag first: records written before it was raised are
        // all taken by the round that follows.
        let last = done.load(Ordering::Acquire);
        meter.drain();
        if last {
            return meter.report();
        }
        thread::sleep(READ_EVERY);
    }
}

fn us(ns: u64) -> f64 {
    ns as f64 / 1000.0
}

/// The heading of a table of durations.
pub fn heading() {
    println!(
        "  {:<24} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "", "least", "mean", "half", "99%", "99.9%", "99.99%", "most"
    );
}

/// One row of a table of durations, in microseconds.
pub fn row(name: &str, durations: &Durations) {
    let figure =
        |value: Option<u64>| value.map_or_else(|| "-".to_string(), |ns| format!("{:.2}", us(ns)));
    println!(
        "  {name:<24} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9}",
        figure(durations.min_ns()),
        figure(durations.mean_ns()),
        figure(durations.quantile_ns(50_000)),
        figure(durations.quantile_ns(99_000)),
        figure(durations.quantile_ns(99_900)),
        figure(durations.quantile_ns(99_990)),
        figure(durations.max_ns()),
    );
}

fn counts_in_words(counts: &[u64; 10]) -> String {
    let words: Vec<String> = Kind::ALL
        .iter()
        .zip(counts)
        .filter(|(_, &count)| count > 0)
        .map(|(kind, count)| format!("{count} {}", kind.words()))
        .collect();
    if words.is_empty() {
        "none".to_string()
    } else {
        words.join("; ")
    }
}

fn print_cycle(prefix: &str, cycle: &Cycle, period_ns: u64, card: bool) {
    let margin = (cycle.due + period_ns) as i64 - cycle.finished() as i64;
    let fill = if card {
        format!(
            "; card: {} frames ready, {} to play",
            cycle.input_ready, cycle.output_queued
        )
    } else {
        String::new()
    };
    println!(
        "{prefix}woke {:.2} us after the period was due; input {:.2}, nodes {:.2} ({} of them), output {:.2} us; ended {:.2} us {} the end of its period{fill}",
        us(cycle.wake_up_ns()),
        us(u64::from(cycle.captured_ns)),
        us(u64::from(cycle.computed_ns.saturating_sub(cycle.captured_ns))),
        cycle.nodes,
        us(u64::from(cycle.finished_ns.saturating_sub(cycle.computed_ns))),
        margin.unsigned_abs() as f64 / 1000.0,
        if margin >= 0 { "before" } else { "AFTER" },
    );
}

fn print_notable(notable: &Notable, start_ns: u64, period_ns: u64, card: bool) {
    let cycle = &notable.cycle;
    let stretch = notable.before.map_or_else(String::new, |before| {
        let gap = cycle.due.saturating_sub(before.due);
        format!(
            ", {:.2} us after the period before ({:.2} periods)",
            us(gap),
            gap as f64 / period_ns as f64
        )
    });
    println!(
        "  slot {}{} cycle {} at {:.3} s: {}{}; periods gone by {}",
        notable.slot,
        if notable.spare { " (spare)" } else { "" },
        cycle.index,
        seconds(start_ns, cycle.woke),
        notable.kind.words(),
        stretch,
        cycle.missed,
    );
    print_cycle("      it: ", cycle, period_ns, card);
    if let Some(before) = &notable.before {
        print_cycle("      the one before: ", before, period_ns, card);
    }
}

/// The part of the report of a run that comes from the record of every
/// cycle.
pub fn print(report: &MeterReport, start_ns: u64, period_ns: u64) {
    let read: u64 = report
        .threads
        .iter()
        .map(|figures| figures.cycles + figures.early_cycles)
        .sum();
    let dropped: u64 = report.threads.iter().map(|figures| figures.dropped).sum();
    let fullest = report
        .threads
        .iter()
        .map(|figures| figures.fullest)
        .max()
        .unwrap_or(0);
    println!();
    println!(
        "record_of_every_cycle: {read} cycles read, {dropped} dropped (rings of {} records, the fullest held {fullest})",
        report.ring_records
    );
    for figures in &report.threads {
        println!(
            "record, slot {}{}: {} cycles after the warm-up, {} in it ({} without a card); microseconds:",
            figures.slot,
            if figures.spare { " (spare)" } else { "" },
            figures.cycles,
            figures.early_cycles,
            figures.on_the_clock,
        );
        heading();
        row("period, off the nominal", &figures.period_off);
        row("wake-up", &figures.wake_up);
        row("input and patch", &figures.input);
        row("nodes", &figures.nodes);
        row("output", &figures.output);
        row("compute, whole", &figures.compute);
        if figures.period_off.count() > 0 {
            println!(
                "  period_us: least {:.2}, most {:.2} (nominal {:.2})",
                us(figures.period_least_ns),
                us(figures.period_most_ns),
                us(period_ns)
            );
        }
        let bands = figures.wake_bands;
        println!(
            "  wake_up_bands: under 5 us {}, 5 to 10 {}, 10 to 20 {}, 20 to 50 {}, 50 and over {}",
            bands[0], bands[1], bands[2], bands[3], bands[4]
        );
        if figures.least_margin_ns != i64::MAX {
            println!(
                "  least_margin_us: {:.2}",
                figures.least_margin_ns as f64 / 1000.0
            );
        }
        if figures.input_ready.0 != u32::MAX {
            println!(
                "  card_frames_at_the_wake_up: ready {} to {}, to play {} to {}",
                figures.input_ready.0,
                figures.input_ready.1,
                figures.output_queued.0,
                figures.output_queued.1
            );
        }
        println!(
            "  periods_gone_by: {}; cycles cut by the budget: {}; cycles without the patch: {}; worst wake-up of the warm-up: {:.2} us",
            figures.missed,
            figures.incomplete,
            figures.without_patch,
            us(figures.early_worst_wake_ns)
        );
    }
    if report.live_threads > 1 {
        println!(
            "record, the {} threads together: {} cycles after the warm-up that all of them took; microseconds:",
            report.live_threads,
            report.wake_spread.count()
        );
        heading();
        row("first awake to last", &report.wake_spread);
    }
    let total: u64 = report.counts.iter().sum();
    let early: u64 = report.early_counts.iter().sum();
    println!(
        "cycles_to_look_at: {total} after the warm-up ({}); {early} in it ({})",
        counts_in_words(&report.counts),
        counts_in_words(&report.early_counts)
    );
    if report.card {
        let by_minute: Vec<String> = report
            .card_late_by_minute
            .iter()
            .map(u32::to_string)
            .collect();
        println!(
            "card_told_late_by_minute: {} (from the first period, the warm-up too)",
            by_minute.join(" ")
        );
    }
    for notable in &report.kept {
        print_notable(notable, start_ns, period_ns, report.card);
    }
    if total > report.kept.len() as u64 {
        println!(
            "  ... and {} more, not shown (of each kind the first {KEPT}; of the late wake-ups the worst {KEPT})",
            total - report.kept.len() as u64
        );
    }
    if !report.early_kept.is_empty() {
        println!("in the warm-up, {} of them:", report.early_kept.len());
        for notable in &report.early_kept {
            print_notable(notable, start_ns, period_ns, report.card);
        }
    }
    if let Some(kernel) = &report.kernel {
        entries::print(&kernel.entries, &kernel.trace, &kernel.symbols, start_ns);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dawos_rt::ring::ring;

    const PERIOD: u64 = 2_000_000;
    const FRAMES: u32 = 96;

    fn shape(warm_up_ns: u64) -> Shape {
        Shape {
            period_ns: PERIOD,
            frames: FRAMES,
            periods: 3,
            start_ns: 1_000_000_000,
            warm_up_ns,
            threads: 1,
        }
    }

    /// A healthy cycle: woke 3 us late, 6 us of work.
    fn healthy(index: u64) -> Cycle {
        let due = 1_000_000_000 + (index + 1) * PERIOD;
        Cycle {
            index,
            due,
            woke: due + 3_000,
            captured_ns: 2_000,
            computed_ns: 5_000,
            finished_ns: 6_000,
            missed: 0,
            input_ready: FRAMES,
            output_queued: 2 * FRAMES,
            nodes: 2,
            flags: 0,
            slot: 0,
            gap_ns: 0,
            gap_end_ns: 0,
        }
    }

    fn run(cycles: &[Cycle], warm_up_ns: u64) -> MeterReport {
        let (mut writer, reader) = ring(64);
        let mut meter = Meter::new(vec![(0, false, reader)], shape(warm_up_ns));
        for cycle in cycles {
            assert!(writer.push(&cycle.to_record()));
            meter.drain();
        }
        meter.report()
    }

    #[test]
    fn a_cycle_goes_through_the_ring_whole() {
        let cycle = Cycle {
            index: u64::MAX - 1,
            due: 1 << 60,
            woke: (1 << 60) + 7,
            captured_ns: u32::MAX,
            computed_ns: 123_456,
            finished_ns: 7,
            missed: 3,
            input_ready: 384,
            output_queued: u32::MAX - 5,
            nodes: 513,
            flags: 0x07ff,
            slot: 3,
            gap_ns: 123_456,
            gap_end_ns: u32::MAX - 9,
        };
        assert_eq!(Cycle::from_record(&cycle.to_record()), cycle);
    }

    #[test]
    fn healthy_cycles_are_counted_and_nothing_is_looked_at() {
        let cycles: Vec<Cycle> = (0..1000).map(healthy).collect();
        let report = run(&cycles, 0);
        let figures = &report.threads[0];
        assert_eq!(figures.cycles, 1000);
        assert_eq!(figures.wake_up.max_ns(), Some(3_000));
        assert_eq!(figures.wake_bands, [1000, 0, 0, 0, 0]);
        assert_eq!(figures.compute.max_ns(), Some(6_000));
        assert_eq!(figures.nodes.max_ns(), Some(3_000));
        assert_eq!(figures.least_margin_ns, (PERIOD - 9_000) as i64);
        assert_eq!(figures.period_off.count(), 999);
        assert_eq!(figures.period_off.max_ns(), Some(0));
        assert_eq!(report.counts.iter().sum::<u64>(), 0);
        assert!(report.kept.is_empty());
    }

    #[test]
    fn a_late_card_is_told_from_a_late_thread() {
        // The card tells of period 11 a period late; the thread was
        // waiting.
        let mut late_card = healthy(11);
        late_card.missed = 1;
        late_card.input_ready = 2 * FRAMES;
        let mut cycles: Vec<Cycle> = (0..10).map(healthy).collect();
        cycles.push(late_card);
        // Then the thread takes two periods and a half over cycle 12,
        // and the card goes on: 14 and 15 go by.
        let mut slow = healthy(12);
        slow.finished_ns = 5_000_000;
        cycles.push(slow);
        let mut after = healthy(15);
        after.missed = 2;
        cycles.push(after);

        let report = run(&cycles, 0);
        assert_eq!(report.counts[Kind::CardLate.place()], 1);
        assert_eq!(report.counts[Kind::Overrun.place()], 1);
        assert_eq!(report.counts[Kind::ThreadLate.place()], 1);
        let kinds: Vec<Kind> = report.kept.iter().map(|notable| notable.kind).collect();
        assert_eq!(kinds, vec![Kind::CardLate, Kind::Overrun, Kind::ThreadLate]);
        assert_eq!(report.kept[0].before, Some(healthy(9)));
        assert_eq!(report.threads[0].missed, 3);
        // One period told late by the card, in the first minute.
        assert_eq!(report.card_late_by_minute, vec![1]);
        // The stretch of two periods is in the figures of the period.
        assert_eq!(report.threads[0].period_most_ns, 3 * PERIOD);
    }

    #[test]
    fn an_xrun_is_told_by_what_came_before() {
        let after_xrun = |index: u64| {
            let mut cycle = healthy(index);
            cycle.flags = u16::from(Wake::AFTER_XRUN);
            cycle
        };
        // In time, then an xrun: not the thread.
        let report = run(&[healthy(0), healthy(1), after_xrun(5)], 0);
        assert_eq!(
            report.kept[0].kind,
            Kind::AfterXrun(XrunCause::NotTheThread)
        );
        // Asked for.
        let mut asked = healthy(1);
        asked.flags = DISTURBED;
        let report = run(&[healthy(0), asked, after_xrun(9)], 0);
        assert_eq!(report.kept[0].kind, Kind::AfterXrun(XrunCause::AskedFor));
        // The cycle before ended after the two periods the card still
        // had to play.
        let mut slow = healthy(1);
        slow.finished_ns = 4_500_000;
        let report = run(&[healthy(0), slow, after_xrun(9)], 0);
        assert_eq!(report.kept[0].kind, Kind::Overrun);
        assert_eq!(report.kept[1].kind, Kind::AfterXrun(XrunCause::Thread));
        // Nothing before it.
        let report = run(&[after_xrun(3)], 0);
        assert_eq!(report.kept[0].kind, Kind::AfterXrun(XrunCause::Unknown));
    }

    #[test]
    fn a_late_wake_up_is_looked_at_and_put_in_its_band() {
        let mut late = healthy(5);
        late.woke = late.due + 31_000;
        let mut very_late = healthy(6);
        very_late.woke = very_late.due + 80_000;
        let mut cycles: Vec<Cycle> = (0..5).map(healthy).collect();
        cycles.push(late);
        cycles.push(very_late);
        let report = run(&cycles, 0);
        assert_eq!(report.threads[0].wake_bands, [5, 0, 0, 1, 1]);
        assert_eq!(report.counts[Kind::WokeLate.place()], 2);
    }

    #[test]
    fn of_many_late_wake_ups_the_worst_are_kept() {
        let cycles: Vec<Cycle> = (0..100)
            .map(|index| {
                let mut cycle = healthy(index);
                // Later and later, then less: the worst are in the middle.
                let extra = if index < 50 { index } else { 100 - index };
                cycle.woke = cycle.due + 30_000 + extra * 1_000;
                cycle
            })
            .collect();
        let report = run(&cycles, 0);
        assert_eq!(report.counts[Kind::WokeLate.place()], 100);
        assert_eq!(report.kept.len(), KEPT);
        assert!(report
            .kept
            .iter()
            .all(|notable| notable.cycle.wake_up_ns() >= 77_000));
        // In the order they happened.
        assert!(report
            .kept
            .windows(2)
            .all(|pair| pair[0].cycle.woke < pair[1].cycle.woke));
    }

    #[test]
    fn the_warm_up_is_counted_apart() {
        // Half a second of warm-up: 250 cycles of 2 ms.
        let mut cycles: Vec<Cycle> = (0..500).map(healthy).collect();
        cycles[10].woke = cycles[10].due + 90_000;
        cycles[400].woke = cycles[400].due + 25_000;
        let report = run(&cycles, 500_000_000);
        let figures = &report.threads[0];
        assert_eq!(figures.early_cycles, 249);
        assert_eq!(figures.cycles, 251);
        assert_eq!(figures.early_worst_wake_ns, 90_000);
        assert_eq!(figures.wake_up.max_ns(), Some(25_000));
        assert_eq!(report.early_counts[Kind::WokeLate.place()], 1);
        assert_eq!(report.counts[Kind::WokeLate.place()], 1);
        assert_eq!(report.early_kept.len(), 1);
    }

    #[test]
    fn cycles_without_a_card_are_kept_out_of_the_figures() {
        let mut on_the_clock = healthy(3);
        on_the_clock.flags = u16::from(Wake::ON_THE_CLOCK);
        on_the_clock.woke = on_the_clock.due + 70_000;
        let report = run(
            &[healthy(0), healthy(1), healthy(2), on_the_clock, healthy(4)],
            0,
        );
        let figures = &report.threads[0];
        assert_eq!(figures.cycles, 5);
        assert_eq!(figures.on_the_clock, 1);
        assert_eq!(figures.wake_up.count(), 4);
        assert_eq!(figures.wake_up.max_ns(), Some(3_000));
        // No stretch is measured from or to a cycle on the clock.
        assert_eq!(figures.period_off.count(), 2);
        assert_eq!(report.counts.iter().sum::<u64>(), 0);
    }

    #[test]
    fn the_spread_of_the_wake_ups_is_taken_cycle_by_cycle() {
        let (mut first, first_reader) = ring(64);
        let (mut second, second_reader) = ring(64);
        let mut meter = Meter::new(
            vec![(0, false, first_reader), (1, false, second_reader)],
            Shape {
                threads: 2,
                ..shape(0)
            },
        );
        for index in 0..10 {
            let leader = healthy(index);
            let mut follower = healthy(index);
            follower.slot = 1;
            // The follower wakes 2 us after the first; 7 us in cycle 4.
            follower.woke += if index == 4 { 7_000 } else { 2_000 };
            assert!(first.push(&leader.to_record()));
            // Cycle 9 is one the follower never took.
            if index != 9 {
                assert!(second.push(&follower.to_record()));
            }
        }
        // A cycle on the clock, with the number of the one before: not
        // counted.
        let mut on_the_clock = healthy(8);
        on_the_clock.slot = 1;
        on_the_clock.flags = u16::from(Wake::ON_THE_CLOCK);
        assert!(second.push(&on_the_clock.to_record()));
        meter.drain();
        let report = meter.report();
        assert_eq!(report.live_threads, 2);
        assert_eq!(report.wake_spread.count(), 9);
        assert_eq!(report.wake_spread.min_ns(), Some(2_000));
        assert_eq!(report.wake_spread.max_ns(), Some(7_000));
    }

    #[test]
    fn the_meter_thread_takes_what_was_written_before_the_end() {
        let (mut writer, reader) = ring(RING_RECORDS);
        let meter = Meter::new(vec![(0, false, reader)], shape(0));
        let done = std::sync::Arc::new(AtomicBool::new(false));
        let handle = {
            let done = std::sync::Arc::clone(&done);
            thread::spawn(move || collect(meter, &done))
        };
        for index in 0..300 {
            assert!(writer.push(&healthy(index).to_record()));
        }
        done.store(true, Ordering::Release);
        let report = handle.join().expect("the meter ended badly");
        assert_eq!(report.threads[0].cycles, 300);
        assert_eq!(report.threads[0].dropped, 0);
    }
}
