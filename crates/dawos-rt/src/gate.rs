//! A gate: a thread sleeps in front of it until another opens it.
//!
//! Used outside the cycle only (ADR 0021): to park a spare thread until
//! the watchdog needs it. Both operations are system calls.

use std::sync::atomic::{AtomicU32, Ordering};

use crate::sys;

/// Closed until `open` is called, once.
#[derive(Default)]
pub struct Gate {
    state: AtomicU32,
}

const CLOSED: u32 = 0;
const OPEN: u32 = 1;

impl Gate {
    /// A closed gate.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: AtomicU32::new(CLOSED),
        }
    }

    /// Sleep until the gate is open; returns at once if it already is.
    pub fn wait(&self) {
        while self.state.load(Ordering::Acquire) == CLOSED {
            sys::futex_wait(&self.state, CLOSED);
        }
    }

    /// Open the gate and wake whoever sleeps in front of it.
    pub fn open(&self) {
        self.state.store(OPEN, Ordering::Release);
        sys::futex_wake_all(&self.state);
    }

    /// Whether the gate is open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.state.load(Ordering::Acquire) == OPEN
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn an_open_gate_does_not_block() {
        let gate = Gate::new();
        gate.open();
        gate.wait();
        assert!(gate.is_open());
    }

    #[test]
    fn a_sleeper_is_woken() {
        let gate = Arc::new(Gate::new());
        let sleeper = {
            let gate = Arc::clone(&gate);
            thread::spawn(move || gate.wait())
        };
        thread::sleep(Duration::from_millis(20));
        assert!(!sleeper.is_finished());
        gate.open();
        sleeper.join().expect("the sleeper panicked");
    }
}
