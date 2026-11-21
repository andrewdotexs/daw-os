//! The command `run`: the live threads, their spares, the watchdog, and
//! the report.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use dawos_audio_io::{Format, NullBackend};
use dawos_rt::cpu::{self, Policy};
use dawos_rt::slot::Slot;
use dawos_rt::{clock, topology};

use crate::cli::{Realtime, RunOptions};
use crate::live::{live_loop, End, Shared, ThreadReport};
use crate::notify::Notifier;
use crate::plan::{Fault, Plan};
use crate::watchdog::{watchdog_loop, Event, Spare, Watch, STUCK_PERIODS};

/// Priority of the watchdog: above the live threads and above anything
/// else on the CPUs it runs on.
const WATCHDOG_PRIORITY: u8 = 95;

/// How many stuck threads one run can record.
const MAX_EVENTS: usize = 8;

/// Limits of ADR 0023, counted from the beginning of the compute phase
/// the thread never left.
const CORE_FREE_LIMIT_MS: f64 = 15.0;
const SPARE_LIMIT_MS: f64 = 50.0;

/// Limit for the kernel probe during a fault: the live CPU is free
/// within 15 ms, and the kernel's work then needs the wait of one cycle.
const KERNEL_WORK_LIMIT_MS: f64 = 20.0;

/// A kernel file whose reading makes every CPU run a piece of work in
/// one of its own kernel threads, and returns when all have done it.
const KERNEL_WORK_FILE: &str = "/proc/sys/vm/stat_refresh";

/// What the kernel probe measured.
pub struct KernelProbe {
    /// Rounds completed.
    pub rounds: u64,
    /// Mean and longest round, in milliseconds.
    pub mean_ms: f64,
    pub max_ms: f64,
    /// Whether the file could be read at all.
    pub readable: bool,
}

/// What a run produced.
pub struct Outcome {
    pub format: Format,
    pub live_cpus: Vec<usize>,
    pub system_cpus: Vec<usize>,
    pub threads: usize,
    pub realtime: bool,
    pub memory_locked: bool,
    pub options: RunOptions,
    /// One report per thread that ended, in no particular order.
    pub reports: Vec<ThreadReport>,
    /// Slots whose first thread never came back.
    pub never_returned: Vec<usize>,
    pub events: Vec<EventView>,
    pub watchdog_rounds: u64,
    /// Whether systemd was listening for the engine's signs of life.
    pub systemd: bool,
    /// The kernel probe, when asked for.
    pub kernel_probe: Option<KernelProbe>,
}

/// One intervention of the watchdog, with times in milliseconds from the
/// beginning of the compute phase the thread never left.
pub struct EventView {
    pub slot: usize,
    pub cycle: u64,
    pub node: Option<usize>,
    pub tid: i32,
    pub detected_ms: f64,
    pub core_free_ms: f64,
    pub spare_ms: Option<f64>,
    pub demote_errors: u32,
}

/// Run and print; an error when the run could not start, or when a fault
/// was injected and not handled within the limits.
pub fn run(options: &RunOptions) -> Result<(), String> {
    let outcome = execute(options)?;
    print(&outcome);
    match verdict(&outcome) {
        Verdict::NotAsked => Ok(()),
        Verdict::Pass(text) => {
            println!("k04: PASS ({text})");
            Ok(())
        }
        Verdict::Fail(text) => {
            println!("k04: FAIL ({text})");
            Err("the fault was not handled as ADR 0023 requires".to_string())
        }
    }
}

/// CPUs from an option, else from a domain of the boot profile, else
/// none.
fn cpus(option: Option<&str>, domain: &str) -> Result<Vec<usize>, String> {
    let list = match option {
        Some(list) => Some(list.to_string()),
        None => topology::domain(Path::new(topology::REPORT), domain),
    };
    match list {
        Some(list) => cpu::parse_cpu_list(&list).map_err(|error| error.to_string()),
        None => Ok(Vec::new()),
    }
}

/// Everything a thread needs to start, shared by all of them.
struct Common {
    shared: Arc<Shared>,
    spares: Arc<Vec<Spare>>,
    format: Format,
    start_ns: u64,
    live_cpus: Vec<usize>,
    system_cpus: Vec<usize>,
    priority: u8,
    realtime: Realtime,
    /// Threads that are set up and waiting for the first period.
    ready: AtomicUsize,
    /// Threads that did not get the real-time priority.
    without_realtime: AtomicUsize,
    /// What went wrong while starting, if anything.
    errors: Mutex<Vec<String>>,
}

impl Common {
    fn error(&self, message: String) {
        if let Ok(mut errors) = self.errors.lock() {
            errors.push(message);
        }
    }

    /// Give the calling thread the real-time priority, as the options
    /// say.
    fn take_priority(&self, priority: u8, who: &str) {
        if self.realtime == Realtime::Off {
            self.without_realtime.fetch_add(1, Ordering::AcqRel);
            return;
        }
        if let Err(error) = cpu::set_policy(0, Policy::Fifo(priority)) {
            self.without_realtime.fetch_add(1, Ordering::AcqRel);
            if self.realtime == Realtime::Required {
                self.error(format!(
                    "{who}: cannot take the real-time priority {priority}: {error}"
                ));
            }
        }
    }
}

/// Start everything, wait for the time asked, stop, collect.
pub fn execute(options: &RunOptions) -> Result<Outcome, String> {
    let format = Format {
        rate: options.rate,
        frames: options
            .frames
            .unwrap_or(Format::default_frames(options.rate)),
    };
    let period_ns = format.period_ns();
    let live_cpus =
        cpus(options.live.as_deref(), "live").map_err(|error| format!("--live: {error}"))?;
    let system_cpus =
        cpus(options.system.as_deref(), "system").map_err(|error| format!("--system: {error}"))?;
    let threads = options.threads.unwrap_or(live_cpus.len().max(1));
    if !live_cpus.is_empty() && threads > live_cpus.len() {
        return Err(format!(
            "{threads} live threads asked for, and {} live CPUs: one thread per CPU at most",
            live_cpus.len()
        ));
    }
    if options.fault != Fault::None && options.fault_thread >= threads {
        return Err(format!(
            "--fault-thread {}: there are {threads} live threads, numbered from 0",
            options.fault_thread
        ));
    }
    if options.fault != Fault::None && options.fault_after_s >= options.seconds {
        return Err("--fault-after must be less than --seconds".to_string());
    }

    let memory_locked = match cpu::lock_memory() {
        Ok(()) => true,
        Err(error) if options.realtime == Realtime::Required => {
            return Err(format!("cannot lock the memory of the process: {error}"));
        }
        Err(_) => false,
    };

    let fault_cycle = options.fault_after_s * 1_000_000_000 / period_ns;
    let shared = Arc::new(Shared {
        slots: (0..threads).map(|_| Slot::new()).collect(),
        plan: Plan::new(
            threads,
            period_ns,
            options.load_percent,
            options.fault,
            options.fault_thread,
            fault_cycle,
        ),
        stop: AtomicBool::new(false),
        period_ns,
        margin_ns: options.margin_us * 1_000,
        fault: options.fault,
        fault_thread: options.fault_thread,
        fault_cycle,
    });
    let spares: Arc<Vec<Spare>> = Arc::new((0..threads).map(|_| Spare::default()).collect());
    let events: Arc<Vec<Event>> = Arc::new((0..MAX_EVENTS).map(|_| Event::default()).collect());
    let heartbeat = Arc::new(AtomicU64::new(0));

    // The periods start a little from now: time for every thread to be
    // in place and waiting.
    let start_ns = clock::now() + 300_000_000;
    let common = Arc::new(Common {
        shared: Arc::clone(&shared),
        spares: Arc::clone(&spares),
        format,
        start_ns,
        live_cpus: live_cpus.clone(),
        system_cpus: system_cpus.clone(),
        priority: options.priority,
        realtime: options.realtime,
        ready: AtomicUsize::new(0),
        without_realtime: AtomicUsize::new(0),
        errors: Mutex::new(Vec::new()),
    });

    // ---- The spare threads: parked outside the live CPUs.
    let mut handles: Vec<(usize, bool, JoinHandle<Option<ThreadReport>>)> = Vec::new();
    for index in 0..threads {
        let common = Arc::clone(&common);
        let handle = spawn(format!("spare-{index}"), move || {
            spare_thread(&common, index)
        })?;
        handles.push((index, true, handle));
    }

    // ---- The live threads: one per live CPU.
    for index in 0..threads {
        let common = Arc::clone(&common);
        let handle = spawn(format!("live-{index}"), move || {
            Some(live_thread(&common, index))
        })?;
        handles.push((index, false, handle));
    }

    // ---- The watchdog.
    let watchdog = {
        let common = Arc::clone(&common);
        let events = Arc::clone(&events);
        let heartbeat = Arc::clone(&heartbeat);
        spawn("watchdog".to_string(), move || {
            if !common.system_cpus.is_empty() {
                if let Err(error) = cpu::set_affinity(0, &common.system_cpus) {
                    common.error(format!("watchdog: cannot run on the system CPUs: {error}"));
                }
            }
            common.take_priority(WATCHDOG_PRIORITY, "watchdog");
            common.ready.fetch_add(1, Ordering::AcqRel);
            let watch = Watch {
                shared: &common.shared,
                spares: &common.spares,
                events: &events,
                heartbeat: &heartbeat,
                live_cpus: &common.live_cpus,
                system_cpus: &common.system_cpus,
                priority: (common.realtime != Realtime::Off
                    && common.without_realtime.load(Ordering::Acquire) == 0)
                    .then_some(common.priority),
                start_ns: common.start_ns,
            };
            watchdog_loop(&watch);
        })?
    };

    // ---- The kernel probe, when asked for: on the system CPUs, at
    // normal priority.
    let probe = if options.kernel_probe {
        let common = Arc::clone(&common);
        Some(spawn("kernel-probe".to_string(), move || {
            kernel_probe(&common)
        })?)
    } else {
        None
    };

    // ---- Everybody in place? Twice the threads (live and spare) plus
    // the watchdog.
    let expected = 2 * threads + 1;
    while common.ready.load(Ordering::Acquire) < expected && clock::now() < start_ns {
        thread::sleep(Duration::from_millis(1));
    }
    let errors = common
        .errors
        .lock()
        .map(|errors| errors.clone())
        .unwrap_or_default();
    let late = common.ready.load(Ordering::Acquire) < expected;
    if !errors.is_empty() || late {
        shared.stop.store(true, Ordering::Release);
        for spare in spares.iter() {
            spare.gate.open();
        }
        return Err(if late {
            "the threads of the engine did not start in time".to_string()
        } else {
            errors.join("; ")
        });
    }

    // ---- The run. This thread only keeps time and tells systemd that
    // the watchdog is alive: when the watchdog stops, so do the messages.
    let notifier = Notifier::from_environment();
    notifier.ready();
    let end_ns = start_ns + options.seconds * 1_000_000_000;
    let mut last_heartbeat = 0;
    while clock::now() < end_ns {
        thread::sleep(Duration::from_millis(100));
        let now = heartbeat.load(Ordering::Acquire);
        if now != last_heartbeat {
            last_heartbeat = now;
            notifier.alive();
        }
    }

    // ---- Stop and collect. A thread that is stuck never ends: give the
    // others a few periods, then go on without it.
    shared.stop.store(true, Ordering::Release);
    for spare in spares.iter() {
        spare.gate.open();
    }
    thread::sleep(Duration::from_nanos(4 * period_ns + 20_000_000));
    let mut reports = Vec::new();
    let mut never_returned = Vec::new();
    for (index, spare, handle) in handles {
        if handle.is_finished() {
            if let Ok(Some(report)) = handle.join() {
                reports.push(report);
            }
        } else if !spare {
            never_returned.push(index);
        }
    }
    if watchdog.is_finished() {
        let _ = watchdog.join();
    }
    let kernel_probe = probe.and_then(|handle| handle.join().ok());

    let events = events
        .iter()
        .filter(|event| event.happened.load(Ordering::Acquire) == 1)
        .map(|event| {
            let slot = event.slot.load(Ordering::Acquire);
            let since = event.since_ns.load(Ordering::Acquire);
            let ms = |instant: u64| instant.saturating_sub(since) as f64 / 1e6;
            let spare_first = match event.spare.load(Ordering::Acquire) {
                0 => 0,
                spare => spares[spare - 1].first_cycle_ns.load(Ordering::Acquire),
            };
            EventView {
                slot,
                cycle: event.cycle.load(Ordering::Acquire),
                node: match event.node.load(Ordering::Acquire) {
                    0 => None,
                    node => Some(node as usize - 1),
                },
                tid: event.tid.load(Ordering::Acquire),
                detected_ms: ms(event.detected_ns.load(Ordering::Acquire)),
                core_free_ms: ms(event.demoted_ns.load(Ordering::Acquire)),
                spare_ms: (spare_first != 0).then(|| ms(spare_first)),
                demote_errors: event.demote_errors.load(Ordering::Acquire),
            }
        })
        .collect();

    Ok(Outcome {
        format,
        live_cpus,
        system_cpus,
        threads,
        realtime: options.realtime != Realtime::Off
            && common.without_realtime.load(Ordering::Acquire) == 0,
        memory_locked,
        options: options.clone(),
        reports,
        never_returned,
        events,
        watchdog_rounds: heartbeat.load(Ordering::Acquire),
        systemd: notifier.active(),
        kernel_probe,
    })
}

/// Again and again, some hundred and fifty times a second, make the
/// kernel run a piece of work on every CPU and time how long it takes to
/// come back. With a live thread that
/// never waits, the work of its CPU cannot run and the reading hangs:
/// the longest round says for how long the kernel threads of the live
/// CPUs were kept from running.
fn kernel_probe(common: &Common) -> KernelProbe {
    if !common.system_cpus.is_empty() {
        let _ = cpu::set_affinity(0, &common.system_cpus);
    }
    let mut probe = KernelProbe {
        rounds: 0,
        mean_ms: 0.0,
        max_ms: 0.0,
        readable: true,
    };
    let mut total_ms = 0.0;
    clock::sleep_until(common.start_ns);
    while !common.shared.stop.load(Ordering::Acquire) {
        let before = clock::now();
        if std::fs::read(KERNEL_WORK_FILE).is_err() {
            probe.readable = false;
            return probe;
        }
        let took_ms = (clock::now() - before) as f64 / 1e6;
        probe.rounds += 1;
        total_ms += took_ms;
        probe.max_ms = probe.max_ms.max(took_ms);
        // A short pause: a thread stuck for some ten milliseconds must
        // not fall between two rounds.
        thread::sleep(Duration::from_millis(5));
    }
    if probe.rounds > 0 {
        probe.mean_ms = total_ms / probe.rounds as f64;
    }
    probe
}

fn spawn<T: Send + 'static>(
    name: String,
    body: impl FnOnce() -> T + Send + 'static,
) -> Result<JoinHandle<T>, String> {
    thread::Builder::new()
        .name(name.clone())
        .spawn(body)
        .map_err(|error| format!("cannot start the thread '{name}': {error}"))
}

/// A live thread: its CPU, its priority, then the cycle.
fn live_thread(common: &Common, index: usize) -> ThreadReport {
    if let Some(&cpu) = common.live_cpus.get(index) {
        if let Err(error) = cpu::set_affinity(0, &[cpu]) {
            common.error(format!(
                "live thread {index}: cannot run on CPU {cpu}: {error}"
            ));
        }
    }
    common.take_priority(common.priority, &format!("live thread {index}"));
    let slot = &common.shared.slots[index];
    slot.take(cpu::thread_id());
    let mut backend = NullBackend::with_start(common.format, common.start_ns);
    common.ready.fetch_add(1, Ordering::AcqRel);
    live_loop(&common.shared, index, slot.generation(), None, &mut backend)
}

/// A spare thread: parked on the system CPUs until the watchdog opens its
/// gate, having already moved it to a live CPU and given it the priority.
fn spare_thread(common: &Common, index: usize) -> Option<ThreadReport> {
    if !common.system_cpus.is_empty() {
        if let Err(error) = cpu::set_affinity(0, &common.system_cpus) {
            common.error(format!(
                "spare thread {index}: cannot run on the system CPUs: {error}"
            ));
        }
    }
    let spare = &common.spares[index];
    spare.tid.store(cpu::thread_id(), Ordering::Release);
    common.ready.fetch_add(1, Ordering::AcqRel);

    spare.gate.wait();
    if common.shared.stop.load(Ordering::Acquire) {
        return None;
    }
    let slot_index = spare.slot.load(Ordering::Acquire);
    let generation = spare.generation.load(Ordering::Acquire);
    common.shared.slots[slot_index].take(cpu::thread_id());
    let mut backend = NullBackend::joining(common.format, common.start_ns);
    Some(live_loop(
        &common.shared,
        slot_index,
        generation,
        Some(&spare.first_cycle_ns),
        &mut backend,
    ))
}

/// The answer to "was the fault handled?".
enum Verdict {
    NotAsked,
    Pass(String),
    Fail(String),
}

fn verdict(outcome: &Outcome) -> Verdict {
    let period_ms = outcome.format.period_ns() as f64 / 1e6;
    // Three periods stuck, one of grain of the watchdog, half a
    // millisecond of slack: 11 ms at 2.667 ms.
    let detect_limit_ms = (STUCK_PERIODS + 1) as f64 * period_ms + 0.5;
    match outcome.options.fault {
        Fault::None => Verdict::NotAsked,
        Fault::WatchdogOff => {
            if outcome.events.is_empty() {
                Verdict::Pass(
                    "the watchdog was off and did nothing, as asked; whether systemd restarts the engine is seen from outside"
                        .to_string(),
                )
            } else {
                Verdict::Fail("the watchdog was meant to be off, and it acted".to_string())
            }
        }
        Fault::StuckNode | Fault::StuckThread => {
            let Some(event) = outcome.events.first() else {
                return Verdict::Fail("the watchdog did not notice the stuck thread".to_string());
            };
            let limits = format!(
                "limits: noticed within {detect_limit_ms:.1} ms, live CPU free within {CORE_FREE_LIMIT_MS:.0} ms, spare thread at work within {SPARE_LIMIT_MS:.0} ms{}",
                if outcome.kernel_probe.is_some() {
                    format!(", kernel work on every CPU within {KERNEL_WORK_LIMIT_MS:.0} ms")
                } else {
                    String::new()
                }
            );
            let spare_after = outcome
                .reports
                .iter()
                .find(|report| report.spare && report.slot == event.slot)
                .map_or(0, |report| report.cycles);
            let kernel_good = outcome
                .kernel_probe
                .as_ref()
                .is_none_or(|probe| probe.readable && probe.max_ms <= KERNEL_WORK_LIMIT_MS);
            let good = kernel_good
                && event.detected_ms <= detect_limit_ms
                && event.core_free_ms <= CORE_FREE_LIMIT_MS
                && event.spare_ms.is_some_and(|ms| ms <= SPARE_LIMIT_MS)
                && event.demote_errors == 0
                && spare_after > 0;
            if good {
                Verdict::Pass(limits)
            } else {
                Verdict::Fail(limits)
            }
        }
    }
}

fn us(ns: Option<u64>) -> String {
    ns.map_or_else(
        || "-".to_string(),
        |ns| format!("{:.2}", ns as f64 / 1000.0),
    )
}

fn list(cpus: &[usize]) -> String {
    if cpus.is_empty() {
        "not pinned".to_string()
    } else {
        cpus.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    }
}

fn print(outcome: &Outcome) {
    let options = &outcome.options;
    println!("dawos-engine {}", env!("CARGO_PKG_VERSION"));
    println!("backend: null");
    println!("rate_hz: {}", outcome.format.rate);
    println!("quantum_frames: {}", outcome.format.frames);
    println!(
        "period_us: {:.3}",
        outcome.format.period_ns() as f64 / 1000.0
    );
    println!("live_threads: {}", outcome.threads);
    println!(
        "live_cpus: {}",
        list(&outcome.live_cpus[..outcome.threads.min(outcome.live_cpus.len())])
    );
    println!("system_cpus: {}", list(&outcome.system_cpus));
    if outcome.realtime {
        println!(
            "scheduling: fifo {} (watchdog: fifo {WATCHDOG_PRIORITY})",
            options.priority
        );
    } else if options.realtime == Realtime::Off {
        println!("scheduling: normal (--no-rt)");
    } else {
        println!("scheduling: normal (the kernel refused the real-time priority: figures below are not those of a live CPU)");
    }
    println!(
        "memory_locked: {}",
        if outcome.memory_locked { "yes" } else { "no" }
    );
    println!("seconds: {}", options.seconds);
    println!("load_percent: {}", options.load_percent);
    println!("margin_us: {}", options.margin_us);
    println!(
        "fault: {}",
        match options.fault {
            Fault::None => "none".to_string(),
            Fault::StuckNode => format!(
                "stuck-node on thread {} after {} s",
                options.fault_thread, options.fault_after_s
            ),
            Fault::StuckThread => format!(
                "stuck-thread on thread {} after {} s",
                options.fault_thread, options.fault_after_s
            ),
            Fault::WatchdogOff => format!(
                "watchdog-off, then stuck-node on thread {} after {} s",
                options.fault_thread, options.fault_after_s
            ),
        }
    );
    println!("watchdog_rounds: {}", outcome.watchdog_rounds);
    println!(
        "systemd_listening: {}",
        if outcome.systemd { "yes" } else { "no" }
    );
    match &outcome.kernel_probe {
        None => {}
        Some(probe) if !probe.readable => {
            println!("kernel_work_ms: cannot read {KERNEL_WORK_FILE} (root only)");
        }
        Some(probe) => println!(
            "kernel_work_ms: {} rounds, mean {:.2}, longest {:.2}",
            probe.rounds, probe.mean_ms, probe.max_ms
        ),
    }

    println!();
    println!("threads (delays and times in microseconds):");
    println!(
        "{:<6} {:<6} {:<8} {:>8} {:>7} {:>10} {:>8} {:>9} {:>9} {:>11} {:>11} {:>12}",
        "slot",
        "kind",
        "end",
        "cycles",
        "missed",
        "incomplete",
        "overruns",
        "late_avg",
        "late_max",
        "compute_avg",
        "compute_max",
        "least_margin"
    );
    let mut reports: Vec<&ThreadReport> = outcome.reports.iter().collect();
    reports.sort_by_key(|report| (report.slot, report.spare));
    for report in reports {
        println!(
            "{:<6} {:<6} {:<8} {:>8} {:>7} {:>10} {:>8} {:>9} {:>9} {:>11} {:>11} {:>12}",
            report.slot,
            if report.spare { "spare" } else { "first" },
            match report.end {
                End::Stopped => "stopped",
                End::Retired => "retired",
            },
            report.cycles,
            report.missed,
            report.incomplete,
            report.overruns,
            us(report.lateness.mean_ns()),
            us(report.lateness.max_ns()),
            us(report.compute.mean_ns()),
            us(report.compute.max_ns()),
            if report.cycles == 0 {
                "-".to_string()
            } else {
                us(report.least_margin_ns)
            },
        );
    }
    for slot in &outcome.never_returned {
        println!(
            "{slot:<6} {:<6} never came back: stuck, moved off its CPU by the watchdog",
            "first"
        );
    }

    if !outcome.events.is_empty() {
        println!();
        println!("watchdog events (milliseconds from the start of the compute phase the thread never left):");
    }
    for event in &outcome.events {
        println!(
            "slot {} cycle {} {} thread {}: noticed after {:.2}, live CPU free after {:.2}, spare thread at work after {}{}",
            event.slot,
            event.cycle,
            event.node.map_or_else(|| "outside any node".to_string(), |node| format!("node {node}")),
            event.tid,
            event.detected_ms,
            event.core_free_ms,
            event.spare_ms.map_or_else(|| "never".to_string(), |ms| format!("{ms:.2}")),
            if event.demote_errors == 0 { "" } else { " (moving the thread failed)" },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // One run at a time: each keeps several CPUs busy, and a thread made
    // to get stuck on purpose goes on spinning, at the lowest priority,
    // until the test program ends.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn alone() -> std::sync::MutexGuard<'static, ()> {
        ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    // Periods of one millisecond and no real-time priority: the logic,
    // on whatever machine runs the tests. Times are not judged here.
    fn options(fault: Fault) -> RunOptions {
        RunOptions {
            frames: Some(48),
            seconds: 2,
            threads: Some(2),
            realtime: Realtime::Off,
            load_percent: 10,
            fault,
            fault_after_s: 1,
            fault_thread: 1,
            ..RunOptions::default()
        }
    }

    #[test]
    fn a_healthy_run_has_no_events() {
        let _alone = alone();
        let outcome = execute(&options(Fault::None)).expect("the run did not start");
        assert!(outcome.events.is_empty());
        assert!(outcome.never_returned.is_empty());
        assert_eq!(outcome.reports.len(), 2);
        for report in &outcome.reports {
            assert!(!report.spare);
            assert_eq!(report.end, End::Stopped);
            let cycles = report.cycles + report.missed;
            assert!(
                (1_900..=2_010).contains(&cycles),
                "{cycles} cycles in two seconds"
            );
        }
        assert!(outcome.watchdog_rounds > 1_000);
    }

    #[test]
    fn a_stuck_node_is_replaced() {
        let _alone = alone();
        let outcome = execute(&options(Fault::StuckNode)).expect("the run did not start");
        assert_eq!(outcome.events.len(), 1);
        let event = &outcome.events[0];
        assert_eq!(event.slot, 1);
        assert_eq!(event.node, Some(0));
        assert_eq!(event.demote_errors, 0);
        assert!(event.spare_ms.is_some());
        assert_eq!(outcome.never_returned, vec![1]);
        // The other thread went on, and a spare took the slot.
        let other = outcome
            .reports
            .iter()
            .find(|report| report.slot == 0)
            .expect("slot 0");
        assert!(other.cycles > 1_000);
        let spare = outcome
            .reports
            .iter()
            .find(|report| report.slot == 1 && report.spare)
            .expect("the spare thread of slot 1");
        assert!(
            spare.cycles > 500,
            "{} cycles of the spare thread",
            spare.cycles
        );
    }

    #[test]
    fn a_stuck_thread_is_replaced() {
        let _alone = alone();
        let outcome = execute(&options(Fault::StuckThread)).expect("the run did not start");
        assert_eq!(outcome.events.len(), 1);
        assert_eq!(outcome.events[0].node, None);
        assert_eq!(outcome.never_returned, vec![1]);
        assert!(outcome
            .reports
            .iter()
            .any(|report| report.slot == 1 && report.spare && report.cycles > 300));
    }

    #[test]
    fn too_much_work_is_cut_by_the_budget() {
        let _alone = alone();
        let mut options = options(Fault::None);
        options.load_percent = 150;
        let outcome = execute(&options).expect("the run did not start");
        assert!(
            outcome.events.is_empty(),
            "the watchdog acted on a heavy cycle"
        );
        for report in &outcome.reports {
            assert!(
                report.incomplete > 1_000,
                "{} incomplete cycles",
                report.incomplete
            );
        }
    }

    #[test]
    fn without_a_watchdog_nobody_acts() {
        let _alone = alone();
        let outcome = execute(&options(Fault::WatchdogOff)).expect("the run did not start");
        assert!(outcome.events.is_empty());
        assert_eq!(outcome.never_returned, vec![1]);
    }

    #[test]
    fn impossible_requests_are_refused() {
        let _alone = alone();
        let mut more_threads = options(Fault::None);
        more_threads.live = Some("0".to_string());
        assert!(execute(&more_threads).is_err());
        let mut wrong_thread = options(Fault::StuckNode);
        wrong_thread.fault_thread = 5;
        assert!(execute(&wrong_thread).is_err());
    }
}
