//! The watchdog (ADR 0023, second line): a thread outside the live CPUs
//! that notices a live thread stuck in its compute phase, takes the CPU
//! back from it and puts a spare thread in its place.

use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use dawos_rt::clock;
use dawos_rt::cpu::{self, Policy};
use dawos_rt::gate::Gate;

use crate::live::Shared;
use crate::plan::Fault;

/// A thread is stuck when its compute phase has lasted this many
/// periods. A healthy cycle lasts less than one, and the compute budget
/// stops a heavy one before the end of the first.
pub const STUCK_PERIODS: u64 = 3;

/// A spare thread, parked outside the live CPUs until needed.
#[derive(Default)]
pub struct Spare {
    /// Opened by the watchdog to send the thread to work.
    pub gate: Gate,
    /// The kernel's number of the thread, written by the thread itself.
    pub tid: AtomicI32,
    /// The slot to take and the generation to take it at.
    pub slot: AtomicUsize,
    pub generation: AtomicU32,
    /// When the thread ran its first cycle; 0: not yet.
    pub first_cycle_ns: AtomicU64,
}

/// What the watchdog did about one stuck thread. Written once by the
/// watchdog; read after the run.
#[derive(Default)]
pub struct Event {
    /// 1 when the event happened.
    pub happened: AtomicU32,
    pub slot: AtomicUsize,
    pub cycle: AtomicU64,
    /// The node the thread was in, plus one; 0: outside any node.
    pub node: AtomicU32,
    /// The thread that was stuck.
    pub tid: AtomicI32,
    /// When its compute phase had begun.
    pub since_ns: AtomicU64,
    /// When the watchdog noticed.
    pub detected_ns: AtomicU64,
    /// When the thread was off the live CPU, without real-time priority.
    pub demoted_ns: AtomicU64,
    /// Whether demotion and move succeeded (0: yes).
    pub demote_errors: AtomicU32,
    /// The spare thread sent in, plus one; 0: none was left.
    pub spare: AtomicUsize,
}

/// What the watchdog needs.
pub struct Watch<'a> {
    pub shared: &'a Shared,
    pub spares: &'a [Spare],
    pub events: &'a [Event],
    /// Counts the rounds of the watchdog: the proof that it is alive,
    /// which the notifier of systemd looks at.
    pub heartbeat: &'a AtomicU64,
    /// CPU of each slot, and where a stuck thread is sent.
    pub live_cpus: &'a [usize],
    pub system_cpus: &'a [usize],
    /// Priority the spare thread gets on the live CPU; `None`: the run is
    /// not real time.
    pub priority: Option<u8>,
    /// When the periods started, to know which cycle it is.
    pub start_ns: u64,
}

/// The loop of the watchdog: one round per period.
pub fn watchdog_loop(watch: &Watch<'_>) {
    let shared = watch.shared;
    let limit_ns = STUCK_PERIODS * shared.period_ns;
    let mut next_spare = 0;
    let mut next_event = 0;
    let mut round: u64 = 0;

    loop {
        round += 1;
        clock::sleep_until(watch.start_ns + round * shared.period_ns);
        if shared.stop.load(Ordering::Acquire) {
            return;
        }
        // The fault "watchdog off": from the cycle of the fault on, the
        // watchdog does nothing more, and its heartbeat stops.
        if shared.fault == Fault::WatchdogOff && round >= shared.fault_cycle {
            while !shared.stop.load(Ordering::Acquire) {
                clock::sleep_until(clock::now() + 100_000_000);
            }
            return;
        }
        watch.heartbeat.fetch_add(1, Ordering::Release);

        let now = clock::now();
        for (index, slot) in shared.slots.iter().enumerate() {
            let Some(since) = slot.computing_since() else {
                continue;
            };
            if now.saturating_sub(since) <= limit_ns {
                continue;
            }

            // ---- A stuck thread. Everything below runs here, on a CPU
            // that is not live.
            let tid = slot.holder();
            let node = slot.node();
            let cycle = slot.cycle();

            // 1. Off the live CPU, without real-time priority. (With no
            // system CPUs named, as on a development PC, the thread stays
            // where the kernel put it, at the lowest priority.)
            let mut errors = 0;
            if cpu::set_policy(tid, Policy::Idle).is_err() {
                errors += 1;
            }
            if !watch.system_cpus.is_empty() && cpu::set_affinity(tid, watch.system_cpus).is_err() {
                errors += 1;
            }
            let demoted = clock::now();

            // 2. The node it was in is given up; the slot changes hands.
            if let Some(node) = node {
                shared.plan.nodes(index)[node].abandon();
            }
            let generation = slot.retire_holder();

            // 3. A spare thread takes the slot, on the same CPU.
            let mut sent = 0;
            if let Some(spare) = watch.spares.get(next_spare) {
                next_spare += 1;
                sent = next_spare;
                let spare_tid = spare.tid.load(Ordering::Acquire);
                spare.slot.store(index, Ordering::Release);
                spare.generation.store(generation, Ordering::Release);
                if let Some(&cpu) = watch.live_cpus.get(index) {
                    let _ = cpu::set_affinity(spare_tid, &[cpu]);
                }
                if let Some(priority) = watch.priority {
                    let _ = cpu::set_policy(spare_tid, Policy::Fifo(priority));
                }
                spare.gate.open();
            }

            // 5. Tell. (Step 4, a graph without the node, comes with the
            // graph in step P1.7: until then the node is skipped.)
            if let Some(event) = watch.events.get(next_event) {
                next_event += 1;
                event.slot.store(index, Ordering::Release);
                event.cycle.store(cycle, Ordering::Release);
                event
                    .node
                    .store(node.map_or(0, |node| node as u32 + 1), Ordering::Release);
                event.tid.store(tid, Ordering::Release);
                event.since_ns.store(since, Ordering::Release);
                event.detected_ns.store(now, Ordering::Release);
                event.demoted_ns.store(demoted, Ordering::Release);
                event.demote_errors.store(errors, Ordering::Release);
                event.spare.store(sent, Ordering::Release);
                event.happened.store(1, Ordering::Release);
            }
            eprintln!(
                "dawos-engine: watchdog: thread {tid} on live slot {index} stuck in cycle {cycle} ({}): moved off its CPU{}",
                node.map_or_else(|| "outside any node".to_string(), |node| format!("node {node}")),
                if sent > 0 { ", spare thread sent in" } else { ", no spare thread left" },
            );
        }
    }
}
