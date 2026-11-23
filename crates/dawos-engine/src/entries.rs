//! Who enters the kernel on the live CPUs, and in which phase of the
//! cycle (step P1.6c): the three measures of ADR 0021.
//!
//! Two streams meet here, both read by the meter thread outside the live
//! CPUs: the record every live thread writes of every cycle
//! ([`crate::meter`]), and the trace the kernel keeps of what happens
//! on the CPU of that thread ([`crate::ktrace`]). They share a clock.
//! The record cuts the time of a CPU into four phases:
//!
//! - **compute**: from the thread running again to the end of the
//!   nodes. Nothing may enter the kernel here;
//! - **delivery**: from the end of the nodes to the output given to the
//!   card. The thread that holds the card tells the driver how far it
//!   wrote: one system call;
//! - **wait**: from there to the instant the next period is due. The
//!   thread sleeps in the kernel, and the kernel does its own work on
//!   the CPU;
//! - **wake-up**: from the instant the period is due to the thread
//!   running again in user space.
//!
//! Every event of the trace falls in one of them, and is counted there
//! under its name. Three things more are made of the two streams:
//!
//! - system calls for each cycle and phase (the first measure);
//! - for every step of more than a microsecond between two readings of
//!   the clock inside the nodes, whether the kernel was entered in it
//!   or not (the third measure: time lost with no entry into the
//!   kernel comes from below the operating system);
//! - the path of every wake-up: when the first interrupt came, when the
//!   thread was given the CPU, whether it lost it again before it was
//!   back in user space.

use std::collections::{HashMap, VecDeque};

use dawos_rt::stats::Durations;

use crate::ktrace::{self, Event, Symbols, What};
use crate::meter::{self, Cycle};

/// A step between two readings of the clock, inside the nodes, that is
/// looked at: a healthy step is some tens of nanoseconds.
const GAP_NS: u64 = 1_000;

/// How far the clock of the trace and the clock of the thread may be
/// apart when an event is looked for inside a step.
const CLOCK_SLACK_NS: u64 = 100;

/// A wake-up later than this has its events kept one by one.
const LATE_WAKE_NS: u64 = 20_000;

/// Events of one cycle held until the cycle is closed: of the compute
/// phase the first, of the wait and the wake-up the last.
const HELD_COMPUTE: usize = 16;
const HELD_WAIT: usize = 48;

/// What is kept one by one for the report.
const KEPT_IN_COMPUTE: usize = 12;
const KEPT_GAPS: usize = 4;
const KEPT_LATE: usize = 4;
const KEPT_AGAIN: usize = 3;

/// The phases of the cycle, as this module cuts them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Phase {
    Compute,
    Delivery,
    Wait,
    WakeUp,
}

impl Phase {
    const ALL: [Self; 4] = [Self::Compute, Self::Delivery, Self::Wait, Self::WakeUp];

    const fn words(self) -> &'static str {
        match self {
            Self::Compute => "compute",
            Self::Delivery => "delivery",
            Self::Wait => "wait",
            Self::WakeUp => "wake-up",
        }
    }

    /// Where the system calls of the phase are counted cycle by cycle:
    /// those of the wake-up with those of the wait.
    const fn column(self) -> usize {
        match self {
            Self::Compute => 0,
            Self::Delivery => 1,
            Self::Wait | Self::WakeUp => 2,
        }
    }
}

/// The name an event is counted under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Label {
    /// A system call, by its number.
    Syscall(u64),
    /// An interrupt of the processor itself.
    Vector(&'static str),
    /// An interrupt of a device, by its number.
    DeviceIrq(u64),
    PageFault,
    /// An interrupt that cannot be masked, by its handler.
    Nmi(u64),
    /// Deferred work of an interrupt, by its kind.
    Softirq(u64),
    /// A timer of the kernel, by the function it calls.
    Timer(u64),
    /// A piece of deferred work, by its function.
    Work(u64),
    /// Another thread was given the CPU, by its name.
    Ran([u8; 16]),
}

/// The kinds of names, for the report.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Syscalls,
    /// The CPU was taken from what it was doing.
    NotAskedFor,
    /// What the kernel did once it had the CPU.
    Inside,
}

impl Label {
    const fn group(&self) -> Group {
        match self {
            Self::Syscall(_) => Group::Syscalls,
            Self::Vector(_) | Self::DeviceIrq(_) | Self::PageFault | Self::Nmi(_) => {
                Group::NotAskedFor
            }
            Self::Softirq(_) | Self::Timer(_) | Self::Work(_) | Self::Ran(_) => Group::Inside,
        }
    }

    fn words(&self, symbols: &Symbols) -> String {
        match self {
            Self::Syscall(number) => syscall(*number),
            Self::Vector(name) => (*name).to_string(),
            Self::DeviceIrq(number) => format!("device interrupt {number}"),
            Self::PageFault => "page fault".to_string(),
            Self::Nmi(handler) => format!("nmi: {}", symbols.name(*handler)),
            Self::Softirq(kind) => format!("softirq: {}", ktrace::softirq_name(*kind)),
            Self::Timer(function) => format!("timer: {}", symbols.name(*function)),
            Self::Work(function) => format!("work: {}", symbols.name(*function)),
            Self::Ran(comm) => format!("thread {}", name(comm)),
        }
    }
}

fn syscall(number: u64) -> String {
    ktrace::syscall_name(number).map_or_else(|| format!("syscall {number}"), str::to_string)
}

/// A name of the kernel as text: bytes up to the first zero.
fn name(comm: &[u8; 16]) -> String {
    let end = comm
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(comm.len());
    String::from_utf8_lossy(&comm[..end]).into_owned()
}

/// The name an event is counted under; none for those that only say
/// something ended.
fn label(event: &Event) -> Option<Label> {
    Some(match event.what {
        What::SyscallEnter => Label::Syscall(event.a),
        What::Vector(name) => Label::Vector(name),
        What::DeviceIrq => Label::DeviceIrq(event.a),
        What::PageFault => Label::PageFault,
        What::Nmi => Label::Nmi(event.a),
        What::Softirq => Label::Softirq(event.a),
        What::Timer => Label::Timer(event.a),
        What::Work => Label::Work(event.a),
        What::SyscallExit | What::Switch => return None,
    })
}

/// Who had the CPU at the instant a period was due.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Holder {
    /// Nobody: the CPU was at rest.
    Idle,
    /// The thread of the engine itself, which had not gone to sleep.
    Itself,
    Other([u8; 16]),
    /// Not seen yet in the trace.
    Unknown,
}

/// One cycle, as far as this module needs it: instants, not lengths.
#[derive(Clone, Copy, Debug)]
struct Span {
    index: u64,
    due: u64,
    woke: u64,
    nodes_end: u64,
    finished: u64,
    gap_ns: u64,
    gap_end: u64,
    /// The cycle began as any other: with a card behind it and nothing
    /// to recover from.
    ordinary: bool,
}

/// What is gathered about the cycle in course on one CPU, and the wait
/// after it.
#[derive(Default)]
struct Tally {
    syscalls: [u32; 3],
    in_compute: Vec<(u64, Label)>,
    wait: VecDeque<Event>,
    /// Times the thread of the engine was given the CPU after the cycle
    /// ended; the first and the last.
    switch_ins: u32,
    first_in: u64,
    last_in: u64,
    /// Times it lost the CPU again before it was back in user space,
    /// blocked in the kernel (on a lock, for one).
    blocked_again: u32,
    /// The first interrupt at or after the instant the next period was
    /// due; 0: none.
    first_irq: u64,
    at_due: Option<Holder>,
}

impl Tally {
    fn clear(&mut self) {
        self.syscalls = [0; 3];
        self.in_compute.clear();
        self.wait.clear();
        self.switch_ins = 0;
        self.first_in = 0;
        self.last_in = 0;
        self.blocked_again = 0;
        self.first_irq = 0;
        self.at_due = None;
    }
}

/// The wake-ups of one thread that took about as long.
#[derive(Default)]
pub struct Band {
    pub cycles: u64,
    pub at_due: HashMap<Holder, u64>,
    /// Events between the instant the period was due and the thread in
    /// user space.
    pub events: HashMap<Label, u64>,
}

/// A wake-up whose events are kept one by one.
pub struct Story {
    pub slot: usize,
    pub index: u64,
    pub why: &'static str,
    pub due: u64,
    pub woke: u64,
    /// The thread of the engine on that CPU.
    pub ours: i32,
    pub events: Vec<Event>,
}

/// An event in the compute phase, kept one by one.
pub struct InCompute {
    pub slot: usize,
    pub index: u64,
    pub woke: u64,
    pub time_ns: u64,
    pub label: Label,
}

/// A step inside the nodes with no entry into the kernel in it.
pub struct Gap {
    pub slot: usize,
    pub index: u64,
    pub at_ns: u64,
    pub gap_ns: u64,
}

/// What is known about one live CPU.
pub struct CpuFigures {
    pub cpu: usize,
    /// Cycles after the warm-up that were closed: the trace was read up
    /// to the cycle after them.
    pub cycles: u64,
    /// For compute, delivery and wait: cycles with no system call, with
    /// one, two, three, four or more.
    pub syscalls_per_cycle: [[u64; 5]; 3],
    /// Ordinary wake-ups, and their path.
    pub wake_ups: u64,
    pub to_first_irq: Durations,
    pub to_cpu: Durations,
    pub off_and_on: Durations,
    pub to_user: Durations,
    /// Wake-ups in which the thread was given the CPU no time (it had
    /// never left it), once, twice, three times or more.
    pub given_the_cpu: [u64; 4],
    /// Times the thread lost the CPU again because it was blocked in
    /// the kernel.
    pub blocked_again: u64,
    /// By how long the wake-up took: under 5 us, 5 to 10, 10 to 20, 20
    /// to 50, 50 and over.
    pub bands: [Band; 5],
    cycles_held: VecDeque<Span>,
    events_held: VecDeque<Event>,
    tally: Tally,
    /// The thread of the engine last seen on this CPU; 0: none yet.
    ours: i32,
    current: Holder,
    story_told: bool,
}

/// Everything found about the kernel on the live CPUs.
pub struct Entries {
    pub cpus: Vec<CpuFigures>,
    warm_up_end_ns: u64,
    /// By slot, phase and name: how many.
    pub counts: HashMap<(usize, Phase, Label), u64>,
    /// Events before the first cycle of their CPU, or on a CPU with no
    /// thread of the engine.
    pub outside: u64,
    /// Cycles that came out of order (a spare thread took a slot) and
    /// were left out.
    pub out_of_order: u64,
    pub in_compute: Vec<InCompute>,
    pub in_compute_total: u64,
    /// The longest step inside the nodes, cycle by cycle, for the
    /// cycles whose nodes read the clock.
    pub gaps: Durations,
    /// Steps of a microsecond and more with an entry into the kernel in
    /// them, by the name of the first: how many, and the longest.
    pub gaps_explained: HashMap<Label, (u64, u64)>,
    /// Those with none.
    pub gaps_unexplained: Durations,
    pub worst_unexplained: Vec<Gap>,
    pub stories: Vec<Story>,
}

impl Entries {
    /// For the live threads on `cpus`, slot by slot.
    #[must_use]
    pub fn new(cpus: &[usize], warm_up_end_ns: u64) -> Self {
        Self {
            cpus: cpus
                .iter()
                .map(|&cpu| CpuFigures {
                    cpu,
                    cycles: 0,
                    syscalls_per_cycle: [[0; 5]; 3],
                    wake_ups: 0,
                    to_first_irq: Durations::new(),
                    to_cpu: Durations::new(),
                    off_and_on: Durations::new(),
                    to_user: Durations::new(),
                    given_the_cpu: [0; 4],
                    blocked_again: 0,
                    bands: Default::default(),
                    cycles_held: VecDeque::with_capacity(64),
                    events_held: VecDeque::with_capacity(1024),
                    tally: Tally {
                        in_compute: Vec::with_capacity(HELD_COMPUTE),
                        wait: VecDeque::with_capacity(HELD_WAIT),
                        ..Tally::default()
                    },
                    ours: 0,
                    current: Holder::Unknown,
                    story_told: false,
                })
                .collect(),
            warm_up_end_ns,
            counts: HashMap::new(),
            outside: 0,
            out_of_order: 0,
            in_compute: Vec::new(),
            in_compute_total: 0,
            gaps: Durations::new(),
            gaps_explained: HashMap::new(),
            gaps_unexplained: Durations::new(),
            worst_unexplained: Vec::new(),
            stories: Vec::new(),
        }
    }

    /// A cycle of the thread that holds `slot`, as it comes out of the
    /// ring.
    pub fn cycle(&mut self, slot: usize, cycle: &Cycle) {
        let Some(cpu) = self.cpus.get_mut(slot) else {
            return;
        };
        if cpu
            .cycles_held
            .back()
            .is_some_and(|last| cycle.woke <= last.woke)
        {
            self.out_of_order += 1;
            return;
        }
        cpu.cycles_held.push_back(Span {
            index: cycle.index,
            due: cycle.due,
            woke: cycle.woke,
            nodes_end: cycle.woke + u64::from(cycle.computed_ns),
            finished: cycle.woke + u64::from(cycle.finished_ns),
            gap_ns: u64::from(cycle.gap_ns),
            gap_end: cycle.woke + u64::from(cycle.gap_end_ns),
            ordinary: cycle.flags & 0xff == 0,
        });
    }

    /// An event of the trace.
    pub fn event(&mut self, event: Event) {
        match self.cpus.iter_mut().find(|cpu| cpu.cpu == event.cpu) {
            Some(cpu) => cpu.events_held.push_back(event),
            None => self.outside += 1,
        }
    }

    /// Place every event that can be placed: those before the last
    /// cycle known of their CPU. To be called when the rings were
    /// emptied first and the trace read after them, so that no event
    /// older than the last cycle is still to come.
    pub fn settle(&mut self) {
        for slot in 0..self.cpus.len() {
            while self.cpus[slot].cycles_held.len() >= 2 {
                let before = self.cpus[slot].cycles_held[0];
                let after = self.cpus[slot].cycles_held[1];
                while let Some(event) = self.cpus[slot].events_held.front().copied() {
                    if event.time_ns >= after.woke {
                        break;
                    }
                    self.cpus[slot].events_held.pop_front();
                    self.place(slot, &event, &before, &after);
                }
                self.close(slot, &before, &after);
                self.cpus[slot].cycles_held.pop_front();
            }
        }
    }

    /// One event, between the wake-up of `before` and that of `after`.
    fn place(&mut self, slot: usize, event: &Event, before: &Span, after: &Span) {
        let cpu = &mut self.cpus[slot];
        let time = event.time_ns;
        let counted = before.woke >= self.warm_up_end_ns;
        let phase = if time < before.woke {
            // Only before the first cycle. Who has the CPU is still to
            // be followed.
            None
        } else if time < before.nodes_end {
            Some(Phase::Compute)
        } else if time < before.finished {
            Some(Phase::Delivery)
        } else if time < after.due {
            Some(Phase::Wait)
        } else {
            Some(Phase::WakeUp)
        };
        if cpu.tally.at_due.is_none() && time >= after.due {
            cpu.tally.at_due = Some(cpu.current);
        }
        let waiting = matches!(phase, Some(Phase::Wait | Phase::WakeUp));
        let mut named = label(event);
        if event.what == What::Switch {
            let (left, taken) = (event.a as i32, event.b as i32);
            if event.comm.starts_with(b"live-") || event.comm.starts_with(b"spare-") {
                cpu.ours = taken;
            }
            cpu.current = if taken == 0 {
                Holder::Idle
            } else if taken == cpu.ours {
                Holder::Itself
            } else {
                Holder::Other(event.comm)
            };
            if taken == cpu.ours && taken != 0 {
                if waiting {
                    if cpu.tally.switch_ins == 0 {
                        cpu.tally.first_in = time;
                    }
                    cpu.tally.switch_ins += 1;
                    cpu.tally.last_in = time;
                }
            } else if taken != 0 {
                named = Some(Label::Ran(event.comm));
            }
            // Given the CPU and blocked again before user space: state
            // 2 is a sleep no signal ends, which a lock of the kernel
            // is.
            if left == cpu.ours && left != 0 && waiting && cpu.tally.switch_ins > 0 && event.c == 2
            {
                cpu.tally.blocked_again += 1;
            }
        }
        let Some(phase) = phase else {
            self.outside += 1;
            return;
        };
        if matches!(event.what, What::Vector(_) | What::DeviceIrq)
            && phase == Phase::WakeUp
            && cpu.tally.first_irq == 0
        {
            cpu.tally.first_irq = time;
        }
        if waiting {
            if cpu.tally.wait.len() == HELD_WAIT {
                cpu.tally.wait.pop_front();
            }
            cpu.tally.wait.push_back(*event);
        }
        let Some(label) = named else {
            return;
        };
        if matches!(label, Label::Syscall(_)) {
            cpu.tally.syscalls[phase.column()] += 1;
        }
        if phase == Phase::Compute && cpu.tally.in_compute.len() < HELD_COMPUTE {
            cpu.tally.in_compute.push((time, label));
        }
        if counted {
            *self.counts.entry((slot, phase, label)).or_insert(0) += 1;
            if phase == Phase::Compute {
                self.in_compute_total += 1;
                if self.in_compute.len() < KEPT_IN_COMPUTE {
                    self.in_compute.push(InCompute {
                        slot,
                        index: before.index,
                        woke: before.woke,
                        time_ns: time,
                        label,
                    });
                }
            }
        }
    }

    /// Every event up to the wake-up of `after` was placed: sum up the
    /// cycle `before` and the wake-up of `after`.
    fn close(&mut self, slot: usize, before: &Span, after: &Span) {
        let cpu = &mut self.cpus[slot];
        if before.woke < self.warm_up_end_ns {
            cpu.tally.clear();
            return;
        }
        cpu.cycles += 1;
        for (column, &calls) in cpu.tally.syscalls.iter().enumerate() {
            cpu.syscalls_per_cycle[column][(calls as usize).min(4)] += 1;
        }

        // The longest step inside the nodes: was the kernel entered in
        // it?
        if before.gap_ns > 0 {
            self.gaps.add(before.gap_ns);
            if before.gap_ns >= GAP_NS {
                let from = before
                    .gap_end
                    .saturating_sub(before.gap_ns + CLOCK_SLACK_NS);
                let to = before.gap_end + CLOCK_SLACK_NS;
                let cause = cpu
                    .tally
                    .in_compute
                    .iter()
                    .find(|(time, _)| (from..=to).contains(time));
                if let Some((_, label)) = cause {
                    let explained = self.gaps_explained.entry(*label).or_insert((0, 0));
                    explained.0 += 1;
                    explained.1 = explained.1.max(before.gap_ns);
                } else {
                    self.gaps_unexplained.add(before.gap_ns);
                    let gap = Gap {
                        slot,
                        index: before.index,
                        at_ns: before.gap_end,
                        gap_ns: before.gap_ns,
                    };
                    if self.worst_unexplained.len() < KEPT_GAPS {
                        self.worst_unexplained.push(gap);
                    } else if let Some(mildest) = self
                        .worst_unexplained
                        .iter_mut()
                        .min_by_key(|kept| kept.gap_ns)
                        .filter(|kept| kept.gap_ns < gap.gap_ns)
                    {
                        *mildest = gap;
                    }
                }
            }
        }

        // The wake-up of the cycle after.
        if after.ordinary {
            let tally = &cpu.tally;
            let wake_up = after.woke.saturating_sub(after.due);
            cpu.wake_ups += 1;
            cpu.given_the_cpu[(tally.switch_ins as usize).min(3)] += 1;
            cpu.blocked_again += u64::from(tally.blocked_again);
            if tally.switch_ins > 0 {
                cpu.to_cpu.add(tally.first_in.saturating_sub(after.due));
                cpu.to_user.add(after.woke.saturating_sub(tally.last_in));
            }
            if tally.switch_ins > 1 {
                cpu.off_and_on.add(tally.last_in - tally.first_in);
            }
            if tally.first_irq != 0 {
                cpu.to_first_irq
                    .add(tally.first_irq.saturating_sub(after.due));
            }
            let band = &mut cpu.bands[match wake_up {
                0..5_000 => 0,
                5_000..10_000 => 1,
                10_000..20_000 => 2,
                20_000..50_000 => 3,
                _ => 4,
            }];
            band.cycles += 1;
            *band
                .at_due
                .entry(tally.at_due.unwrap_or(cpu.current))
                .or_insert(0) += 1;
            for event in tally.wait.iter().filter(|event| event.time_ns >= after.due) {
                let named = match (label(event), event.what) {
                    (Some(named), _) => named,
                    (None, What::Switch) if event.b as i32 != cpu.ours && event.b != 0 => {
                        Label::Ran(event.comm)
                    }
                    _ => continue,
                };
                *band.events.entry(named).or_insert(0) += 1;
            }

            // Told one by one: the first of every thread that slept,
            // those where the thread lost the CPU again, the latest.
            let why = if !cpu.story_told && tally.switch_ins > 0 {
                cpu.story_told = true;
                Some("the first after the warm-up")
            } else if wake_up > LATE_WAKE_NS {
                Some("woke more than 20 us late")
            } else if tally.switch_ins > 1 {
                Some("given the CPU more than once")
            } else {
                None
            };
            if let Some(why) = why {
                let story = Story {
                    slot,
                    index: after.index,
                    why,
                    due: after.due,
                    woke: after.woke,
                    ours: cpu.ours,
                    events: tally.wait.iter().copied().collect(),
                };
                let same = |kept: &&mut Story| kept.why == why;
                let room = match wake_up {
                    _ if why.starts_with("the first") => usize::MAX,
                    LATE_WAKE_NS.. => KEPT_LATE,
                    _ => KEPT_AGAIN,
                };
                if self.stories.iter_mut().filter(same).count() < room {
                    self.stories.push(story);
                } else if wake_up > LATE_WAKE_NS {
                    // Of the late ones, the latest.
                    if let Some(mildest) = self
                        .stories
                        .iter_mut()
                        .filter(same)
                        .min_by_key(|kept| kept.woke - kept.due)
                        .filter(|kept| kept.woke - kept.due < wake_up)
                    {
                        *mildest = story;
                    }
                }
            }
        }
        cpu.tally.clear();
    }

    /// System calls and entries not asked for in the compute phase,
    /// after the warm-up.
    #[must_use]
    pub fn in_compute(&self) -> (u64, u64) {
        let mut found = (0, 0);
        for ((_, phase, label), count) in &self.counts {
            if *phase == Phase::Compute {
                match label.group() {
                    Group::Syscalls => found.0 += count,
                    Group::NotAskedFor => found.1 += count,
                    Group::Inside => {}
                }
            }
        }
        found
    }
}

/// What the trace itself has to say.
pub struct TraceFigures {
    pub records: u64,
    pub events: u64,
    pub lost_pages: u64,
    pub malformed_pages: u64,
    pub missing: Vec<String>,
}

/// The names of one group in one phase, the most frequent first, each
/// with its count slot by slot.
fn names(entries: &Entries, symbols: &Symbols, phase: Phase, group: Group) -> String {
    let slots = entries.cpus.len();
    let mut by_label: HashMap<Label, Vec<u64>> = HashMap::new();
    for ((slot, of_phase, label), count) in &entries.counts {
        if *of_phase == phase && label.group() == group {
            by_label.entry(*label).or_insert_with(|| vec![0; slots])[*slot] += count;
        }
    }
    let mut rows: Vec<(Label, Vec<u64>)> = by_label.into_iter().collect();
    rows.sort_by_key(|(label, counts)| (u64::MAX - counts.iter().sum::<u64>(), *label));
    if rows.is_empty() {
        return "none".to_string();
    }
    let words: Vec<String> = rows
        .iter()
        .map(|(label, counts)| {
            let each: Vec<String> = counts.iter().map(u64::to_string).collect();
            format!(
                "{} {} [{}]",
                label.words(symbols),
                counts.iter().sum::<u64>(),
                each.join(" ")
            )
        })
        .collect();
    words.join("; ")
}

fn us(ns: u64) -> f64 {
    ns as f64 / 1000.0
}

/// One event of a story, in words.
fn told(event: &Event, ours: i32, symbols: &Symbols) -> String {
    match event.what {
        What::SyscallEnter => format!("enters the kernel: {}", syscall(event.a)),
        What::SyscallExit => format!(
            "back from {}, which returned {}",
            syscall(event.a),
            event.b as i64
        ),
        What::Switch => {
            let state = match event.c {
                0 => "able to run".to_string(),
                1 => "asleep".to_string(),
                2 => "blocked in the kernel".to_string(),
                0x100 => "able to run, pushed aside".to_string(),
                other => format!("state {other:#x}"),
            };
            let taken = if event.b as i32 == ours && ours != 0 {
                "the thread of the engine".to_string()
            } else if event.b == 0 {
                "nobody (the CPU rests)".to_string()
            } else {
                format!("thread {}", name(&event.comm))
            };
            format!("the CPU goes to {taken}; the one before is left {state}")
        }
        _ => label(event).map_or_else(String::new, |label| label.words(symbols)),
    }
}

/// The part of the report of a run that comes from the trace of the
/// kernel.
pub fn print(entries: &Entries, trace: &TraceFigures, symbols: &Symbols, start_ns: u64) {
    let cpus: Vec<String> = entries.cpus.iter().map(|cpu| cpu.cpu.to_string()).collect();
    println!();
    println!(
        "kernel_entries: CPUs {} traced; {} records read, {} of them of the events asked for; pages with records lost before them: {}; pages not understood: {}; events outside the cycles: {}; cycles out of order: {}",
        cpus.join(" "),
        trace.records,
        trace.events,
        trace.lost_pages,
        trace.malformed_pages,
        entries.outside,
        entries.out_of_order,
    );
    println!(
        "kernel_entries_events_this_kernel_has_not: {}",
        if trace.missing.is_empty() {
            "none".to_string()
        } else {
            trace.missing.join(" ")
        }
    );
    let (calls, others) = entries.in_compute();
    let cycles: u64 = entries.cpus.iter().map(|cpu| cpu.cycles).sum();
    println!(
        "kernel_entries_in_compute: {calls} system calls, {others} entries not asked for, in {cycles} cycles after the warm-up"
    );

    println!("kernel_entries, system calls for each cycle; cycles with:");
    println!(
        "  {:<24} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "", "none", "one", "two", "three", "more"
    );
    for (slot, cpu) in entries.cpus.iter().enumerate() {
        for (column, phase) in ["compute", "delivery", "wait"].iter().enumerate() {
            let row = cpu.syscalls_per_cycle[column];
            println!(
                "  {:<24} {:>9} {:>9} {:>9} {:>9} {:>9}",
                format!("slot {slot}, {phase}"),
                row[0],
                row[1],
                row[2],
                row[3],
                row[4]
            );
        }
    }
    let groups = [
        (Group::Syscalls, "system calls"),
        (Group::NotAskedFor, "entries not asked for"),
        (Group::Inside, "what the kernel did there"),
    ];
    for (group, words) in groups {
        println!("kernel_entries, {words}, by phase (the total, and slot by slot in brackets):");
        for phase in Phase::ALL {
            println!(
                "  {:<9} {}",
                format!("{}:", phase.words()),
                names(entries, symbols, phase, group)
            );
        }
    }
    if entries.in_compute_total > 0 {
        println!(
            "kernel_entries, in the compute phase one by one (the first {} of {}):",
            entries.in_compute.len(),
            entries.in_compute_total
        );
        for seen in &entries.in_compute {
            println!(
                "  slot {} cycle {} at {:.3} s: {:.2} us after the wake-up: {}",
                seen.slot,
                seen.index,
                seen.woke.saturating_sub(start_ns) as f64 / 1e9,
                us(seen.time_ns.saturating_sub(seen.woke)),
                seen.label.words(symbols)
            );
        }
    }

    // The third measure.
    if entries.gaps.count() == 0 {
        println!("kernel_entries, time lost in the nodes: not measured (no node read the clock: run with --load)");
    } else {
        println!(
            "kernel_entries, the longest step between two readings of the clock in the nodes of each cycle ({} cycles); microseconds:",
            entries.gaps.count()
        );
        meter::heading();
        meter::row("longest step", &entries.gaps);
        let explained: u64 = entries
            .gaps_explained
            .values()
            .map(|(count, _)| count)
            .sum();
        println!(
            "  steps_of_a_microsecond_and_more: {} with an entry into the kernel in them, {} with none",
            explained,
            entries.gaps_unexplained.count()
        );
        let mut causes: Vec<(&Label, &(u64, u64))> = entries.gaps_explained.iter().collect();
        causes.sort_by_key(|(label, (count, _))| (u64::MAX - count, **label));
        for (label, (count, longest)) in causes {
            println!(
                "    {}: {count}, the longest {:.2} us",
                label.words(symbols),
                us(*longest)
            );
        }
        if entries.gaps_unexplained.count() > 0 {
            meter::row("with no entry", &entries.gaps_unexplained);
            let mut worst: Vec<&Gap> = entries.worst_unexplained.iter().collect();
            worst.sort_by_key(|gap| u64::MAX - gap.gap_ns);
            for gap in worst {
                println!(
                    "    slot {} cycle {} at {:.3} s: {:.2} us",
                    gap.slot,
                    gap.index,
                    gap.at_ns.saturating_sub(start_ns) as f64 / 1e9,
                    us(gap.gap_ns)
                );
            }
        }
    }

    // The path of the wake-up.
    for (slot, cpu) in entries.cpus.iter().enumerate() {
        println!(
            "kernel_entries, the wake-up of slot {slot} (CPU {}), {} of them; microseconds from the instant the period was due:",
            cpu.cpu, cpu.wake_ups
        );
        meter::heading();
        meter::row("to the first interrupt", &cpu.to_first_irq);
        meter::row("to the thread on the CPU", &cpu.to_cpu);
        meter::row("from there to user space", &cpu.to_user);
        meter::row("off the CPU and on again", &cpu.off_and_on);
        let given = cpu.given_the_cpu;
        println!(
            "  given_the_cpu: never left it {}, once {}, twice {}, three times or more {}; lost it again blocked in the kernel: {}",
            given[0], given[1], given[2], given[3], cpu.blocked_again
        );
        let bands = [
            "under 5 us",
            "5 to 10 us",
            "10 to 20 us",
            "20 to 50 us",
            "50 us and over",
        ];
        for (band, words) in cpu.bands.iter().zip(bands) {
            if band.cycles == 0 {
                continue;
            }
            let mut holders: Vec<(&Holder, &u64)> = band.at_due.iter().collect();
            holders.sort_by_key(|(holder, count)| (u64::MAX - **count, **holder));
            let holders: Vec<String> = holders
                .iter()
                .map(|(holder, count)| {
                    let who = match holder {
                        Holder::Idle => "nobody".to_string(),
                        Holder::Itself => "the thread itself".to_string(),
                        Holder::Other(comm) => format!("thread {}", name(comm)),
                        Holder::Unknown => "not known".to_string(),
                    };
                    format!("{who} {count}")
                })
                .collect();
            let mut events: Vec<(&Label, &u64)> = band.events.iter().collect();
            events.sort_by_key(|(label, count)| (u64::MAX - **count, **label));
            let events: Vec<String> = events
                .iter()
                .map(|(label, count)| format!("{} {count}", label.words(symbols)))
                .collect();
            println!(
                "  wake-ups of {words}: {}; the CPU was held by: {}; before user space: {}",
                band.cycles,
                holders.join(", "),
                if events.is_empty() {
                    "nothing traced".to_string()
                } else {
                    events.join(", ")
                }
            );
        }
    }
    if !entries.stories.is_empty() {
        println!("kernel_entries, wake-ups one by one (microseconds from the instant the period was due):");
    }
    let mut stories: Vec<&Story> = entries.stories.iter().collect();
    stories.sort_by_key(|story| (story.woke, story.slot));
    for story in stories {
        println!(
            "  slot {} cycle {} at {:.3} s ({}): in user space {:.2} us after the period was due",
            story.slot,
            story.index,
            story.woke.saturating_sub(start_ns) as f64 / 1e9,
            story.why,
            us(story.woke.saturating_sub(story.due))
        );
        if story.events.is_empty() {
            println!("    nothing traced: the thread did not enter the kernel");
        }
        for event in &story.events {
            let who = if event.pid == story.ours && story.ours != 0 {
                "the thread".to_string()
            } else if event.pid == 0 {
                "at rest".to_string()
            } else {
                format!("pid {}", event.pid)
            };
            println!(
                "    {:>+10.2}  {:<10}  {}",
                (event.time_ns as i64 - story.due as i64) as f64 / 1000.0,
                who,
                told(event, story.ours, symbols)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: u64 = 2_000_000;
    const START: u64 = 1_000_000_000;
    /// The thread of the engine on CPU 4 in these tests.
    const OURS: i32 = 700;

    /// Cycle `index`: woke 3 us late, nodes for 100 us, output for 5.
    fn cycle(index: u64) -> Cycle {
        let due = START + index * PERIOD;
        Cycle {
            index,
            due,
            woke: due + 3_000,
            captured_ns: 2_000,
            computed_ns: 102_000,
            finished_ns: 107_000,
            ..Cycle::default()
        }
    }

    fn event(time_ns: u64, pid: i32, what: What, a: u64) -> Event {
        Event {
            cpu: 4,
            time_ns,
            pid,
            what,
            a,
            b: 0,
            c: 0,
            comm: [0; 16],
        }
    }

    fn switch(time_ns: u64, left: i32, taken: i32, state: u64, comm: &str) -> Event {
        let mut bytes = [0; 16];
        bytes[..comm.len()].copy_from_slice(comm.as_bytes());
        Event {
            cpu: 4,
            time_ns,
            pid: left,
            what: What::Switch,
            a: left as u64,
            b: taken as u64,
            c: state,
            comm: bytes,
        }
    }

    /// What a healthy cycle `index` leaves in the trace: the output told
    /// to the driver, the sleep, and the wake-up of the next cycle.
    fn healthy(index: u64) -> Vec<Event> {
        let due = START + index * PERIOD;
        let next = due + PERIOD;
        vec![
            event(due + 106_000, OURS, What::SyscallEnter, 16),
            event(due + 108_000, OURS, What::SyscallExit, 16),
            event(due + 111_000, OURS, What::SyscallEnter, 7),
            switch(due + 112_000, OURS, 0, 1, "swapper/4"),
            event(next + 500, 0, What::Vector("reschedule interrupt"), 253),
            switch(next + 1_500, 0, OURS, 0, "live-0"),
            event(next + 2_500, OURS, What::SyscallExit, 7),
        ]
    }

    fn run(cycles: &[Cycle], events: &[Event]) -> Entries {
        let mut entries = Entries::new(&[4], START);
        for cycle in cycles {
            entries.cycle(0, cycle);
        }
        for event in events {
            entries.event(*event);
        }
        entries.settle();
        entries
    }

    fn count(entries: &Entries, phase: Phase, label: Label) -> u64 {
        entries.counts.get(&(0, phase, label)).copied().unwrap_or(0)
    }

    #[test]
    fn system_calls_are_counted_in_the_phase_they_are_entered_in() {
        let cycles: Vec<Cycle> = (0..11).map(cycle).collect();
        let events: Vec<Event> = (0..11).flat_map(healthy).collect();
        let entries = run(&cycles, &events);
        // Ten cycles are closed: the eleventh waits for the one after.
        let cpu = &entries.cpus[0];
        assert_eq!(cpu.cycles, 10);
        assert_eq!(cpu.syscalls_per_cycle[0], [10, 0, 0, 0, 0]);
        assert_eq!(cpu.syscalls_per_cycle[1], [0, 10, 0, 0, 0]);
        assert_eq!(cpu.syscalls_per_cycle[2], [0, 10, 0, 0, 0]);
        assert_eq!(count(&entries, Phase::Delivery, Label::Syscall(16)), 10);
        assert_eq!(count(&entries, Phase::Wait, Label::Syscall(7)), 10);
        assert_eq!(count(&entries, Phase::Compute, Label::Syscall(7)), 0);
        let interrupt = Label::Vector("reschedule interrupt");
        assert_eq!(count(&entries, Phase::WakeUp, interrupt), 10);
        assert_eq!(entries.in_compute(), (0, 0));
        assert_eq!(entries.outside, 0);
    }

    #[test]
    fn the_path_of_a_wake_up_is_measured_from_the_instant_the_period_was_due() {
        let cycles: Vec<Cycle> = (0..11).map(cycle).collect();
        let events: Vec<Event> = (0..11).flat_map(healthy).collect();
        let entries = run(&cycles, &events);
        let cpu = &entries.cpus[0];
        assert_eq!(cpu.wake_ups, 10);
        assert_eq!(cpu.given_the_cpu, [0, 10, 0, 0]);
        assert_eq!(cpu.blocked_again, 0);
        assert_eq!(cpu.to_first_irq.max_ns(), Some(500));
        assert_eq!(cpu.to_cpu.max_ns(), Some(1_500));
        assert_eq!(cpu.to_user.max_ns(), Some(1_500));
        assert_eq!(cpu.off_and_on.count(), 0);
        // All under 5 us, with the CPU at rest when the period was due.
        assert_eq!(cpu.bands[0].cycles, 10);
        assert_eq!(cpu.bands[0].at_due.get(&Holder::Idle), Some(&10));
        let interrupt = Label::Vector("reschedule interrupt");
        assert_eq!(cpu.bands[0].events.get(&interrupt), Some(&10));
        // The first of them is told one by one.
        assert_eq!(entries.stories.len(), 1);
        assert_eq!(entries.stories[0].ours, OURS);
        assert_eq!(entries.stories[0].index, 1);
        assert_eq!(entries.stories[0].events.len(), 5);
    }

    #[test]
    fn a_thread_that_loses_the_cpu_again_before_user_space_is_seen() {
        let cycles: Vec<Cycle> = (0..4).map(cycle).collect();
        let mut events: Vec<Event> = Vec::new();
        for index in 0..4 {
            let next = START + (index + 1) * PERIOD;
            let mut one = healthy(index);
            if index == 1 {
                // Given the CPU, blocked on a lock, given it again.
                one.truncate(5);
                one.push(switch(next + 1_000, 0, OURS, 0, "live-0"));
                one.push(switch(next + 1_400, OURS, 0, 2, "swapper/4"));
                one.push(switch(next + 2_400, 0, OURS, 0, "live-0"));
                one.push(event(next + 2_800, OURS, What::SyscallExit, 7));
            }
            events.extend(one);
        }
        let entries = run(&cycles, &events);
        let cpu = &entries.cpus[0];
        assert_eq!(cpu.given_the_cpu, [0, 2, 1, 0]);
        assert_eq!(cpu.blocked_again, 1);
        assert_eq!(cpu.off_and_on.max_ns(), Some(1_400));
        assert!(entries
            .stories
            .iter()
            .any(|story| story.why == "given the CPU more than once" && story.index == 2));
    }

    #[test]
    fn an_interrupt_in_the_compute_phase_has_a_name_and_explains_the_step_it_made() {
        let mut cycles: Vec<Cycle> = (0..4).map(cycle).collect();
        // Cycle 1: a step of 4 us that ends 50 us into the cycle, with
        // the timer interrupt in it. Cycle 2: one of 18 us with nothing.
        cycles[1].gap_ns = 4_000;
        cycles[1].gap_end_ns = 50_000;
        cycles[2].gap_ns = 18_000;
        cycles[2].gap_end_ns = 60_000;
        // Cycle 0: a healthy step.
        cycles[0].gap_ns = 60;
        cycles[0].gap_end_ns = 30_000;
        let mut events: Vec<Event> = (0..4).flat_map(healthy).collect();
        let timer = What::Vector("local timer interrupt");
        events.push(event(cycles[1].woke + 46_500, OURS, timer, 236));
        events.push(event(
            cycles[1].woke + 47_000,
            OURS,
            What::Timer,
            0xffff_ffff_8100_0000,
        ));
        events.sort_by_key(|event| event.time_ns);
        let entries = run(&cycles, &events);
        assert_eq!(entries.in_compute(), (0, 1));
        assert_eq!(entries.in_compute_total, 2);
        assert_eq!(entries.in_compute[0].index, 1);
        assert_eq!(
            entries.in_compute[0].label,
            Label::Vector("local timer interrupt")
        );
        assert_eq!(entries.gaps.count(), 3);
        assert_eq!(
            entries
                .gaps_explained
                .get(&Label::Vector("local timer interrupt")),
            Some(&(1, 4_000))
        );
        assert_eq!(entries.gaps_unexplained.count(), 1);
        assert_eq!(entries.worst_unexplained.len(), 1);
        assert_eq!(entries.worst_unexplained[0].index, 2);
        assert_eq!(entries.worst_unexplained[0].gap_ns, 18_000);
    }

    #[test]
    fn a_system_call_in_the_compute_phase_is_found() {
        let cycles: Vec<Cycle> = (0..3).map(cycle).collect();
        let mut events: Vec<Event> = (0..3).flat_map(healthy).collect();
        events.push(event(
            cycles[1].woke + 20_000,
            OURS,
            What::SyscallEnter,
            202,
        ));
        events.sort_by_key(|event| event.time_ns);
        let entries = run(&cycles, &events);
        assert_eq!(entries.in_compute(), (1, 0));
        assert_eq!(entries.cpus[0].syscalls_per_cycle[0], [1, 1, 0, 0, 0]);
        assert_eq!(count(&entries, Phase::Compute, Label::Syscall(202)), 1);
    }

    #[test]
    fn threads_of_the_kernel_that_run_in_the_wait_are_named() {
        let cycles: Vec<Cycle> = (0..3).map(cycle).collect();
        let mut events: Vec<Event> = (0..3).flat_map(healthy).collect();
        let at = cycles[1].woke + 500_000;
        events.push(switch(at, 0, 41, 0, "ktimers/4"));
        events.push(switch(at + 3_000, 41, 0, 1, "swapper/4"));
        events.sort_by_key(|event| event.time_ns);
        let entries = run(&cycles, &events);
        let mut comm = [0; 16];
        comm[..9].copy_from_slice(b"ktimers/4");
        assert_eq!(count(&entries, Phase::Wait, Label::Ran(comm)), 1);
        assert_eq!(
            Label::Ran(comm).words(&Symbols::from_text("")),
            "thread ktimers/4"
        );
    }

    #[test]
    fn the_warm_up_events_before_the_first_cycle_and_other_cpus_are_left_out() {
        let cycles: Vec<Cycle> = (0..6).map(cycle).collect();
        let mut events: Vec<Event> = (0..6).flat_map(healthy).collect();
        // Before the first cycle, and on a CPU with no live thread.
        events.insert(0, event(START - 5_000, OURS, What::SyscallEnter, 202));
        let mut elsewhere = event(START + 10_000, 9, What::SyscallEnter, 1);
        elsewhere.cpu = 17;
        events.push(elsewhere);
        let mut entries = Entries::new(&[4], START + 3 * PERIOD);
        for cycle in &cycles {
            entries.cycle(0, cycle);
        }
        for event in &events {
            entries.event(*event);
        }
        entries.settle();
        assert_eq!(entries.outside, 2);
        // Cycles 3 and 4 are after the warm-up and closed.
        assert_eq!(entries.cpus[0].cycles, 2);
        assert_eq!(count(&entries, Phase::Wait, Label::Syscall(7)), 2);
        // A cycle out of order, a slot with no CPU: left out, no panic.
        entries.cycle(0, &cycle(2));
        entries.cycle(9, &cycle(7));
        assert_eq!(entries.out_of_order, 1);
        entries.settle();
    }

    #[test]
    fn events_wait_for_the_cycle_that_tells_their_phase() {
        let mut entries = Entries::new(&[4], START);
        entries.cycle(0, &cycle(0));
        for event in healthy(0) {
            entries.event(event);
        }
        entries.settle();
        // The cycle after is not known yet: nothing is closed.
        assert_eq!(entries.cpus[0].cycles, 0);
        assert!(entries.counts.is_empty());
        entries.cycle(0, &cycle(1));
        entries.settle();
        assert_eq!(entries.cpus[0].cycles, 1);
        assert_eq!(count(&entries, Phase::Wait, Label::Syscall(7)), 1);
    }
}
