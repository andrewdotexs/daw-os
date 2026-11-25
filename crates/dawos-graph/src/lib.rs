//! The audio graph of dawOS: what is computed in a cycle, and by which
//! live thread (step P1.7).
//!
//! A graph is made of nodes (an input of the sound card, a gain, a sum,
//! a delay line, a generator, a load, an output) and of connections
//! between their ports, each carrying one channel of sound. It goes
//! through three forms:
//!
//! 1. a **description** in a text file, one line per node and one per
//!    chain of connections ([`text`]);
//! 2. the **compiled graph** ([`compile`]): checked, put in an order in
//!    which every node comes after what it depends on, with every node
//!    given to one live thread for good (decision D-10: the assignment
//!    is made here, never during a cycle) and every output port given a
//!    buffer;
//! 3. the **running graph** ([`exec`]): the buffers, what each node
//!    keeps from one cycle to the next, and the marks by which a thread
//!    knows that what it waits for is ready.
//!
//! The first two happen outside the real-time path: they read text,
//! allocate, and take the time they take. The third is built there too,
//! and finished before the cycle that first uses it; what the live
//! threads call in it neither allocates nor blocks.

#![forbid(unsafe_code)]

pub mod compile;
pub mod exec;
pub mod kind;
pub mod text;

pub use compile::{compile, Compiled, Settings};
pub use exec::{Inputs, Running};
pub use kind::{Costs, Kind};
pub use text::{parse, Description};

/// What is wrong with a description, and on which line of its text (0:
/// not about one line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// Line of the text, from 1.
    pub line: usize,
    /// What is wrong, in words.
    pub message: String,
}

impl Error {
    pub(crate) fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line == 0 {
            write!(formatter, "{}", self.message)
        } else {
            write!(formatter, "line {}: {}", self.line, self.message)
        }
    }
}

impl std::error::Error for Error {}
