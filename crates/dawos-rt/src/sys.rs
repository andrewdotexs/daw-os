//! The calls to the C library, and through it to the kernel.
//!
//! The only module of the workspace with `unsafe` code. Each function
//! wraps one call, checks its result and hands out plain Rust types.

use std::io;
use std::sync::atomic::AtomicU32;

/// Nanoseconds of the monotonic clock: the clock that never jumps and
/// that the kernel uses for timers. On x86-64 this does not enter the
/// kernel (it goes through the vDSO), so it is allowed in the compute
/// phase of the cycle (ADR 0021).
pub(crate) fn monotonic_ns() -> u64 {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a valid, writable timespec, and CLOCK_MONOTONIC
    // exists on every Linux this code runs on; the call cannot fail with
    // these arguments.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) };
    // The monotonic clock starts at boot: both fields are non-negative.
    (now.tv_sec as u64) * 1_000_000_000 + now.tv_nsec as u64
}

/// Sleep until the monotonic clock reaches `deadline_ns`. Returns at
/// once if that instant has already passed. This is a blocking system
/// call: it belongs to the wait phase of the cycle.
pub(crate) fn sleep_until_ns(deadline_ns: u64) {
    let deadline = libc::timespec {
        tv_sec: (deadline_ns / 1_000_000_000) as libc::time_t,
        tv_nsec: (deadline_ns % 1_000_000_000) as libc::c_long,
    };
    loop {
        // SAFETY: `deadline` is a valid timespec that outlives the call;
        // with TIMER_ABSTIME the last argument is not written and may be
        // null.
        let result = unsafe {
            libc::clock_nanosleep(
                libc::CLOCK_MONOTONIC,
                libc::TIMER_ABSTIME,
                &deadline,
                std::ptr::null_mut(),
            )
        };
        // A signal interrupts the sleep: the deadline is absolute, so
        // sleeping again loses nothing.
        if result != libc::EINTR {
            return;
        }
    }
}

/// Bind the calling thread to one CPU.
pub(crate) fn pin_current_thread(cpu: usize) -> io::Result<()> {
    if cpu >= libc::CPU_SETSIZE as usize {
        return Err(io::Error::from_raw_os_error(libc::EINVAL));
    }
    // SAFETY: an all-zero cpu_set_t is a valid, empty set.
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    // SAFETY: `set` is a valid cpu_set_t and `cpu` is below CPU_SETSIZE,
    // checked above.
    unsafe { libc::CPU_SET(cpu, &mut set) };
    // SAFETY: pid 0 means the calling thread; `set` is valid for the
    // size passed.
    let result =
        unsafe { libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Give the calling thread the real-time policy SCHED_FIFO at `priority`
/// (1 to 99).
pub(crate) fn set_fifo_priority(priority: u8) -> io::Result<()> {
    let param = libc::sched_param {
        sched_priority: libc::c_int::from(priority),
    };
    // SAFETY: pid 0 means the calling thread; `param` is a valid
    // sched_param that outlives the call.
    let result = unsafe { libc::sched_setscheduler(0, libc::SCHED_FIFO, &param) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Keep every page of the process in memory, now and in the future: a
/// page fault on a live thread is an entry into the kernel.
pub(crate) fn lock_memory() -> io::Result<()> {
    // SAFETY: mlockall takes flags only and touches no memory of ours.
    let result = unsafe { libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The CPU the calling thread is running on.
pub(crate) fn current_cpu() -> Option<usize> {
    // SAFETY: sched_getcpu takes no argument and only returns a number.
    let cpu = unsafe { libc::sched_getcpu() };
    usize::try_from(cpu).ok()
}

/// The kernel's number for the calling thread.
pub(crate) fn thread_id() -> i32 {
    // SAFETY: gettid takes no argument and cannot fail.
    unsafe { libc::gettid() }
}

/// Scheduling policy for `set_policy`.
pub(crate) enum RawPolicy {
    Fifo(u8),
    Other,
    Idle,
}

/// Give thread `tid` (0: the calling thread) a scheduling policy.
pub(crate) fn set_policy(tid: i32, policy: &RawPolicy) -> io::Result<()> {
    let (policy, priority) = match policy {
        RawPolicy::Fifo(priority) => (libc::SCHED_FIFO, libc::c_int::from(*priority)),
        RawPolicy::Other => (libc::SCHED_OTHER, 0),
        RawPolicy::Idle => (libc::SCHED_IDLE, 0),
    };
    let param = libc::sched_param {
        sched_priority: priority,
    };
    // SAFETY: `param` is a valid sched_param that outlives the call; the
    // kernel checks `tid` and the policy and reports an error for a
    // thread that does not exist.
    let result = unsafe { libc::sched_setscheduler(tid, policy, &param) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Bind thread `tid` (0: the calling thread) to the CPUs of `cpus`.
pub(crate) fn set_affinity(tid: i32, cpus: &[usize]) -> io::Result<()> {
    // SAFETY: an all-zero cpu_set_t is a valid, empty set.
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    for &cpu in cpus {
        if cpu >= libc::CPU_SETSIZE as usize {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        // SAFETY: `set` is a valid cpu_set_t and `cpu` is below
        // CPU_SETSIZE, checked above.
        unsafe { libc::CPU_SET(cpu, &mut set) };
    }
    // SAFETY: `set` is valid for the size passed; the kernel checks
    // `tid`.
    let result =
        unsafe { libc::sched_setaffinity(tid, std::mem::size_of::<libc::cpu_set_t>(), &set) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Sleep as long as `word` holds `expected`, until another thread calls
/// `futex_wake` on it. May return early: callers check the value again.
pub(crate) fn futex_wait(word: &AtomicU32, expected: u32) {
    // SAFETY: `word` is a valid, aligned 32-bit value that lives for the
    // whole call; FUTEX_WAIT only reads it. The timeout is null (wait
    // without limit) and the last two arguments are unused by this
    // operation.
    unsafe {
        libc::syscall(
            libc::SYS_futex,
            word.as_ptr(),
            libc::FUTEX_WAIT | libc::FUTEX_PRIVATE_FLAG,
            expected,
            std::ptr::null::<libc::timespec>(),
            std::ptr::null::<u32>(),
            0u32,
        );
    }
}

/// Wake every thread sleeping in `futex_wait` on `word`.
pub(crate) fn futex_wake_all(word: &AtomicU32) {
    // SAFETY: `word` is a valid, aligned 32-bit value; FUTEX_WAKE does
    // not access it, only uses its address as a key. The remaining
    // arguments are unused by this operation.
    unsafe {
        libc::syscall(
            libc::SYS_futex,
            word.as_ptr(),
            libc::FUTEX_WAKE | libc::FUTEX_PRIVATE_FLAG,
            i32::MAX,
            std::ptr::null::<libc::timespec>(),
            std::ptr::null::<u32>(),
            0u32,
        );
    }
}
