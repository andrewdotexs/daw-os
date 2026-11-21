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

/// Keep every page of the process in memory, now and in the future.
pub fn lock_memory() -> io::Result<()> {
    sys::lock_memory()
}

/// The CPU the calling thread is running on, if the kernel tells.
#[must_use]
pub fn current_cpu() -> Option<usize> {
    sys::current_cpu()
}

#[cfg(test)]
mod tests {
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
