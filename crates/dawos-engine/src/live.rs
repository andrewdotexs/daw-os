//! The cycle of one live thread: two phases (ADR 0021), a compute budget
//! (ADR 0023, first line).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use dawos_audio_io::{Backend, Disturbance};
use dawos_rt::clock;
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
    };
    let mut first = true;
    // Sound is the business of the first slot alone.
    let patch = shared.patch.as_ref().filter(|_| slot_index == 0);
    let mut disturbed = false;

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
        if !disturbed && slot_index == 0 && !spare && wake.index >= shared.fault_cycle {
            match shared.fault {
                Fault::CardXrun => {
                    backend.disturb(Disturbance::Xrun);
                    flags |= meter::DISTURBED;
                }
                Fault::CardLost => {
                    backend.disturb(Disturbance::CardLost);
                    flags |= meter::DISTURBED;
                }
                _ => {}
            }
            disturbed = true;
        }
        let captured = clock::now();

        let mut incomplete = false;
        let mut nodes_run = 0_u16;
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
            node.run(wake.index);
            slot.leave_node();
            nodes_run = nodes_run.saturating_add(1);
            // Replaced while inside the node: a node that was only very
            // slow. The slot belongs to another thread now.
            if slot.generation() != generation {
                report.end = End::Retired;
                return report;
            }
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
            };
            record.push(&cycle.to_record());
        }
        slot.enter_wait();
    }
}
