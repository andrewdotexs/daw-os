//! The cycle of one live thread: two phases (ADR 0021), a compute budget
//! (ADR 0023, first line).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use dawos_audio_io::{Backend, Disturbance};
use dawos_rt::clock;
use dawos_rt::dispatch::{self, Caught};
use dawos_rt::ring::Writer;
use dawos_rt::slot::Slot;
use dawos_rt::stats::Durations;

use crate::meter::{self, Cycle};
use crate::patch::Patch;
use crate::plan::{Fault, Plan};

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
    /// How long before the end of the period a thread stops taking nodes.
    pub margin_ns: u64,
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
    /// How late each wake-up was.
    pub lateness: Durations,
    /// How long each compute phase lasted.
    pub compute: Durations,
    /// The shortest time left before the end of the period, in
    /// nanoseconds; `None` when a cycle ended after its period.
    pub least_margin_ns: Option<u64>,
    /// Cycles that ended after their period.
    pub overruns: u64,
    /// System calls stopped in the compute phase, and the first of
    /// them one by one.
    pub caught: u64,
    pub caught_calls: Vec<Caught>,
    /// System calls made on purpose in the compute phase (a test).
    pub calls_on_purpose: u64,
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
        compute: Durations::new(),
        least_margin_ns: Some(u64::MAX),
        overruns: 0,
        caught: 0,
        caught_calls: Vec::new(),
        calls_on_purpose: 0,
    };
    // System calls stopped so far, to tell those of each cycle.
    let mut caught_before = dispatch::caught();
    let mut first = true;
    // Sound is the business of the first slot alone.
    let patch = shared.patch.as_ref().filter(|_| slot_index == 0);

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

        // ---- Compute phase: no system call from here to the next wait.
        let period_end = wake.due + shared.period_ns;
        let deadline = period_end.saturating_sub(shared.margin_ns);
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
        if let Some(patch) = patch {
            if let Ok(mut patch) = patch.try_lock() {
                backend.capture(patch.input_mut());
                patch.process();
            } else {
                flags |= meter::NO_PATCH;
            }
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

        let mut incomplete = false;
        let mut nodes_run = 0_u16;
        // The longest step between two readings of the clock inside the
        // nodes, and when it ended.
        let mut gap = (0_u64, 0_u64);
        for (index, node) in nodes.iter().enumerate() {
            if node.is_abandoned() {
                continue;
            }
            // The compute budget: past the deadline no node is started.
            if clock::now() >= deadline {
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
        if let Some(patch) = patch {
            if let Ok(patch) = patch.try_lock() {
                backend.deliver(patch.output());
            }
        }

        let finished = clock::now();
        report.cycles += 1;
        report.missed += wake.missed;
        report.incomplete += u64::from(incomplete);
        report.lateness.add(wake.lateness_ns());
        report.compute.add(finished.saturating_sub(wake.woke));
        if finished <= period_end {
            let margin = period_end - finished;
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
