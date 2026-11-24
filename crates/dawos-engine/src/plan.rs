//! What each live thread does in a cycle, for step P1.3: test nodes.
//!
//! The real graph comes in step P1.7. Until then a plan is a list of
//! nodes per thread, with no dependencies between them: enough to give
//! the cycle a load, and to make a node or a thread go wrong on purpose
//! (ADR 0023).

use std::sync::atomic::{AtomicBool, Ordering};

use dawos_rt::clock;

/// What goes wrong, and when.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// Nothing.
    None,
    /// A node never returns.
    StuckNode,
    /// A thread stops respecting the phases, outside any node.
    StuckThread,
    /// The watchdog stops, then a node never returns: only the second
    /// line (systemd) is left.
    WatchdogOff,
    /// The engine skips input and output for some periods: the driver
    /// stops the card, as when the engine is late (ALSA back end).
    CardXrun,
    /// The card is taken away, as if unplugged (ALSA back end).
    CardLost,
}

/// What a test node does when it is run.
#[derive(Clone, Copy, Debug)]
pub enum Work {
    /// Keep the CPU busy for this many nanoseconds.
    Spin(u64),
    /// From this cycle on, never return.
    StuckFrom(u64),
}

/// One node of a plan.
pub struct Node {
    work: Work,
    /// Set by the watchdog when the thread running this node is stuck in
    /// it: nobody runs the node again.
    abandoned: AtomicBool,
}

impl Node {
    fn new(work: Work) -> Self {
        Self {
            work,
            abandoned: AtomicBool::new(false),
        }
    }

    /// Whether the watchdog gave this node up.
    #[inline]
    pub fn is_abandoned(&self) -> bool {
        self.abandoned.load(Ordering::Acquire)
    }

    /// Give the node up.
    pub fn abandon(&self) {
        self.abandoned.store(true, Ordering::Release);
    }

    /// Run the node in cycle `cycle`. Compute phase: no system call.
    ///
    /// A node that keeps the CPU busy does it by reading the clock
    /// again and again, some tens of nanoseconds apart. A longer step
    /// between two readings is time something took from the thread: the
    /// longest step of the run is returned, with the instant it ended
    /// (0 and 0 for a node that does not read the clock).
    #[inline]
    pub fn run(&self, cycle: u64) -> (u64, clock::Instant) {
        match self.work {
            Work::Spin(ns) => {
                let mut before = clock::now();
                let end = before + ns;
                let mut longest = (0, 0);
                loop {
                    let now = clock::now();
                    if now - before > longest.0 {
                        longest = (now - before, now);
                    }
                    if now >= end {
                        return longest;
                    }
                    before = now;
                    std::hint::spin_loop();
                }
            }
            Work::StuckFrom(from) => {
                if cycle >= from {
                    // A node gone wrong: it does not look at anything and
                    // does not come back.
                    loop {
                        std::hint::spin_loop();
                    }
                }
                (0, 0)
            }
        }
    }
}

/// The nodes of every live thread.
pub struct Plan {
    threads: Vec<Vec<Node>>,
}

/// How many nodes the load of one thread is split into. The compute
/// budget is checked between nodes: a node that starts just before the
/// deadline runs to its end, so the budget only holds if nodes are
/// shorter than the margin. Fifty nodes at 100 % of a period of 2.667 ms
/// last 53 us each, a third of the default margin.
const LOAD_NODES: u64 = 50;

impl Plan {
    /// A plan for `threads` threads, each busy for `load_percent` of a
    /// period of `period_ns`, with `fault` striking thread
    /// `fault_thread` at cycle `fault_cycle`.
    pub fn new(
        threads: usize,
        period_ns: u64,
        load_percent: u64,
        fault: Fault,
        fault_thread: usize,
        fault_cycle: u64,
    ) -> Self {
        let slice_ns = period_ns * load_percent / 100 / LOAD_NODES;
        let threads = (0..threads)
            .map(|thread| {
                let mut nodes = Vec::new();
                if thread == fault_thread && matches!(fault, Fault::StuckNode | Fault::WatchdogOff)
                {
                    nodes.push(Node::new(Work::StuckFrom(fault_cycle)));
                }
                if slice_ns > 0 {
                    nodes.extend((0..LOAD_NODES).map(|_| Node::new(Work::Spin(slice_ns))));
                }
                nodes
            })
            .collect();
        Self { threads }
    }

    /// The nodes of thread `thread`.
    #[inline]
    pub fn nodes(&self, thread: usize) -> &[Node] {
        &self.threads[thread]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_spreads_the_load() {
        let plan = Plan::new(2, 1_000_000, 50, Fault::None, 0, 0);
        assert_eq!(plan.nodes(0).len(), 50);
        assert_eq!(plan.nodes(1).len(), 50);
        // Fifty nodes of 10 us: half a period of one millisecond.
        let before = clock::now();
        for node in plan.nodes(0) {
            node.run(0);
        }
        let took = clock::now() - before;
        assert!((500_000..5_000_000).contains(&took), "{took} ns");
    }

    #[test]
    fn the_stuck_node_is_first_on_its_thread_only() {
        let plan = Plan::new(2, 1_000_000, 10, Fault::StuckNode, 1, 100);
        assert_eq!(plan.nodes(0).len(), 50);
        assert_eq!(plan.nodes(1).len(), 51);
        // Before its cycle it returns.
        plan.nodes(1)[0].run(99);
        assert!(!plan.nodes(1)[0].is_abandoned());
        plan.nodes(1)[0].abandon();
        assert!(plan.nodes(1)[0].is_abandoned());
    }

    #[test]
    fn no_load_no_nodes() {
        let plan = Plan::new(1, 1_000_000, 0, Fault::None, 0, 0);
        assert!(plan.nodes(0).is_empty());
    }
}
