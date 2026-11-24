//! What enters the kernel on the live CPUs, read from the trace of the
//! kernel itself (step P1.6c, ADR 0021).
//!
//! The kernel can write a record every time something happens on a CPU:
//! a system call, an interrupt, a switch from one thread to another. It
//! keeps them in a ring for each CPU and serves them under
//! `/sys/kernel/tracing`. Writing a record costs the CPU it happens on
//! some hundred nanoseconds, and only when something happens, which on
//! a live CPU is the very thing to be counted; reading them is the work
//! of a thread outside the live CPUs.
//!
//! The records are read as the kernel keeps them, in binary
//! (`per_cpu/cpuN/trace_pipe_raw`), and not as the text it can make of
//! them: the text gives the instants to the microsecond, the records to
//! the nanosecond, and the wake-up of a thread is a matter of a few
//! microseconds. They are bytes read from a file with an ordinary
//! `read()`: there is no `unsafe` code here.
//!
//! The format, as the kernel describes it in `events/header_page` and
//! `events/header_event` and in `include/linux/ring_buffer.h`:
//!
//! - a page of 4096 bytes begins with the instant of its first record
//!   (8 bytes) and the length of what follows (8 bytes, with two flags
//!   in bits 30 and 31 for records that were lost);
//! - every record begins with 4 bytes: a kind in the low 5 bits and, in
//!   the other 27, the nanoseconds since the record before;
//! - kinds 1 to 28 are a record of so many words of 4 bytes; kind 0 has
//!   its length in the next 4 bytes; 29 is padding, 30 carries a longer
//!   stretch of time, 31 an instant in full;
//! - a record begins with the number of its event (2 bytes), two flags
//!   and the thread it happened in (4 bytes); where the other fields
//!   are is told, event by event, by `events/GROUP/NAME/format`, which
//!   is read here instead of being assumed.
//!
//! A trace of its own (an "instance") is made for the engine and taken
//! away at the end: whatever else is tracing on the machine is left
//! alone.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

/// Where the kernel serves its traces.
const TRACEFS: &str = "/sys/kernel/tracing";

/// The name of the trace of the engine.
const INSTANCE: &str = "dawos-engine";

/// Kibibytes of ring for each CPU: some seconds of the busiest run (a
/// dozen records a cycle, five hundred cycles a second), and the ring
/// is emptied fifty times a second.
const RING_KB: u32 = 2048;

/// Pages read from one CPU in one round, at the most: the reader has
/// the other CPUs to see to.
const PAGES_A_ROUND: usize = 512;

/// The largest page the kernel can be set to give.
const LARGEST_PAGE: usize = 65_536;

/// What a record of the trace says happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum What {
    /// A thread entered the kernel with a system call (`a`: its
    /// number).
    SyscallEnter,
    /// It came back (`a`: the number; `b`: what the call returned).
    SyscallExit,
    /// An interrupt of the processor itself, by its name here (`a`: its
    /// vector).
    Vector(&'static str),
    /// An interrupt of a device (`a`: its number).
    DeviceIrq,
    /// Deferred work of an interrupt (`a`: which kind).
    Softirq,
    /// The CPU went from one thread to another (`a`: the thread it
    /// left; `b`: the thread it took, whose name is in `comm`; `c`: the
    /// state the first was left in, 0 when it could have gone on).
    Switch,
    /// A page of memory was not there (`a`: the address; `b`: where the
    /// program was).
    PageFault,
    /// A timer of the kernel ran out (`a`: the function it calls).
    Timer,
    /// A piece of deferred work began (`a`: its function).
    Work,
    /// An interrupt that cannot be masked (`a`: its handler).
    Nmi,
    /// A thread found a lock of the kernel taken and began to wait for
    /// it (`a`: the address of the lock; `b`: the kind of lock and of
    /// wait, as the kernel flags them).
    LockWait,
    /// The wait ended (`a`: the address of the lock).
    LockGot,
    /// A thread is being woken by what runs on this CPU (`a`: the
    /// thread, whose name is in `comm`; `b`: the CPU it will run on).
    Waking,
    /// A function another CPU asked this one to run begins (`a`: the
    /// function): what a function call interrupt was sent for.
    Call,
}

/// One record of the trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub cpu: usize,
    /// Nanoseconds of the monotonic clock: the clock of the engine.
    pub time_ns: u64,
    /// The thread it happened in (0: the idle thread of the CPU).
    pub pid: i32,
    pub what: What,
    pub a: u64,
    pub b: u64,
    pub c: u64,
    /// A name, where the event has one (see [`What::Switch`]).
    pub comm: [u8; 16],
}

/// Where a field of an event is in its record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Field {
    offset: usize,
    size: usize,
}

/// How to read one event.
#[derive(Clone, Copy, Debug)]
struct Decoder {
    what: What,
    a: Option<Field>,
    b: Option<Field>,
    c: Option<Field>,
    comm: Option<Field>,
}

/// An event to ask the kernel for: its group and name, what it means
/// here, and the fields that go into `a`, `b`, `c` and `comm`.
struct Wanted {
    group: &'static str,
    name: &'static str,
    what: What,
    fields: [&'static str; 4],
}

const fn wanted(
    group: &'static str,
    name: &'static str,
    what: What,
    fields: [&'static str; 4],
) -> Wanted {
    Wanted {
        group,
        name,
        what,
        fields,
    }
}

const fn vector(name: &'static str, words: &'static str) -> Wanted {
    wanted(
        "irq_vectors",
        name,
        What::Vector(words),
        ["vector", "", "", ""],
    )
}

/// Everything asked for. An event this kernel does not have is left
/// out and named in the report.
const WANTED: [Wanted; 25] = [
    wanted(
        "raw_syscalls",
        "sys_enter",
        What::SyscallEnter,
        ["id", "", "", ""],
    ),
    wanted(
        "raw_syscalls",
        "sys_exit",
        What::SyscallExit,
        ["id", "ret", "", ""],
    ),
    vector("local_timer_entry", "local timer interrupt"),
    vector("reschedule_entry", "reschedule interrupt"),
    vector("call_function_entry", "function call interrupt"),
    vector(
        "call_function_single_entry",
        "function call interrupt (one CPU)",
    ),
    vector("irq_work_entry", "irq work interrupt"),
    vector("x86_platform_ipi_entry", "platform interrupt"),
    vector("thermal_apic_entry", "thermal interrupt"),
    vector("threshold_apic_entry", "error threshold interrupt"),
    vector("deferred_error_apic_entry", "deferred error interrupt"),
    vector("error_apic_entry", "interrupt controller error"),
    vector("spurious_apic_entry", "spurious interrupt"),
    wanted(
        "irq",
        "irq_handler_entry",
        What::DeviceIrq,
        ["irq", "", "", ""],
    ),
    wanted("irq", "softirq_entry", What::Softirq, ["vec", "", "", ""]),
    wanted(
        "sched",
        "sched_switch",
        What::Switch,
        ["prev_pid", "next_pid", "prev_state", "next_comm"],
    ),
    wanted(
        "exceptions",
        "page_fault_user",
        What::PageFault,
        ["address", "ip", "", ""],
    ),
    wanted(
        "exceptions",
        "page_fault_kernel",
        What::PageFault,
        ["address", "ip", "", ""],
    ),
    wanted(
        "timer",
        "hrtimer_expire_entry",
        What::Timer,
        ["function", "", "", ""],
    ),
    wanted(
        "workqueue",
        "workqueue_execute_start",
        What::Work,
        ["function", "", "", ""],
    ),
    wanted("nmi", "nmi_handler", What::Nmi, ["handler", "", "", ""]),
    wanted(
        "lock",
        "contention_begin",
        What::LockWait,
        ["lock_addr", "flags", "", ""],
    ),
    wanted(
        "csd",
        "csd_function_entry",
        What::Call,
        ["func", "", "", ""],
    ),
    wanted(
        "sched",
        "sched_waking",
        What::Waking,
        ["pid", "target_cpu", "", "comm"],
    ),
    wanted(
        "lock",
        "contention_end",
        What::LockGot,
        ["lock_addr", "", "", ""],
    ),
];

/// The fields of an event, from the text of its `format` file: name to
/// place. A line of it reads
/// `field:pid_t prev_pid;  offset:24;  size:4;  signed:1;`.
fn fields_of(format: &str) -> HashMap<String, Field> {
    let mut fields = HashMap::new();
    for line in format.lines() {
        let mut name = None;
        let mut offset = None;
        let mut size = None;
        for part in line.split(';') {
            let part = part.trim();
            if let Some(declaration) = part.strip_prefix("field:") {
                // The name is the last word, without the length of an
                // array: "char prev_comm[16]".
                name = declaration
                    .split_whitespace()
                    .last()
                    .map(|word| word.split('[').next().unwrap_or(word).to_string());
            } else if let Some(value) = part.strip_prefix("offset:") {
                offset = value.parse().ok();
            } else if let Some(value) = part.strip_prefix("size:") {
                size = value.parse().ok();
            }
        }
        if let (Some(name), Some(offset), Some(size)) = (name, offset, size) {
            fields.insert(name, Field { offset, size });
        }
    }
    fields
}

/// A number of 1 to 8 bytes at `field` of `record`, least significant
/// byte first; 0 when the record is too short.
fn number(record: &[u8], field: Option<Field>) -> u64 {
    let Some(field) = field else {
        return 0;
    };
    let Some(bytes) = record.get(field.offset..field.offset + field.size.min(8)) else {
        return 0;
    };
    let mut value = [0_u8; 8];
    value[..bytes.len()].copy_from_slice(bytes);
    u64::from_le_bytes(value)
}

fn word(page: &[u8], at: usize) -> Option<u32> {
    let bytes = page.get(at..at + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// What a page held.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageSummary {
    /// Records given to the caller.
    pub records: u64,
    /// The kernel says records were lost before this page: the reader
    /// was too slow, or the ring too small.
    pub lost: bool,
    /// The page did not read as the format says, from some point on.
    pub malformed: bool,
}

/// Go through one page of the trace: `each` gets the instant of every
/// record, in nanoseconds, and the record itself. Never trusts the
/// lengths the page claims.
pub fn read_page(page: &[u8], mut each: impl FnMut(u64, &[u8])) -> PageSummary {
    const HEADER: usize = 16;
    let mut summary = PageSummary::default();
    let (Some(low), Some(high), Some(commit)) = (word(page, 0), word(page, 4), word(page, 8))
    else {
        summary.malformed = true;
        return summary;
    };
    let mut time = u64::from(low) | u64::from(high) << 32;
    summary.lost = commit & (1 << 31) != 0;
    let length = (commit & 0x3fff_ffff) as usize;
    let Some(data) = page.get(HEADER..HEADER + length) else {
        summary.malformed = true;
        return summary;
    };
    let mut at = 0;
    while at < data.len() {
        let Some(head) = word(data, at) else {
            summary.malformed = true;
            break;
        };
        let kind = head & 0x1f;
        let delta = u64::from(head >> 5);
        // The word after the head, for the kinds that have one.
        let next = word(data, at + 4);
        let step = match kind {
            // A longer stretch of time than 27 bits hold.
            30 => {
                let Some(more) = next else {
                    summary.malformed = true;
                    break;
                };
                time += delta + (u64::from(more) << 27);
                8
            }
            // An instant in full.
            31 => {
                let Some(more) = next else {
                    summary.malformed = true;
                    break;
                };
                time = delta + (u64::from(more) << 27);
                8
            }
            // Padding: to the end of the page, or of a given length.
            29 => {
                if delta == 0 {
                    break;
                }
                match next {
                    Some(length) => 4 + length as usize,
                    None => {
                        summary.malformed = true;
                        break;
                    }
                }
            }
            // A record whose length is in the next word (that word
            // counted in).
            0 => {
                let Some(length) = next else {
                    summary.malformed = true;
                    break;
                };
                let length = length as usize;
                time += delta;
                match data.get(at + 8..at + 4 + length) {
                    Some(record) if length >= 4 => {
                        each(time, record);
                        summary.records += 1;
                    }
                    _ => {
                        summary.malformed = true;
                        break;
                    }
                }
                4 + length
            }
            // A record of `kind` words.
            words => {
                let length = words as usize * 4;
                time += delta;
                match data.get(at + 4..at + 4 + length) {
                    Some(record) => {
                        each(time, record);
                        summary.records += 1;
                    }
                    None => {
                        summary.malformed = true;
                        break;
                    }
                }
                4 + length
            }
        };
        at += step;
    }
    summary
}

/// The events of the kernel this module understands, by their number.
#[derive(Default)]
pub struct Catalog {
    decoders: HashMap<u16, Decoder>,
}

impl Catalog {
    /// Add an event: its number, what it means, and the text of its
    /// `format` file. `false`: a field it needs is not there.
    fn add(&mut self, id: u16, what: What, wanted: &[&str; 4], format: &str) -> bool {
        let fields = fields_of(format);
        let mut places = [None; 4];
        for (place, name) in places.iter_mut().zip(wanted) {
            if !name.is_empty() {
                match fields.get(*name) {
                    Some(field) => *place = Some(*field),
                    None => return false,
                }
            }
        }
        self.decoders.insert(
            id,
            Decoder {
                what,
                a: places[0],
                b: places[1],
                c: places[2],
                comm: places[3],
            },
        );
        true
    }

    /// The event a record carries, if it is one of those asked for.
    #[must_use]
    pub fn decode(&self, cpu: usize, time_ns: u64, record: &[u8]) -> Option<Event> {
        // Every record begins with the number of its event (2 bytes),
        // two bytes of flags and the thread (4 bytes).
        let id = u16::from_le_bytes([*record.first()?, *record.get(1)?]);
        let decoder = self.decoders.get(&id)?;
        let pid = number(record, Some(Field { offset: 4, size: 4 })) as u32 as i32;
        let mut comm = [0_u8; 16];
        if let Some(field) = decoder.comm {
            if let Some(bytes) = record.get(field.offset..field.offset + field.size.min(16)) {
                comm[..bytes.len()].copy_from_slice(bytes);
            }
        }
        Some(Event {
            cpu,
            time_ns,
            pid,
            what: decoder.what,
            a: number(record, decoder.a),
            b: number(record, decoder.b),
            c: number(record, decoder.c),
            comm,
        })
    }
}

/// The CPUs `cpus` as the kernel wants a set of them written: in
/// hexadecimal, 32 bits at a time from the highest, with commas.
fn cpu_mask(cpus: &[usize]) -> String {
    let words = cpus.iter().max().map_or(1, |highest| highest / 32 + 1);
    let mut mask = vec![0_u32; words];
    for &cpu in cpus {
        mask[cpu / 32] |= 1 << (cpu % 32);
    }
    let text: Vec<String> = mask.iter().rev().map(|word| format!("{word:x}")).collect();
    text.join(",")
}

fn write(path: &Path, value: &str) -> Result<(), String> {
    fs::write(path, value).map_err(|error| format!("{}: {error}", path.display()))
}

/// A trace of the kernel on some CPUs, being read.
pub struct Tracer {
    instance: PathBuf,
    catalog: Catalog,
    pipes: Vec<(usize, File)>,
    page: Vec<u8>,
    /// Events of the list this kernel does not have.
    pub missing: Vec<String>,
    /// Records read, of every kind, and those of them given on.
    pub records: u64,
    pub events: u64,
    /// Pages that came with records lost before them, and pages that
    /// did not read as the format says.
    pub lost_pages: u64,
    pub malformed_pages: u64,
}

impl Tracer {
    /// Start tracing on `cpus`. Needs root. Many small writes of files:
    /// before the cycle starts.
    pub fn start(cpus: &[usize]) -> Result<Self, String> {
        if cpus.is_empty() {
            return Err("no CPU to trace: the live CPUs are not known".to_string());
        }
        let tracefs = Path::new(TRACEFS);
        if !tracefs.join("instances").is_dir() {
            return Err(format!(
                "{TRACEFS}/instances is not there: is the trace file system mounted?"
            ));
        }
        let instance = tracefs.join("instances").join(INSTANCE);
        // One left by a run that ended badly is taken away first.
        if instance.is_dir() {
            let _ = fs::remove_dir(&instance);
        }
        if !instance.is_dir() {
            fs::create_dir(&instance)
                .map_err(|error| format!("{}: {error}", instance.display()))?;
        }
        let mut tracer = Self {
            instance,
            catalog: Catalog::default(),
            pipes: Vec::new(),
            page: Vec::new(),
            missing: Vec::new(),
            records: 0,
            events: 0,
            lost_pages: 0,
            malformed_pages: 0,
        };
        // From here on an error drops the tracer, which takes the
        // instance away again.
        let instance = tracer.instance.clone();
        write(&instance.join("tracing_on"), "0")?;
        // The clock of the engine, so that instants compare.
        write(&instance.join("trace_clock"), "mono")?;
        write(&instance.join("tracing_cpumask"), &cpu_mask(cpus))?;
        write(&instance.join("buffer_size_kb"), &RING_KB.to_string())?;
        for event in &WANTED {
            let directory = instance.join("events").join(event.group).join(event.name);
            let added = fs::read_to_string(directory.join("id"))
                .ok()
                .and_then(|id| id.trim().parse::<u16>().ok())
                .zip(fs::read_to_string(directory.join("format")).ok())
                .is_some_and(|(id, format)| {
                    tracer.catalog.add(id, event.what, &event.fields, &format)
                });
            if added && write(&directory.join("enable"), "1").is_ok() {
                continue;
            }
            tracer
                .missing
                .push(format!("{}:{}", event.group, event.name));
        }
        if tracer.catalog.decoders.is_empty() {
            return Err("the kernel has none of the events asked for".to_string());
        }
        // How large a page the kernel gives, in kibibytes; 4 unless set
        // otherwise.
        let page_kb = fs::read_to_string(instance.join("buffer_subbuf_size_kb"))
            .ok()
            .and_then(|size| size.trim().parse::<usize>().ok())
            .unwrap_or(4);
        tracer.page = vec![0; (page_kb * 1024).clamp(4096, LARGEST_PAGE)];
        for &cpu in cpus {
            let path = instance
                .join("per_cpu")
                .join(format!("cpu{cpu}"))
                .join("trace_pipe_raw");
            let pipe = dawos_rt::files::open_without_waiting(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            tracer.pipes.push((cpu, pipe));
        }
        write(&instance.join("tracing_on"), "1")?;
        Ok(tracer)
    }

    /// Stop the kernel writing. What is in the rings can still be read.
    pub fn stop(&mut self) {
        let _ = fs::write(self.instance.join("tracing_on"), "0");
    }

    /// Give `each` every event that is there to read, CPU by CPU and in
    /// the order it happened on each. Never waits.
    pub fn read(&mut self, mut each: impl FnMut(Event)) {
        for (cpu, pipe) in &mut self.pipes {
            for _ in 0..PAGES_A_ROUND {
                // One read gives one page, whole.
                match pipe.read(&mut self.page) {
                    Ok(length) if length > 0 => {
                        let catalog = &self.catalog;
                        let mut given = 0;
                        let summary = read_page(&self.page[..length], |time, record| {
                            if let Some(event) = catalog.decode(*cpu, time, record) {
                                each(event);
                                given += 1;
                            }
                        });
                        self.records += summary.records;
                        self.events += given;
                        self.lost_pages += u64::from(summary.lost);
                        self.malformed_pages += u64::from(summary.malformed);
                    }
                    Ok(_) => break,
                    Err(error) if error.kind() == ErrorKind::Interrupted => {}
                    // Nothing to read now (`WouldBlock`), or the trace
                    // is gone.
                    Err(_) => break,
                }
            }
        }
    }
}

impl Drop for Tracer {
    fn drop(&mut self) {
        self.stop();
        // The instance cannot go while its files are open.
        self.pipes.clear();
        let _ = fs::remove_dir(&self.instance);
    }
}

/// The name of a system call of x86-64 by its number, for those an
/// engine and its libraries are likely to make.
#[must_use]
pub fn syscall_name(number: u64) -> Option<&'static str> {
    Some(match number {
        0 => "read",
        1 => "write",
        3 => "close",
        7 => "poll",
        9 => "mmap",
        10 => "mprotect",
        11 => "munmap",
        12 => "brk",
        13 => "rt_sigaction",
        14 => "rt_sigprocmask",
        15 => "rt_sigreturn",
        16 => "ioctl",
        24 => "sched_yield",
        28 => "madvise",
        35 => "nanosleep",
        39 => "getpid",
        60 => "exit",
        186 => "gettid",
        202 => "futex",
        228 => "clock_gettime",
        230 => "clock_nanosleep",
        257 => "openat",
        271 => "ppoll",
        273 => "set_robust_list",
        334 => "rseq",
        _ => return None,
    })
}

/// The name of a kind of deferred interrupt work.
#[must_use]
pub fn softirq_name(kind: u64) -> &'static str {
    match kind {
        0 => "high priority",
        1 => "timer",
        2 => "network send",
        3 => "network receive",
        4 => "block",
        5 => "irq poll",
        6 => "tasklet",
        7 => "scheduler",
        8 => "high resolution timer",
        9 => "rcu",
        _ => "unknown",
    }
}

/// The names of the functions of the kernel, from `/proc/kallsyms`:
/// for the timers and the deferred work of a report, whose records
/// carry an address. Read once, when a report needs it.
pub struct Symbols {
    /// Address and name, by address.
    names: Vec<(u64, String)>,
}

impl Symbols {
    /// Read the names. Empty when the kernel does not show them (not
    /// root, or addresses hidden).
    #[must_use]
    pub fn load() -> Self {
        Self::from_text(&fs::read_to_string("/proc/kallsyms").unwrap_or_default())
    }

    /// The names in a text like that of `/proc/kallsyms`.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        let mut names: Vec<(u64, String)> = text
            .lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let address = u64::from_str_radix(parts.next()?, 16).ok()?;
                let kind = parts.next()?;
                let name = parts.next()?;
                // Code only, and not the hidden addresses (all zero).
                (address != 0 && matches!(kind, "t" | "T" | "w" | "W"))
                    .then(|| (address, name.to_string()))
            })
            .collect();
        names.sort_unstable();
        Self { names }
    }

    /// The function `address` is in: its name, or the address itself.
    #[must_use]
    pub fn name(&self, address: u64) -> String {
        let after = self.names.partition_point(|(start, _)| *start <= address);
        match after.checked_sub(1).and_then(|place| self.names.get(place)) {
            // Not further than a function is long.
            Some((start, name)) if address - start < 65_536 => name.clone(),
            _ => format!("{address:#x}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page read from `trace_pipe_raw` of CPU 1 of the virtual
    /// machine, kernel 6.18.64, with four events on: 89 records.
    const PAGE: &[u8] = include_bytes!("../testdata/trace-page.bin");

    const SWITCH: &str = "name: sched_switch\nID: 219\nformat:\n\
        \tfield:unsigned short common_type;\toffset:0;\tsize:2;\tsigned:0;\n\
        \tfield:int common_pid;\toffset:4;\tsize:4;\tsigned:1;\n\n\
        \tfield:char prev_comm[16];\toffset:8;\tsize:16;\tsigned:0;\n\
        \tfield:pid_t prev_pid;\toffset:24;\tsize:4;\tsigned:1;\n\
        \tfield:int prev_prio;\toffset:28;\tsize:4;\tsigned:1;\n\
        \tfield:long prev_state;\toffset:32;\tsize:8;\tsigned:1;\n\
        \tfield:char next_comm[16];\toffset:40;\tsize:16;\tsigned:0;\n\
        \tfield:pid_t next_pid;\toffset:56;\tsize:4;\tsigned:1;\n\
        \tfield:int next_prio;\toffset:60;\tsize:4;\tsigned:1;\n";
    const SYS_ENTER: &str = "\tfield:long id;\toffset:8;\tsize:8;\tsigned:1;\n\
        \tfield:unsigned long args[6];\toffset:16;\tsize:48;\tsigned:0;\n";
    const TIMER_IRQ: &str = "\tfield:int vector;\toffset:8;\tsize:4;\tsigned:1;\n";
    const HRTIMER: &str = "\tfield:void * hrtimer;\toffset:8;\tsize:8;\tsigned:0;\n\
        \tfield:s64 now;\toffset:16;\tsize:8;\tsigned:1;\n\
        \tfield:void * function;\toffset:24;\tsize:8;\tsigned:0;\n";

    /// The catalog of the machine the page was read on.
    fn catalog() -> Catalog {
        let mut catalog = Catalog::default();
        let switch = ["prev_pid", "next_pid", "prev_state", "next_comm"];
        assert!(catalog.add(219, What::Switch, &switch, SWITCH));
        assert!(catalog.add(283, What::SyscallEnter, &["id", "", "", ""], SYS_ENTER));
        let timer = What::Vector("local timer interrupt");
        assert!(catalog.add(167, timer, &["vector", "", "", ""], TIMER_IRQ));
        assert!(catalog.add(294, What::Timer, &["function", "", "", ""], HRTIMER));
        catalog
    }

    fn comm(name: &str) -> [u8; 16] {
        let mut comm = [0; 16];
        comm[..name.len()].copy_from_slice(name.as_bytes());
        comm
    }

    #[test]
    fn a_page_of_the_kernel_reads_as_the_text_trace_of_the_same_events() {
        let catalog = catalog();
        let mut events = Vec::new();
        let summary = read_page(PAGE, |time, record| {
            events.extend(catalog.decode(1, time, record));
        });
        assert_eq!(
            summary,
            PageSummary {
                records: 89,
                lost: false,
                malformed: false
            }
        );
        assert_eq!(events.len(), 89);
        // "<idle>-0 [001] 100.387756: sched_switch: prev_comm=swapper/1
        // prev_pid=0 prev_state=R ==> next_comm=ksoftirqd/1 next_pid=31"
        assert_eq!(
            events[0],
            Event {
                cpu: 1,
                time_ns: 100_387_756_159,
                pid: 0,
                what: What::Switch,
                a: 0,
                b: 31,
                c: 0,
                comm: comm("ksoftirqd/1"),
            }
        );
        // "ksoftirqd/1-31 100.387860: sched_switch: ... prev_state=S ==>
        // next_comm=swapper/1 next_pid=0"
        assert_eq!(events[1].time_ns, 100_387_859_508);
        assert_eq!(
            (events[1].pid, events[1].a, events[1].b, events[1].c),
            (31, 31, 0, 1)
        );
        // "<idle>-0 100.388118: local_timer_entry: vector=236"
        assert_eq!(events[2].what, What::Vector("local timer interrupt"));
        assert_eq!((events[2].time_ns, events[2].a), (100_388_118_301, 236));
        // "<idle>-0 100.388134: hrtimer_expire_entry: ... function=..."
        assert_eq!(events[3].what, What::Timer);
        assert_eq!(events[3].a, 0xffff_ffff_85e7_88c0);
        // "sh-1457 100.427920: sys_enter: NR 9 (...)"
        assert_eq!(events[87].what, What::SyscallEnter);
        assert_eq!(
            (events[87].time_ns, events[87].pid, events[87].a),
            (100_427_919_531, 1457, 9)
        );
        assert_eq!(events[88].time_ns, 100_428_075_420);
        // In the order they happened.
        assert!(events
            .windows(2)
            .all(|pair| pair[0].time_ns <= pair[1].time_ns));
        let count = |what: What| events.iter().filter(|event| event.what == what).count();
        assert_eq!(count(What::Switch), 39);
        assert_eq!(count(What::Vector("local timer interrupt")), 24);
        assert_eq!(count(What::Timer), 23);
        assert_eq!(count(What::SyscallEnter), 3);
    }

    #[test]
    fn events_not_asked_for_are_passed_over() {
        let mut catalog = Catalog::default();
        assert!(catalog.add(283, What::SyscallEnter, &["id", "", "", ""], SYS_ENTER));
        let mut events = 0;
        let summary = read_page(PAGE, |time, record| {
            events += usize::from(catalog.decode(1, time, record).is_some());
        });
        assert_eq!(summary.records, 89);
        assert_eq!(events, 3);
    }

    #[test]
    fn a_page_cut_short_or_spoiled_does_not_bring_the_reader_down() {
        // Cut anywhere: what is whole is read, the rest is refused.
        for length in [0, 7, 15, 16, 17, 100, 2000, 4083] {
            let summary = read_page(&PAGE[..length], |_, _| {});
            assert!(summary.malformed, "{length} bytes");
            assert_eq!(summary.records, 0);
        }
        // The records of this page end at byte 4084: what follows is
        // not looked at.
        assert_eq!(read_page(&PAGE[..4084], |_, _| {}).records, 89);
        // A length that claims more than the page has.
        let mut page = PAGE.to_vec();
        page[8..12].copy_from_slice(&60_000_u32.to_le_bytes());
        assert!(read_page(&page, |_, _| {}).malformed);
        // Noise in place of the records: no panic, whatever comes out.
        let mut page = PAGE.to_vec();
        for (place, byte) in page.iter_mut().enumerate().skip(16) {
            *byte = (place * 31 + 7) as u8;
        }
        let _ = read_page(&page, |_, _| {});
    }

    #[test]
    fn records_lost_and_long_stretches_of_time_are_understood() {
        // A page made by hand: instant 1000, then a stretch of time
        // longer than 27 bits, a record of one word, padding to the
        // end. The kernel says records were lost before it.
        let mut page = Vec::new();
        page.extend(1000_u64.to_le_bytes());
        page.extend((20_u32 | 1 << 31).to_le_bytes());
        page.extend(0_u32.to_le_bytes());
        // Kind 30, 5 ns in the head and 3 << 27 ns in the next word.
        page.extend((30_u32 | 5 << 5).to_le_bytes());
        page.extend(3_u32.to_le_bytes());
        // Kind 1 (one word), 7 ns later.
        page.extend((1_u32 | 7 << 5).to_le_bytes());
        page.extend([0xaa, 0xbb, 0xcc, 0xdd]);
        // Padding to the end.
        page.extend(29_u32.to_le_bytes());
        let mut seen = Vec::new();
        let summary = read_page(&page, |time, record| seen.push((time, record.to_vec())));
        assert_eq!(
            summary,
            PageSummary {
                records: 1,
                lost: true,
                malformed: false
            }
        );
        assert_eq!(
            seen,
            vec![(1000 + 5 + (3 << 27) + 7, vec![0xaa, 0xbb, 0xcc, 0xdd])]
        );
    }

    #[test]
    fn an_event_without_a_field_it_needs_is_refused() {
        let mut catalog = Catalog::default();
        assert!(!catalog.add(1, What::Switch, &["prev_pid", "no_such", "", ""], SWITCH));
        assert!(catalog.decoders.is_empty());
    }

    #[test]
    fn sets_of_cpus_are_written_as_the_kernel_reads_them() {
        assert_eq!(cpu_mask(&[4, 5, 6, 7]), "f0");
        assert_eq!(cpu_mask(&[0]), "1");
        assert_eq!(cpu_mask(&[31, 32]), "1,80000000");
        assert_eq!(cpu_mask(&[70]), "40,0,0");
    }

    #[test]
    fn functions_of_the_kernel_are_found_by_address() {
        let symbols = Symbols::from_text(
            "ffffffff81000000 T _text\n\
             ffffffff81200000 T tick_nohz_handler\n\
             ffffffff81200400 t hrtimer_wakeup\n\
             ffffffff82000000 D some_data\n\
             0000000000000000 T hidden\n",
        );
        assert_eq!(symbols.name(0xffff_ffff_8120_0000), "tick_nohz_handler");
        assert_eq!(symbols.name(0xffff_ffff_8120_0010), "tick_nohz_handler");
        assert_eq!(symbols.name(0xffff_ffff_8120_0400), "hrtimer_wakeup");
        // Far from any function, or with no names at all: the address.
        assert_eq!(symbols.name(0xffff_ffff_9000_0000), "0xffffffff90000000");
        assert_eq!(Symbols::from_text("").name(0x1234), "0x1234");
    }

    #[test]
    fn the_calls_an_engine_makes_have_names() {
        assert_eq!(syscall_name(7), Some("poll"));
        assert_eq!(syscall_name(16), Some("ioctl"));
        assert_eq!(syscall_name(230), Some("clock_nanosleep"));
        assert_eq!(syscall_name(9999), None);
        assert_eq!(softirq_name(9), "rcu");
    }
}
