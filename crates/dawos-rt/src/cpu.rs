//! Where a thread runs, and with which priority.

use std::fmt;
use std::io;

use crate::sys;

/// A list of CPUs could not be read.
#[derive(Debug, PartialEq, Eq)]
pub struct CpuListError(String);

impl fmt::Display for CpuListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "not a CPU list: '{}' (examples: 4, 4-7, 0-3,8-15)",
            self.0
        )
    }
}

impl std::error::Error for CpuListError {}

/// Read a CPU list in the format of the kernel: "4-7", "0-3,8-15".
/// The result is sorted and has each CPU once. Allocates: for start-up
/// code, never for a live path.
pub fn parse_cpu_list(text: &str) -> Result<Vec<usize>, CpuListError> {
    let error = || CpuListError(text.to_string());
    let mut cpus = Vec::new();
    for part in text.trim().split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(error());
        }
        let (first, last) = match part.split_once('-') {
            Some((first, last)) => (first, last),
            None => (part, part),
        };
        let first: usize = first.trim().parse().map_err(|_| error())?;
        let last: usize = last.trim().parse().map_err(|_| error())?;
        if last < first || last >= 4096 {
            return Err(error());
        }
        cpus.extend(first..=last);
    }
    cpus.sort_unstable();
    cpus.dedup();
    Ok(cpus)
}

/// Bind the calling thread to one CPU.
pub fn pin_current_thread(cpu: usize) -> io::Result<()> {
    sys::pin_current_thread(cpu)
}

/// Give the calling thread the real-time policy SCHED_FIFO.
pub fn set_fifo_priority(priority: u8) -> io::Result<()> {
    sys::set_fifo_priority(priority)
}

/// The kernel's number for the calling thread: what another thread needs
/// to change its priority or its CPUs.
#[must_use]
pub fn thread_id() -> i32 {
    sys::thread_id()
}

/// How the kernel schedules a thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Real time, first in first out, at a priority from 1 to 99.
    Fifo(u8),
    /// The normal policy.
    Other,
    /// Runs only when nothing else wants the CPU.
    Idle,
}

/// Give thread `tid` a scheduling policy. Meant to be called from another
/// thread (the watchdog): the kernel interrupts the CPU of `tid` to apply
/// it.
pub fn set_policy(tid: i32, policy: Policy) -> io::Result<()> {
    let raw = match policy {
        Policy::Fifo(priority) => sys::RawPolicy::Fifo(priority),
        Policy::Other => sys::RawPolicy::Other,
        Policy::Idle => sys::RawPolicy::Idle,
    };
    sys::set_policy(tid, &raw)
}

/// Bind thread `tid` to the CPUs of `cpus`; `tid` 0 is the calling
/// thread.
pub fn set_affinity(tid: i32, cpus: &[usize]) -> io::Result<()> {
    sys::set_affinity(tid, cpus)
}

/// Keep every page of the process in memory, now and in the future.
pub fn lock_memory() -> io::Result<()> {
    sys::lock_memory()
}

/// The CPU the calling thread is running on, if the kernel tells.
#[must_use]
pub fn current_cpu() -> Option<usize> {
    sys::current_cpu()
}

/// Have the calling thread take the numbers too small for the normal
/// form of a float (denormals) for zero, as operands and as results, or
/// stop doing so; returns whether it did before.
///
/// A filter fed with silence decays through those numbers, and a
/// processor computes with them tens of times more slowly than with the
/// others: an audio thread sets this once, when it starts (FTZ and DAZ
/// of the vector unit). It is a setting of the thread, which a new
/// thread takes from the one that makes it. No system call.
pub fn flush_denormals(on: bool) -> bool {
    sys::flush_denormals(on)
}

#[cfg(test)]
mod tests {
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn denormals_are_flushed_on_the_thread_that_asks() {
        // The smallest number in the normal form, halved: with the
        // setting off the result is a denormal, with it on a zero.
        let halve = |value: f32| std::hint::black_box(value) * std::hint::black_box(0.5);
        let before = super::flush_denormals(false);
        assert!(!before, "a new thread starts with the setting off");
        assert!(halve(f32::MIN_POSITIVE) > 0.0);
        assert!(!super::flush_denormals(true));
        assert_eq!(halve(f32::MIN_POSITIVE), 0.0);
        // And a denormal operand counts as zero.
        let denormal = f32::from_bits(1);
        assert_eq!(
            std::hint::black_box(denormal) * std::hint::black_box(1.0e30),
            0.0
        );
        // A thread starts with the setting of the thread that made it,
        // and from then on has its own.
        std::thread::spawn(move || {
            assert_eq!(halve(f32::MIN_POSITIVE), 0.0);
            assert!(super::flush_denormals(false));
            assert!(halve(f32::MIN_POSITIVE) > 0.0);
        })
        .join()
        .unwrap();
        assert_eq!(halve(f32::MIN_POSITIVE), 0.0);
        assert!(super::flush_denormals(false));
        assert!(halve(f32::MIN_POSITIVE) > 0.0);
    }

    use super::*;

    #[test]
    fn lists_of_the_kernel_are_read() {
        assert_eq!(parse_cpu_list("4-7"), Ok(vec![4, 5, 6, 7]));
        assert_eq!(parse_cpu_list("0-3,8-9\n"), Ok(vec![0, 1, 2, 3, 8, 9]));
        assert_eq!(parse_cpu_list(" 5 "), Ok(vec![5]));
        assert_eq!(parse_cpu_list("7,4-5,5"), Ok(vec![4, 5, 7]));
    }

    #[test]
    fn what_is_not_a_list_is_refused() {
        for text in ["", "a", "4-", "-4", "7-4", "1,,2", "1;2", "99999"] {
            assert!(parse_cpu_list(text).is_err(), "'{text}' was accepted");
        }
    }

    #[test]
    fn the_current_cpu_is_known() {
        assert!(current_cpu().is_some());
    }

    #[test]
    fn a_cpu_that_cannot_exist_is_refused() {
        assert!(pin_current_thread(1_000_000).is_err());
    }
}
