//! The cycle of one live thread: two phases (ADR 0021), a compute budget
//! (ADR 0023, first line).
//!
//! **The budget** (as counted from step P1.7b, choice of the user of 25
//! November 2026). A cycle may compute for a period less the margin
//! **from the instant it really began**, and never past the instant the
//! card would have played all it had, less the margin. Not from the
//! instant the period was due: a USB card tells of its periods in
//! blocks of a millisecond, and now and then tells of one a millisecond
//! early, while a busy thread is still at the cycle before (open point
//! A-68). Counted from that instant the cycle after would lose the time
//! the thread was still busy, though the card had never had so much left
//! to play.
//!
//! One more bound keeps a thread that is behind from falling further
//! behind at every cycle: no cycle ends later than half a period past
//! the end of its own period. A thread whose work does not fit is cut
//! there, as before, and takes from what the card has left to play no
//! more than that half period.
//!
//! **The graph** (step P1.7c). When the run has one, each thread runs
//! the nodes the compiler gave it, in their order. A node whose inputs
//! come from another thread is not started until they are written: the
//! thread spins, reading the marks of the nodes it waits for, with no
//! system call, until they are there or the budget is over. The thread
//! that holds the card runs the inputs of the graph right after it has
//! read the card, and takes the outputs right before it writes it: an
//! output whose node was not run in time is silence for that cycle.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use dawos_audio_io::{Backend, Disturbance, Wake};
use dawos_graph::Inputs;
use dawos_rt::clock;
use dawos_rt::cpu;
use dawos_rt::dispatch::{self, Caught};
use dawos_rt::ring::Writer;
use dawos_rt::slot::Slot;
use dawos_rt::stats::Durations;

use crate::meter::{self, Cycle};
use crate::patch::Patch;
use crate::plan::{Fault, Plan};
use crate::swap::Switch;

/// What the threads of the engine share. Read-only after start-up,
/// except through atomics.
pub struct Shared {
    /// One slot per live thread.
    pub slots: Vec<Slot>,
    /// The nodes of every thread.
    pub plan: Plan,
    /// Raised when the run is over.
    pub stop: AtomicBool,
    /// Length of a period in nanoseconds.
    pub period_ns: u64,
    /// How long before the instant a cycle has to end by a thread stops
    /// taking nodes.
    pub margin_ns: u64,
    /// Frames in a period: what turns the frames a card has left to play
    /// into time. 0 without a card.
    pub frames: u32,
    /// The instant the card would have played all it had, as the thread
    /// that holds it read at the start of a cycle, and that cycle plus
    /// one (0: none yet). The other threads keep to it too.
    pub dry_at: AtomicU64,
    pub dry_cycle: AtomicU64,
    /// The fault to inject, the thread it strikes and the cycle.
    pub fault: Fault,
    pub fault_thread: usize,
    pub fault_cycle: u64,
    /// The fault of the card was made: it is made once, by whichever
    /// thread holds the card when its cycle comes.
    pub fault_made: AtomicBool,
    /// What the first live thread does with the sound of the card, when
    /// the back end has sound. Shared with the spare thread that may
    /// replace it; never waited for.
    pub patch: Option<Mutex<Patch>>,
    /// The graph the threads run in every cycle, when the run has one,
    /// and the place for the graph that takes over from it.
    pub graphs: Option<Switch>,
    /// The instant the warm-up ends: what happens to the graph before
    /// it is counted apart (an interface that is settling stops its
    /// streams, and cycles are cut for it). 0: there is none.
    pub judged_from_ns: AtomicU64,
    /// What the graph had refused when the warm-up ended; `u64::MAX`
    /// until then.
    pub graph_refused_early: AtomicU64,
    /// Whether the threads of the cycle take the numbers too small for
    /// the normal form of a float for zero (FTZ and DAZ): on, unless
    /// the run asks to see what those numbers cost.
    pub flush_denormals: bool,
}

impl Shared {
    /// The thread that holds the card says when the card runs dry in
    /// cycle `cycle`.
    #[inline]
    fn tell_dry(&self, cycle: u64, at: u64) {
        self.dry_at.store(at, Ordering::Release);
        self.dry_cycle.store(cycle + 1, Ordering::Release);
    }

    /// When the card runs dry in cycle `cycle`, if the thread that holds
    /// it has said so yet. A thread that asks a moment too soon gets
    /// nothing and keeps to its own budget until it asks again, between
    /// two nodes.
    #[inline]
    fn dry(&self, cycle: u64) -> Option<u64> {
        (self.dry_cycle.load(Ordering::Acquire) == cycle + 1)
            .then(|| self.dry_at.load(Ordering::Acquire))
    }
}

/// What one thread measured. Owned by the thread while it runs, handed
/// back when it ends.
pub struct ThreadReport {
    /// The slot the thread held.
    pub slot: usize,
    /// Whether it was a spare thread, brought in by the watchdog.
    pub spare: bool,
    /// How it ended.
    pub end: End,
    /// Cycles run.
    pub cycles: u64,
    /// Periods that went by unserved.
    pub missed: u64,
    /// Cycles cut short by the compute budget.
    pub incomplete: u64,
    /// How late each wake-up was: of the cycles the thread slept before.
    pub lateness: Durations,
    /// Cycles that began behind: their period was due before the cycle
    /// before had ended, and the thread did not sleep.
    pub behind: u64,
    /// How long each compute phase lasted.
    pub compute: Durations,
    /// The shortest time left before the instant a cycle had to end by,
    /// in nanoseconds; `None` when a cycle ended after it.
    pub least_margin_ns: Option<u64>,
    /// Cycles that ended after the instant they had to end by.
    pub overruns: u64,
    /// System calls stopped in the compute phase, and the first of
    /// them one by one.
    pub caught: u64,
    pub caught_calls: Vec<Caught>,
    /// System calls made on purpose in the compute phase (a test).
    pub calls_on_purpose: u64,
    /// Nodes of the graph run.
    pub graph_nodes: u64,
    /// How long the thread spun for the inputs of a node that another
    /// thread writes: of the nodes it had to wait for.
    pub graph_waits: Durations,
    /// Cycles in which this thread did not get to the end of its nodes
    /// of the graph, after the warm-up and in it.
    pub graph_cut: u64,
    pub graph_cut_early: u64,
    /// Of those after the warm-up: the cycles in which the inputs of a
    /// node were not there when the budget was over, or were gone.
    pub graph_starved: u64,
    /// Outputs of the card the graph left silent for a cycle, after the
    /// warm-up and in it (the thread that holds the card counts them).
    pub graph_silent: u64,
    pub graph_silent_early: u64,
    /// Times the thread found another graph in the place of the one of
    /// the cycle before.
    pub graph_changes: u64,
    /// Cycles for which no graph could be had: none, by the order in
    /// which a change is made.
    pub graph_missing: u64,
}

/// How long a node of the graph took, over the cycles it was run in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeTime {
    /// Cycles it was run in.
    pub runs: u64,
    /// Nanoseconds in all, and the longest run.
    pub sum_ns: u64,
    pub max_ns: u64,
}

/// How a live thread ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// The run was over.
    Stopped,
    /// The watchdog had replaced it, and it found out on coming back.
    Retired,
}

/// Run the cycle on slot `slot` until the run is over or the watchdog
/// retires this thread. A spare thread passes `first_cycle`, where the
/// instant of its first cycle is written. `record`: the ring where every
/// cycle is written for the meter, when the record is kept. Nothing in
/// the loop allocates or prints.
pub fn live_loop(
    shared: &Shared,
    slot_index: usize,
    generation: u32,
    first_cycle: Option<&AtomicU64>,
    backend: &mut dyn Backend,
    record: Option<&mut Writer>,
) -> ThreadReport {
    // A setting of this thread, for as long as it lives: a filter fed
    // with silence must not slow down on the way to zero.
    if shared.flush_denormals {
        cpu::flush_denormals(true);
    }
    let mut report = cycles(shared, slot_index, generation, first_cycle, backend, record);
    // Out of the cycle: system calls go through, and memory may be
    // asked for.
    dispatch::allow();
    report.caught = dispatch::caught();
    report.caught_calls = dispatch::kept();
    report
}

/// The loop itself.
fn cycles(
    shared: &Shared,
    slot_index: usize,
    generation: u32,
    first_cycle: Option<&AtomicU64>,
    backend: &mut dyn Backend,
    mut record: Option<&mut Writer>,
) -> ThreadReport {
    let spare = first_cycle.is_some();
    let slot = &shared.slots[slot_index];
    let nodes = shared.plan.nodes(slot_index);
    let mut report = ThreadReport {
        slot: slot_index,
        spare,
        end: End::Stopped,
        cycles: 0,
        missed: 0,
        incomplete: 0,
        lateness: Durations::new(),
        behind: 0,
        compute: Durations::new(),
        least_margin_ns: Some(u64::MAX),
        overruns: 0,
        caught: 0,
        caught_calls: Vec::new(),
        calls_on_purpose: 0,
        graph_nodes: 0,
        graph_waits: Durations::new(),
        graph_cut: 0,
        graph_cut_early: 0,
        graph_starved: 0,
        graph_silent: 0,
        graph_silent_early: 0,
        graph_changes: 0,
        graph_missing: 0,
    };
    let graphs = shared.graphs.as_ref();
    // The place of the graph of the cycle before.
    let mut last_place: Option<usize> = None;
    // System calls stopped so far, to tell those of each cycle.
    let mut caught_before = dispatch::caught();
    let mut first = true;
    // Sound is the business of the first slot alone.
    let patch = shared.patch.as_ref().filter(|_| slot_index == 0);
    // For a cycle in which the patch cannot be had: the card must be
    // read and written all the same, or its input stays where it is and
    // the wait for the next period returns at once (open point A-70).
    // Before the first cycle: the allocations of the loop.
    let (mut idle_input, idle_output) = if patch.is_some() {
        let (inputs, outputs) = backend.channels();
        let frames = shared.frames as usize;
        (vec![0_i32; inputs * frames], vec![0_i32; outputs * frames])
    } else {
        (Vec::new(), Vec::new())
    };
    // When the cycle before ended: a period due before then finds the
    // thread still busy.
    let mut last_finished: Option<u64> = None;

    loop {
        // ---- Wait phase: the one blocking call of the cycle.
        let wake = backend.wait();
        if shared.stop.load(Ordering::Acquire) {
            return report;
        }
        if slot.generation() != generation {
            report.end = End::Retired;
            return report;
        }

        // The graph of this cycle, held until the cycle ends: a change
        // comes between two cycles, for every thread at the same one.
        let held = graphs.map(|switch| switch.for_cycle(wake.index));
        let graph = match &held {
            Some(Some((place, graph))) => {
                if last_place.is_some_and(|last| last != *place) {
                    report.graph_changes += 1;
                }
                last_place = Some(*place);
                Some(&**graph)
            }
            Some(None) => {
                report.graph_missing += 1;
                None
            }
            None => None,
        };

        // ---- Compute phase: no system call from here to the next wait.
        let behind = last_finished.is_some_and(|finished| wake.due <= finished);
        // The cycle has to end a period after it really began, no later
        // than half a period into the next one, and before the card has
        // played all it had.
        let mut allowed_end =
            (wake.woke + shared.period_ns).min(wake.due + shared.period_ns * 3 / 2);
        if slot_index == 0
            && shared.frames > 0
            && wake.output_queued > 0
            && wake.events & (Wake::ON_THE_CLOCK | Wake::STALLED) == 0
        {
            let dry = wake.due
                + u64::from(wake.output_queued) * shared.period_ns / u64::from(shared.frames);
            shared.tell_dry(wake.index, dry);
            allowed_end = allowed_end.min(dry);
        }
        let deadline = allowed_end.saturating_sub(shared.margin_ns);
        slot.enter_compute(wake.index, wake.woke);
        // From here to the end of the nodes the kernel stops every
        // system call of this thread (when it was asked to: one store
        // that nobody reads otherwise).
        dispatch::block(wake.index);
        // By whichever thread holds the slot: a spare one too.
        let call_on_purpose = shared.fault == Fault::Syscall
            && slot_index == shared.fault_thread
            && wake.index >= shared.fault_cycle;
        if first {
            if let Some(first_cycle) = first_cycle {
                first_cycle.store(wake.woke, Ordering::Release);
            }
            first = false;
        }

        // The input of this period, and what the patch makes of it. A
        // patch that cannot be had (a thread went wrong while holding
        // it) is a cycle without sound, not a wait.
        let mut flags = u16::from(wake.events);
        if behind {
            flags |= meter::BEHIND;
        }
        let mut captured_for_graph = false;
        if let Some(patch) = patch {
            if let Ok(mut patch) = patch.try_lock() {
                backend.capture(patch.input_mut());
                patch.process();
                if let Some(graph) = graph {
                    graph.capture(wake.index, patch.input(), patch.channels().0);
                    captured_for_graph = true;
                }
            } else {
                flags |= meter::NO_PATCH;
                backend.capture(&mut idle_input);
            }
        }
        // Without a card, or without its input: the graph hears
        // silence.
        if let Some(graph) = graph.filter(|_| slot_index == 0 && !captured_for_graph) {
            graph.capture(wake.index, &[], 0);
        }
        if slot_index == 0
            && wake.index >= shared.fault_cycle
            && matches!(shared.fault, Fault::CardXrun | Fault::CardLost)
            && !shared.fault_made.swap(true, Ordering::AcqRel)
        {
            backend.disturb(if shared.fault == Fault::CardXrun {
                Disturbance::Xrun
            } else {
                Disturbance::CardLost
            });
            flags |= meter::DISTURBED;
        }
        let captured = clock::now();
        // What happens to the graph in the warm-up is counted apart.
        let judged = wake.woke >= shared.judged_from_ns.load(Ordering::Relaxed);
        if let Some(graph) = graph.filter(|_| slot_index == 0 && judged) {
            // Once: what had been refused until here.
            if shared.graph_refused_early.load(Ordering::Relaxed) == u64::MAX {
                let _ = shared.graph_refused_early.compare_exchange(
                    u64::MAX,
                    graph.refused(),
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                );
            }
        }

        let mut incomplete = false;
        let mut nodes_run = 0_u16;
        // The longest step between two readings of the clock inside the
        // nodes, and when it ended.
        let mut gap = (0_u64, 0_u64);
        // The compute budget: past the deadline no node is started.
        // The threads that do not hold the card keep to what the one
        // that does has read of it.
        let limit = || {
            if slot_index == 0 {
                deadline
            } else {
                shared.dry(wake.index).map_or(deadline, |dry| {
                    deadline.min(dry.saturating_sub(shared.margin_ns))
                })
            }
        };
        for (index, node) in nodes.iter().enumerate() {
            if node.is_abandoned() {
                continue;
            }
            if clock::now() >= limit() {
                incomplete = true;
                break;
            }
            slot.enter_node(index);
            dispatch::in_node(Some(index as u32));
            if call_on_purpose && index == 0 {
                report.calls_on_purpose += 1;
                std::thread::yield_now();
            }
            let step = node.run(wake.index);
            slot.leave_node();
            if step.0 > gap.0 {
                gap = step;
            }
            nodes_run = nodes_run.saturating_add(1);
            // Replaced while inside the node: a node that was only very
            // slow. The slot belongs to another thread now.
            if slot.generation() != generation {
                report.end = End::Retired;
                return report;
            }
        }
        dispatch::in_node(None);

        // The nodes of the graph this thread was given, in their order.
        // The slot says which one the thread is in: a thread stuck in a
        // node has that node given up by the watchdog (step P1.7f).
        if let Some(graph) = graph.filter(|_| !incomplete) {
            // One reading of the clock per node: the end of a node is
            // the beginning of the next.
            let mut now = clock::now();
            for &node in graph.nodes_of(slot_index) {
                // The inputs of the card were run by `capture`.
                if graph.is_input(node) || graph.is_abandoned(node) {
                    continue;
                }
                let mut inputs = graph.inputs_of(node, wake.index);
                if inputs == Inputs::NotYet {
                    // Another thread is still writing what this node
                    // reads: spin, with an eye on the budget.
                    let began = now;
                    while now < limit() {
                        std::hint::spin_loop();
                        inputs = graph.inputs_of(node, wake.index);
                        now = clock::now();
                        if inputs != Inputs::NotYet {
                            break;
                        }
                    }
                    if inputs == Inputs::Ready {
                        report.graph_waits.add(now - began);
                    }
                }
                // Not there when the budget was over; or the others are
                // at a later cycle already, and this thread is behind:
                // the sooner it ends this cycle, the sooner it is back
                // with them.
                if inputs != Inputs::Ready {
                    report.graph_starved += u64::from(judged);
                    incomplete = true;
                    break;
                }
                if now >= limit() {
                    incomplete = true;
                    break;
                }
                let began = now;
                slot.enter_node(node);
                graph.run(node, wake.index);
                slot.leave_node();
                now = clock::now();
                graph.record(node, now - began);
                report.graph_nodes += 1;
                nodes_run = nodes_run.saturating_add(1);
                if slot.generation() != generation {
                    report.end = End::Retired;
                    return report;
                }
            }
        }
        if graph.is_some() && incomplete {
            if judged {
                report.graph_cut += 1;
            } else {
                report.graph_cut_early += 1;
            }
        }
        if call_on_purpose && nodes.is_empty() {
            report.calls_on_purpose += 1;
            std::thread::yield_now();
        }

        // A thread gone wrong outside any node: it never reaches the
        // wait.
        if shared.fault == Fault::StuckThread
            && slot_index == shared.fault_thread
            && wake.index >= shared.fault_cycle
            && !spare
        {
            loop {
                std::hint::spin_loop();
            }
        }

        let computed = clock::now();
        // The compute phase is over: the output may tell the driver.
        dispatch::allow();
        let caught_now = dispatch::caught();
        let caught = u16::try_from(caught_now - caught_before).unwrap_or(u16::MAX);
        caught_before = caught_now;

        // The output, before the period ends: whatever the nodes did or
        // did not finish.
        let mut silent = None;
        if let Some(patch) = patch {
            if let Ok(mut patch) = patch.try_lock() {
                if let Some(graph) = graph {
                    // What the graph did not finish stays as the patch
                    // left it: silence.
                    let channels = patch.channels().1;
                    silent = Some(graph.deliver(wake.index, patch.output_mut(), channels));
                }
                backend.deliver(patch.output());
            } else {
                backend.deliver(&idle_output);
            }
        }
        // Without a card the outputs go nowhere: those not run in time
        // are counted all the same.
        if let Some(graph) = graph.filter(|_| slot_index == 0) {
            let silent = silent.unwrap_or_else(|| graph.deliver(wake.index, &mut [], 0));
            if judged {
                report.graph_silent += u64::from(silent);
            } else {
                report.graph_silent_early += u64::from(silent);
            }
        }

        let finished = clock::now();
        report.cycles += 1;
        report.missed += wake.missed;
        report.incomplete += u64::from(incomplete);
        if behind {
            report.behind += 1;
        } else {
            report.lateness.add(wake.lateness_ns());
        }
        report.compute.add(finished.saturating_sub(wake.woke));
        if slot_index != 0 {
            if let Some(dry) = shared.dry(wake.index) {
                allowed_end = allowed_end.min(dry);
            }
        }
        last_finished = Some(finished);
        if finished <= allowed_end {
            let margin = allowed_end - finished;
            report.least_margin_ns = report.least_margin_ns.map(|least| least.min(margin));
        } else {
            report.least_margin_ns = None;
            report.overruns += 1;
        }
        // The record of this cycle, for the meter: a few stores, and if
        // the ring is full the record is lost and counted.
        if let Some(record) = record.as_deref_mut() {
            let since =
                |instant: u64| u32::try_from(instant.saturating_sub(wake.woke)).unwrap_or(u32::MAX);
            if incomplete {
                flags |= meter::INCOMPLETE;
            }
            let cycle = Cycle {
                index: wake.index,
                due: wake.due,
                woke: wake.woke,
                captured_ns: since(captured),
                computed_ns: since(computed),
                finished_ns: since(finished),
                missed: u32::try_from(wake.missed).unwrap_or(u32::MAX),
                input_ready: wake.input_ready,
                output_queued: wake.output_queued,
                nodes: nodes_run,
                flags,
                slot: u16::try_from(slot_index).unwrap_or(u16::MAX),
                gap_ns: u32::try_from(gap.0).unwrap_or(u32::MAX),
                gap_end_ns: since(gap.1),
                caught,
            };
            record.push(&cycle.to_record());
        }
        slot.enter_wait();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dawos_audio_io::Format;

    use super::*;

    /// A source of periods that, like a sound card, never drops one: a
    /// period already due when the thread comes to wait is handed over
    /// at once, with the instant it was due.
    struct Queueing {
        shared: Arc<Shared>,
        start: u64,
        next: u64,
        cycles: u64,
        /// How long after it was due each period was taken.
        late: Vec<u64>,
        /// Channels it has, and how many times it was read and written.
        channels: usize,
        captured: u64,
        delivered: u64,
    }

    impl Backend for Queueing {
        fn name(&self) -> &'static str {
            "queueing"
        }

        fn format(&self) -> Format {
            Format {
                rate: 48_000,
                frames: 96,
            }
        }

        fn wait(&mut self) -> Wake {
            if self.next == self.cycles {
                self.shared.stop.store(true, Ordering::Release);
            }
            let due = self.start + (self.next + 1) * self.shared.period_ns;
            clock::sleep_until(due);
            let woke = clock::now();
            self.late.push(woke - due);
            let index = self.next;
            self.next += 1;
            Wake {
                index,
                due,
                woke,
                missed: 0,
                input_ready: 0,
                output_queued: 0,
                events: 0,
            }
        }

        fn channels(&self) -> (usize, usize) {
            (self.channels, self.channels)
        }

        fn capture(&mut self, input: &mut [i32]) {
            assert_eq!(input.len(), self.channels * 96);
            self.captured += 1;
        }

        fn deliver(&mut self, output: &[i32]) {
            assert_eq!(output.len(), self.channels * 96);
            self.delivered += 1;
        }
    }

    fn shared(plan: Plan, period_ns: u64, margin_ns: u64, patch: Option<Mutex<Patch>>) -> Shared {
        Shared {
            slots: vec![Slot::new()],
            plan,
            stop: AtomicBool::new(false),
            period_ns,
            margin_ns,
            frames: if patch.is_some() { 96 } else { 0 },
            dry_at: AtomicU64::new(0),
            dry_cycle: AtomicU64::new(0),
            fault: Fault::None,
            fault_thread: 0,
            fault_cycle: 0,
            fault_made: AtomicBool::new(false),
            patch,
            graphs: None,
            judged_from_ns: AtomicU64::new(0),
            graph_refused_early: AtomicU64::new(u64::MAX),
            flush_denormals: true,
        }
    }

    /// The patch is held by somebody else (a thread that went wrong
    /// while it had it): the cycles have no sound, but the card is read
    /// and written at every one of them all the same. Were it not, its
    /// input would stay where it is, the wait for the next period would
    /// return at once, and the thread would run cycles as fast as it
    /// can (open point A-70).
    #[test]
    fn without_the_patch_the_card_is_still_read_and_written() {
        const PERIOD: u64 = 2_000_000;
        const CYCLES: u64 = 40;
        let patch = Patch::new(&crate::patch::PatchConfig::default(), 2, 2, 96, 48_000)
            .expect("a patch with nothing to do");
        let shared = Arc::new(shared(
            Plan::new(1, PERIOD, 0, Fault::None, 0, 0),
            PERIOD,
            150_000,
            Some(Mutex::new(patch)),
        ));
        let mut backend = Queueing {
            shared: Arc::clone(&shared),
            start: clock::now(),
            next: 0,
            cycles: CYCLES,
            late: Vec::new(),
            channels: 2,
            captured: 0,
            delivered: 0,
        };
        let generation = shared.slots[0].generation();
        // Half of the cycles with the patch held from here, half free.
        let report = {
            let held = shared.patch.as_ref().unwrap().lock().unwrap();
            let shared = Arc::clone(&shared);
            let handle = std::thread::spawn(move || {
                let report = live_loop(&shared, 0, generation, None, &mut backend, None);
                (report, backend.captured, backend.delivered)
            });
            std::thread::sleep(std::time::Duration::from_nanos(PERIOD * CYCLES / 2));
            drop(held);
            handle.join().unwrap()
        };
        assert_eq!(report.0.cycles, CYCLES);
        assert_eq!((report.1, report.2), (CYCLES, CYCLES));
    }

    /// A thread with half as much work again as fits, in nodes longer
    /// than the margin: every cycle is cut by the budget, and ends a
    /// little after it. The thread begins its cycles behind, by a time
    /// that stays within bounds: it does not fall further back at every
    /// cycle, as it would with a budget counted from its real start and
    /// nothing else.
    #[test]
    fn a_thread_with_too_much_work_stays_behind_by_a_bounded_time() {
        const PERIOD: u64 = 5_000_000;
        const CYCLES: u64 = 80;
        // Fifty nodes of 150 us: 7.5 ms of work for 5 ms. A margin far
        // shorter than a node.
        let shared = Arc::new(shared(
            Plan::new(1, PERIOD, 150, Fault::None, 0, 0),
            PERIOD,
            20_000,
            None,
        ));
        let mut backend = Queueing {
            shared: Arc::clone(&shared),
            start: clock::now(),
            next: 0,
            cycles: CYCLES,
            late: Vec::new(),
            channels: 0,
            captured: 0,
            delivered: 0,
        };
        let generation = shared.slots[0].generation();
        let report = live_loop(&shared, 0, generation, None, &mut backend, None);
        assert_eq!(report.cycles, CYCLES);
        assert!(
            report.incomplete * 10 >= CYCLES * 9,
            "{} cycles cut of {CYCLES}",
            report.incomplete
        );
        assert!(report.behind * 2 >= CYCLES, "{} behind", report.behind);
        // With nothing to bound it the thread would be 130 us further
        // behind at every cycle, nearly two periods by the end, and none
        // of the last twenty cycles would begin less than a period after
        // it was due. Bounded, it keeps coming back to about half a
        // period behind. On a machine at work the thread is held back
        // for milliseconds at a time and takes some cycles to come back:
        // a few of the twenty are enough to tell.
        let last = &backend.late[backend.late.len() - 20..];
        let in_bounds = last.iter().filter(|late| **late < PERIOD).count();
        assert!(in_bounds >= 3, "{last:?}");
    }
}
