//! What a live thread tells the watchdog, in memory.
//!
//! One slot per live CPU. The thread that holds the slot writes to it at
//! the two borders of the compute phase; the watchdog, on another CPU,
//! reads it once per period. Plain stores and loads: no system call, no
//! lock (ADR 0021, ADR 0023).

use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};

/// The state of one live CPU's thread. Aligned to a cache line of its
/// own, so that the threads of two CPUs never write to the same line.
#[repr(align(64))]
#[derive(Default)]
pub struct Slot {
    /// Raised by the watchdog when it replaces the thread: a thread whose
    /// generation is no longer the one of its slot has been retired.
    generation: AtomicU32,
    /// The kernel's number of the thread that holds the slot.
    tid: AtomicI32,
    /// Number of the cycle in progress or last completed.
    cycle: AtomicU64,
    /// When the compute phase in progress began; 0 while waiting.
    compute_since: AtomicU64,
    /// The node being run, plus one; 0 outside any node.
    node: AtomicU32,
}

impl Slot {
    /// An empty slot.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            generation: AtomicU32::new(0),
            tid: AtomicI32::new(0),
            cycle: AtomicU64::new(0),
            compute_since: AtomicU64::new(0),
            node: AtomicU32::new(0),
        }
    }

    /// The generation the slot is at.
    #[inline]
    pub fn generation(&self) -> u32 {
        self.generation.load(Ordering::Acquire)
    }

    /// Retire the thread that holds the slot: from now on its generation
    /// is stale. Returns the new generation, for the thread that takes
    /// over. Called by the watchdog.
    pub fn retire_holder(&self) -> u32 {
        // The holder is no longer computing as far as the slot goes.
        self.compute_since.store(0, Ordering::Release);
        self.node.store(0, Ordering::Release);
        self.generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// A thread takes the slot.
    pub fn take(&self, tid: i32) {
        self.tid.store(tid, Ordering::Release);
    }

    /// The thread that holds the slot.
    pub fn holder(&self) -> i32 {
        self.tid.load(Ordering::Acquire)
    }

    /// The holder enters the compute phase of cycle `cycle` at `now`.
    #[inline]
    pub fn enter_compute(&self, cycle: u64, now: u64) {
        self.cycle.store(cycle, Ordering::Relaxed);
        // 0 means "waiting": an instant of 0 cannot happen after boot,
        // but make sure.
        self.compute_since.store(now.max(1), Ordering::Release);
    }

    /// The holder starts node number `index`.
    #[inline]
    pub fn enter_node(&self, index: usize) {
        self.node.store(index as u32 + 1, Ordering::Release);
    }

    /// The holder is between nodes.
    #[inline]
    pub fn leave_node(&self) {
        self.node.store(0, Ordering::Release);
    }

    /// The holder enters the wait phase.
    #[inline]
    pub fn enter_wait(&self) {
        self.compute_since.store(0, Ordering::Release);
    }

    /// When the compute phase in progress began; `None` while waiting.
    pub fn computing_since(&self) -> Option<u64> {
        match self.compute_since.load(Ordering::Acquire) {
            0 => None,
            since => Some(since),
        }
    }

    /// The cycle in progress or last completed.
    pub fn cycle(&self) -> u64 {
        self.cycle.load(Ordering::Relaxed)
    }

    /// The node being run; `None` outside any node.
    pub fn node(&self) -> Option<usize> {
        match self.node.load(Ordering::Acquire) {
            0 => None,
            index => Some(index as usize - 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_follows_its_thread() {
        let slot = Slot::new();
        assert_eq!(slot.computing_since(), None);
        slot.take(42);
        slot.enter_compute(7, 1_000);
        assert_eq!(slot.holder(), 42);
        assert_eq!(slot.cycle(), 7);
        assert_eq!(slot.computing_since(), Some(1_000));
        assert_eq!(slot.node(), None);
        slot.enter_node(3);
        assert_eq!(slot.node(), Some(3));
        slot.leave_node();
        slot.enter_wait();
        assert_eq!(slot.computing_since(), None);
    }

    #[test]
    fn retiring_the_holder_changes_the_generation() {
        let slot = Slot::new();
        let before = slot.generation();
        slot.enter_compute(1, 500);
        slot.enter_node(0);
        let after = slot.retire_holder();
        assert_eq!(after, before + 1);
        assert_eq!(slot.generation(), after);
        assert_eq!(slot.computing_since(), None);
        assert_eq!(slot.node(), None);
    }

    #[test]
    fn a_slot_has_a_cache_line_of_its_own() {
        assert_eq!(std::mem::align_of::<Slot>(), 64);
    }
}
