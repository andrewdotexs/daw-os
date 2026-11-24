//! No system call in the compute phase, enforced (ADR 0021, step
//! P1.6d).
//!
//! The kernel has a mechanism made for this, Syscall User Dispatch. A
//! thread gives the kernel the address of one byte of its own. Before
//! every system call of that thread the kernel reads the byte: 0, the
//! call goes on; 1, the call is not made, and the thread gets a signal
//! (SIGSYS) instead. Writing the byte is an ordinary store: the thread
//! writes 1 when its compute phase begins and 0 when it ends, and pays
//! nothing more as long as nobody calls the kernel.
//!
//! When a call is stopped, the handler of the signal:
//!
//! 1. writes down which call it was, where it came from, and in which
//!    cycle and node the thread was (the engine keeps these up to date
//!    with two more stores);
//! 2. makes the call in place of the code that wanted it, and hands it
//!    the result: the code goes on as if nothing had happened, only
//!    later. A plugin that calls the kernel in its `process()` keeps
//!    working, and is named;
//! 3. returns.
//!
//! Returning from a handler is a system call too (`rt_sigreturn`), made
//! by a few bytes of the C library. The kernel lets one stretch of code
//! through whatever the byte says: that stretch is set to those few
//! bytes, and to nothing else.
//!
//! Some calls cannot be made in place of another piece of code, because
//! what they do depends on where they are made from: those that start a
//! thread or a process, and `rt_sigreturn` itself. They are refused
//! (the code gets the error "no such call") and written down like the
//! others.
//!
//! Everything a handler touches is thread-local, made of atomics, and
//! never allocated or freed: a handler may run between any two
//! instructions of its thread.

use std::io;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};

use crate::sys;

/// What the kernel reads in the byte.
const ALLOW: u8 = 0;
const BLOCK: u8 = 1;

/// Calls written down one by one for each thread: the first ones. All
/// of them are counted.
pub const KEPT: usize = 8;

/// Bytes of code let through at the end of the handler. The code of the
/// C library is nine bytes long (thirteen on a processor with shadow
/// stacks), and the kernel compares the address after the call.
const END_OF_HANDLER_BYTES: usize = 16;

/// System calls that are not made in place of the code that wanted
/// them: `rt_sigreturn`, `clone`, `fork`, `vfork`, `execve`, `clone3`
/// and `execveat` on x86-64.
const REFUSED: [u64; 7] = [15, 56, 57, 58, 59, 435, 322];

/// "No such call": what a refused call returns.
const NO_SUCH_CALL: i64 = -38;

/// What a thread keeps for the kernel and for its handler.
pub struct State {
    /// The byte the kernel reads.
    pub(crate) selector: AtomicU8,
    /// Calls stopped so far.
    caught: AtomicU64,
    /// Where the thread is: the cycle, and the node plus one (0:
    /// outside any node).
    cycle: AtomicU64,
    node: AtomicU32,
    /// The first calls stopped: number, address, cycle, and the node
    /// plus one with bit 32 set when the call was refused.
    kept: [[AtomicU64; 4]; KEPT],
}

impl State {
    const fn new() -> Self {
        Self {
            selector: AtomicU8::new(ALLOW),
            caught: AtomicU64::new(0),
            cycle: AtomicU64::new(0),
            node: AtomicU32::new(0),
            kept: [const { [const { AtomicU64::new(0) }; 4] }; KEPT],
        }
    }
}

thread_local! {
    static STATE: State = const { State::new() };
}

/// Where the code that ends the handler is; 0: no handler yet.
static END_OF_HANDLER: AtomicUsize = AtomicUsize::new(0);

/// A system call that was stopped in the compute phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caught {
    /// The number of the call.
    pub number: u64,
    /// Where the program was: the instruction after the call.
    pub address: u64,
    /// The cycle the thread was in.
    pub cycle: u64,
    /// The node the thread was in; `None`: outside any node.
    pub node: Option<u32>,
    /// The call was made in place of the code that wanted it; `false`:
    /// it was refused.
    pub carried_out: bool,
}

/// The handler of SIGSYS. Runs on the thread that made the call, in
/// place of the call.
extern "C" fn stopped(
    _signal: libc::c_int,
    _details: *mut libc::siginfo_t,
    context: *mut libc::c_void,
) {
    STATE.with(|state| {
        // A SIGSYS that is not ours (somebody sent one): not touched.
        if state.selector.load(Ordering::Relaxed) != BLOCK {
            return;
        }
        // The calls of this handler must go through.
        state.selector.store(ALLOW, Ordering::Relaxed);
        let call = sys::stopped_call(context);
        let refused = REFUSED.contains(&call.number);
        let place = state.caught.fetch_add(1, Ordering::Relaxed) as usize;
        if let Some(kept) = state.kept.get(place) {
            let node = u64::from(state.node.load(Ordering::Relaxed));
            kept[0].store(call.number, Ordering::Relaxed);
            kept[1].store(call.address, Ordering::Relaxed);
            kept[2].store(state.cycle.load(Ordering::Relaxed), Ordering::Relaxed);
            kept[3].store(node | u64::from(refused) << 32, Ordering::Relaxed);
        }
        let result = if refused {
            NO_SUCH_CALL
        } else {
            sys::call_kernel(call.number, &call.arguments)
        };
        sys::answer_stopped_call(context, result);
        state.selector.store(BLOCK, Ordering::Relaxed);
    });
}

/// Set the handler, for the whole process. Before any thread calls
/// [`enable_here`]; calling it again changes nothing.
pub fn install() -> io::Result<()> {
    if END_OF_HANDLER.load(Ordering::Acquire) != 0 {
        return Ok(());
    }
    sys::handle_stopped_calls(stopped)?;
    END_OF_HANDLER.store(sys::end_of_handler()?, Ordering::Release);
    Ok(())
}

/// Have the kernel watch the system calls of the calling thread, and
/// try it once: a call made on purpose must be stopped, written down
/// and carried out. Not in the cycle: it makes system calls.
pub fn enable_here() -> io::Result<()> {
    let end = END_OF_HANDLER.load(Ordering::Acquire);
    if end == 0 {
        return Err(io::Error::other(
            "no handler for the calls that are stopped",
        ));
    }
    sys::stop_calls_by(&STATE, end, END_OF_HANDLER_BYTES)?;
    // The trial: asking the kernel who we are.
    block(u64::MAX);
    let answer = std::process::id();
    allow();
    let first = kept().first().copied();
    let caught = STATE.with(|state| state.caught.swap(0, Ordering::Relaxed));
    STATE.with(|state| {
        for field in &state.kept[0] {
            field.store(0, Ordering::Relaxed);
        }
    });
    // Asked again now that calls go through: the same answer.
    match first {
        Some(call) if caught == 1 && call.carried_out && answer == std::process::id() => Ok(()),
        _ => Err(io::Error::other(format!(
            "a system call made on purpose was not stopped and carried out ({caught} stopped)"
        ))),
    }
}

/// The compute phase of `cycle` begins: from here on a system call of
/// this thread is stopped. One store for the kernel, two for the
/// report.
#[inline]
pub fn block(cycle: u64) {
    STATE.with(|state| {
        state.cycle.store(cycle, Ordering::Relaxed);
        state.node.store(0, Ordering::Relaxed);
        state.selector.store(BLOCK, Ordering::Relaxed);
    });
}

/// The thread enters node `node` (`None`: it is between nodes).
#[inline]
pub fn in_node(node: Option<u32>) {
    STATE.with(|state| {
        state.node.store(
            node.map_or(0, |node| node.saturating_add(1)),
            Ordering::Relaxed,
        );
    });
}

/// The compute phase is over: system calls go through again.
#[inline]
pub fn allow() {
    STATE.with(|state| state.selector.store(ALLOW, Ordering::Relaxed));
}

/// System calls of this thread stopped so far.
#[inline]
#[must_use]
pub fn caught() -> u64 {
    STATE.with(|state| state.caught.load(Ordering::Relaxed))
}

/// The first calls of this thread that were stopped. Allocates: not in
/// the cycle.
#[must_use]
pub fn kept() -> Vec<Caught> {
    STATE.with(|state| {
        let count = (state.caught.load(Ordering::Relaxed) as usize).min(KEPT);
        state.kept[..count]
            .iter()
            .map(|kept| {
                let node = kept[3].load(Ordering::Relaxed);
                Caught {
                    number: kept[0].load(Ordering::Relaxed),
                    address: kept[1].load(Ordering::Relaxed),
                    cycle: kept[2].load(Ordering::Relaxed),
                    node: (node as u32).checked_sub(1),
                    carried_out: node >> 32 == 0,
                }
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `body` on a thread of its own with the kernel watching its
    /// calls. `None`: this kernel cannot (it is older than 5.11, or not
    /// built for it), and the test has nothing to say.
    fn watched<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        std::thread::spawn(move || {
            install().expect("the handler cannot be set");
            match enable_here() {
                Ok(()) => Some(body()),
                Err(error) if error.raw_os_error() == Some(libc::EINVAL) => {
                    eprintln!("this kernel has no Syscall User Dispatch: {error}");
                    None
                }
                Err(error) => panic!("the kernel does not watch the calls: {error}"),
            }
        })
        .join()
        .expect("the thread of the test went wrong")
    }

    #[test]
    fn a_call_in_the_compute_phase_is_stopped_named_and_carried_out() {
        let seen = watched(|| {
            // Nothing stopped by the trial is left behind.
            assert_eq!(caught(), 0);
            assert!(kept().is_empty());
            block(41);
            in_node(Some(3));
            let pid = std::process::id();
            in_node(None);
            std::thread::yield_now();
            allow();
            // Outside the compute phase calls are not stopped.
            std::thread::yield_now();
            (pid, caught(), kept())
        });
        let Some((pid, caught, kept)) = seen else {
            return;
        };
        assert_eq!(pid, std::process::id());
        assert_eq!(caught, 2);
        assert_eq!(kept.len(), 2);
        // getpid in node 3, sched_yield outside any node.
        assert_eq!(
            (kept[0].number, kept[0].cycle, kept[0].node),
            (39, 41, Some(3))
        );
        assert_eq!(
            (kept[1].number, kept[1].cycle, kept[1].node),
            (24, 41, None)
        );
        assert!(kept
            .iter()
            .all(|call| call.carried_out && call.address != 0));
    }

    #[test]
    fn the_result_of_a_call_and_its_error_reach_the_code_that_made_it() {
        let seen = watched(|| {
            block(1);
            // A file that is not there: the error must come back as it
            // would have. And one that is: its contents.
            let missing = std::fs::read("/nonexistent/dawos-dispatch-test");
            let version = std::fs::read_to_string("/proc/version");
            allow();
            (missing.map_err(|error| error.kind()), version, caught())
        });
        let Some((missing, version, caught)) = seen else {
            return;
        };
        assert_eq!(missing, Err(io::ErrorKind::NotFound));
        assert!(version.is_ok_and(|text| text.starts_with("Linux")));
        // An open that fails; an open, the reads and a close.
        assert!(caught >= 4, "{caught} calls stopped");
    }

    #[test]
    fn only_the_first_calls_are_written_down_and_all_are_counted() {
        let seen = watched(|| {
            block(7);
            for _ in 0..KEPT + 5 {
                std::thread::yield_now();
            }
            allow();
            (caught(), kept().len())
        });
        let Some((caught, kept)) = seen else {
            return;
        };
        assert_eq!(caught, (KEPT + 5) as u64);
        assert_eq!(kept, KEPT);
    }

    #[test]
    fn a_thread_that_was_not_enabled_is_left_alone() {
        std::thread::spawn(|| {
            block(1);
            std::thread::yield_now();
            allow();
            assert_eq!(caught(), 0);
        })
        .join()
        .expect("the thread of the test went wrong");
    }

    #[test]
    fn enabling_needs_the_handler() {
        // In a process where `install` was never called this is an
        // error; here another test may have called it, and then the
        // thread is watched and the trial passes, or the kernel cannot.
        if END_OF_HANDLER.load(Ordering::Acquire) == 0 {
            let refused = std::thread::spawn(enable_here).join().expect("thread");
            // Unless another test set the handler in the meantime.
            if END_OF_HANDLER.load(Ordering::Acquire) == 0 {
                assert!(refused.is_err());
            }
        }
    }
}
