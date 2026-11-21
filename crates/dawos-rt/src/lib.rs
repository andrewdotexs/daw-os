//! The real-time core of dawOS.
//!
//! Everything a thread on a live CPU needs and may use: the clock, the
//! placement of a thread on a CPU with a real-time priority, and
//! statistics that never allocate.
//!
//! Rules of this crate (question D-18 and ADR 0021):
//!
//! - nothing here allocates, takes a lock or blocks, except the functions
//!   whose name says so (`sleep_until`, `Gate::wait`);
//! - `unsafe` code lives in the module `sys` only, one call to the C
//!   library per function, each with its reason written next to it;
//! - no type from `std::sync` that can block, no channel.

pub mod clock;
pub mod cpu;
pub mod gate;
pub mod slot;
pub mod stats;
mod sys;
pub mod topology;
