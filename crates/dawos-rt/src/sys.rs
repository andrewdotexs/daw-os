//! The calls to the C library, and through it to the kernel.
//!
//! The only module of the workspace with `unsafe` code. Each function
//! wraps one call, checks its result and hands out plain Rust types.
//! Two of them make no call: they read and write the registers of a
//! thread in the state the kernel hands a signal handler (see
//! `dispatch`).

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

/// Open a device of the kernel for reading, without ever blocking on
/// it. Returns the descriptor.
pub(crate) fn open_device(path: &std::ffi::CStr) -> io::Result<i32> {
    // SAFETY: `path` is a valid C string that outlives the call, and the
    // flags ask for nothing that needs a third argument.
    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd >= 0 {
        Ok(fd)
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Close a descriptor this process opened.
pub(crate) fn close_descriptor(fd: i32) {
    // SAFETY: closing a descriptor has no memory to get wrong; the
    // caller owns `fd` and does not use it again.
    unsafe { libc::close(fd) };
}

/// A request to a device that takes a structure and only reads it.
/// `T` must be the plain structure the kernel defines for `request`.
pub(crate) fn device_request<T>(fd: i32, request: u32, argument: &T) -> io::Result<()> {
    // SAFETY: `argument` is a valid, initialised `T` that outlives the
    // call; the callers of this module pass the structure whose size is
    // written in `request`, and requests that write to it are not made
    // through this function.
    let result =
        unsafe { libc::ioctl(fd, libc::Ioctl::from(request), std::ptr::from_ref(argument)) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// A request to a device that takes nothing.
pub(crate) fn device_request_bare(fd: i32, request: u32) -> io::Result<()> {
    // SAFETY: the request has no argument for the kernel to read or
    // write.
    let result = unsafe { libc::ioctl(fd, libc::Ioctl::from(request)) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// What a wait on one descriptor ended with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Readable {
    /// There is something to read.
    Yes,
    /// The time ran out, or the sleep was cut short by a signal.
    NotYet,
    /// The descriptor is of no use any more.
    Broken,
}

/// Sleep until `fd` has something to read, for `timeout_ms` at the
/// most. A blocking system call: wait phase only.
pub(crate) fn wait_readable(fd: i32, timeout_ms: i32) -> Readable {
    let mut descriptor = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: `descriptor` is one valid pollfd, and the count says one.
    let result = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
    if result <= 0 {
        return Readable::NotYet;
    }
    if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
        Readable::Broken
    } else if descriptor.revents & libc::POLLIN != 0 {
        Readable::Yes
    } else {
        Readable::NotYet
    }
}

/// Read into `buffer` whatever `fd` has, without blocking. The number
/// of bytes, or the error of the kernel.
pub(crate) fn read_some(fd: i32, buffer: &mut [u8]) -> io::Result<usize> {
    // SAFETY: `buffer` is valid for writes of its whole length, which is
    // the length given to the kernel.
    let result = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
    usize::try_from(result).map_err(|_| io::Error::last_os_error())
}

/// What the kernel calls a handler of a signal that wants the details:
/// the signal, what is known about it, and the state of the thread at
/// the instant it was stopped.
pub(crate) type SignalHandler = extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void);

/// Have `handler` called when a thread of this process gets SIGSYS, the
/// signal the kernel sends in place of a system call it was told to
/// stop. For the whole process.
pub(crate) fn handle_stopped_calls(handler: SignalHandler) -> io::Result<()> {
    // SAFETY: an all-zero sigaction is a valid one (no handler, no
    // flags, an empty set of signals to hold back).
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = handler as usize;
    action.sa_flags = libc::SA_SIGINFO;
    // SAFETY: `action` is a valid sigaction that outlives the call; the
    // old one is not asked for. `handler` is a function of this program
    // that lives as long as the process.
    let result = unsafe { libc::sigaction(libc::SIGSYS, &action, std::ptr::null_mut()) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The handler of a signal as the kernel keeps it: not the structure of
/// the C library, which hides the last field that matters here.
#[repr(C)]
struct KernelAction {
    handler: usize,
    flags: u64,
    restorer: usize,
    mask: u64,
}

/// Where the code is that ends the handler of SIGSYS: a few bytes of
/// the C library that make the system call `rt_sigreturn`, and that the
/// library gives the kernel with every handler. Asked of the kernel
/// itself, after [`handle_stopped_calls`].
pub(crate) fn end_of_handler() -> io::Result<usize> {
    let mut action = KernelAction {
        handler: 0,
        flags: 0,
        restorer: 0,
        mask: 0,
    };
    // SAFETY: with no new action the kernel only writes the current one
    // into `action`, which has the layout of its own structure on
    // x86-64 and outlives the call; the last argument is the size of
    // its set of signals, eight bytes.
    let result = unsafe {
        libc::syscall(
            libc::SYS_rt_sigaction,
            libc::SIGSYS,
            std::ptr::null::<KernelAction>(),
            &mut action as *mut KernelAction,
            std::mem::size_of::<u64>(),
        )
    };
    // SA_RESTORER, which the C library sets on x86-64.
    const WITH_RESTORER: u64 = 0x0400_0000;
    if result != 0 {
        Err(io::Error::last_os_error())
    } else if action.flags & WITH_RESTORER == 0 || action.restorer == 0 {
        Err(io::Error::other(
            "the C library gave the kernel no code to end a signal handler with",
        ))
    } else {
        Ok(action.restorer)
    }
}

/// Ask the kernel to look at `selector` before every system call of the
/// calling thread, and to send SIGSYS in place of the call when it
/// holds 1; calls made from the `length` bytes of code at `allowed` are
/// let through whatever it holds (Syscall User Dispatch).
///
/// `selector` must live as long as the thread: the kernel keeps its
/// address.
pub(crate) fn stop_calls_by(
    selector: &'static std::thread::LocalKey<crate::dispatch::State>,
    allowed: usize,
    length: usize,
) -> io::Result<()> {
    /// From `linux/prctl.h`: the C library crate does not have them for
    /// this target.
    const PR_SET_SYSCALL_USER_DISPATCH: libc::c_int = 59;
    const PR_SYS_DISPATCH_ON: libc::c_ulong = 1;
    let address = selector.with(|state| state.selector.as_ptr());
    // SAFETY: `address` is the byte of an atomic in the thread-local
    // storage of the calling thread, which lives until the thread ends,
    // and with the thread ends the setting made here; the kernel only
    // reads it. The other arguments are plain numbers.
    let result = unsafe {
        libc::prctl(
            PR_SET_SYSCALL_USER_DISPATCH,
            PR_SYS_DISPATCH_ON,
            allowed as libc::c_ulong,
            length as libc::c_ulong,
            address,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// A system call the kernel stopped: its number, where the program was
/// (the instruction after the call) and its six arguments.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StoppedCall {
    pub number: u64,
    pub address: u64,
    pub arguments: [u64; 6],
}

/// The system call a handler of SIGSYS was called for, from the state
/// of the thread the kernel hands it. On x86-64 the number is in RAX
/// (the kernel puts it back there before the signal) and the arguments
/// in RDI, RSI, RDX, R10, R8 and R9.
pub(crate) fn stopped_call(context: *mut libc::c_void) -> StoppedCall {
    // SAFETY: the kernel calls a handler set with SA_SIGINFO with a
    // pointer to a valid ucontext_t on the stack of the thread, which
    // stays there until the handler returns; it is only read here.
    let registers = unsafe { (*context.cast::<libc::ucontext_t>()).uc_mcontext.gregs };
    let register = |which: libc::c_int| registers[which as usize] as u64;
    StoppedCall {
        number: register(libc::REG_RAX),
        address: register(libc::REG_RIP),
        arguments: [
            register(libc::REG_RDI),
            register(libc::REG_RSI),
            register(libc::REG_RDX),
            register(libc::REG_R10),
            register(libc::REG_R8),
            register(libc::REG_R9),
        ],
    }
}

/// Make a system call by its number, and return what the kernel
/// returns: the result, or the error as a negative number.
pub(crate) fn call_kernel(number: u64, arguments: &[u64; 6]) -> i64 {
    // SAFETY: this repeats, with the same number and the same six
    // values, a system call the program itself had made and the kernel
    // had stopped: whatever those values point to is the business of
    // the code that made the call, as it would have been.
    let result = unsafe {
        libc::syscall(
            number as libc::c_long,
            arguments[0],
            arguments[1],
            arguments[2],
            arguments[3],
            arguments[4],
            arguments[5],
        )
    };
    if result == -1 {
        // The C library turns an error into -1 and a number apart.
        -i64::from(
            io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(libc::ENOSYS),
        )
    } else {
        result
    }
}

/// Give the program `result` as what its stopped system call returned:
/// it finds it in RAX when the handler ends.
pub(crate) fn answer_stopped_call(context: *mut libc::c_void, result: i64) {
    // SAFETY: as in `stopped_call`, `context` is the valid ucontext_t
    // the kernel gave the handler; the kernel puts its registers back
    // into the thread when the handler returns, and RAX is where a
    // system call leaves its result.
    unsafe {
        (*context.cast::<libc::ucontext_t>()).uc_mcontext.gregs[libc::REG_RAX as usize] = result;
    }
}
