//! The command `run`: the live threads, their spares, the watchdog, and
//! the report.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use dawos_audio_io::{
    keeper_loop, AlsaBackend, AlsaFollower, AlsaShared, Backend, CardConfig, CardInfo, Format,
    NullBackend, XrunView,
};
use dawos_rt::cpu::{self, Policy};
use dawos_rt::dispatch;
use dawos_rt::ring::{ring, Writer};
use dawos_rt::slot::Slot;
use dawos_rt::{clock, topology};

use crate::cli::{BackendKind, Realtime, RunOptions};
use crate::ktrace::Tracer;
use crate::live::{live_loop, End, Shared, ThreadReport};
use crate::meter::{self, Meter, MeterReport, Shape, RING_RECORDS};
use crate::notify::Notifier;
use crate::patch::{Heard, Patch, PatchConfig};
use crate::plan::{Fault, Plan};
use crate::probe::{BurstKind, Counted, Timed};
use crate::watchdog::{watchdog_loop, Event, Spare, Watch, STUCK_PERIODS};
use dawos_graph::{Kind, Running};

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

/// What happened to the sound card during a run, in plain numbers.
pub struct CardOutcome {
    /// What the driver accepted.
    pub info: CardInfo,
    pub card_cycles: u64,
    pub silent_cycles: u64,
    pub xruns: u64,
    /// Those of the xruns that fell in the warm-up.
    pub early_xruns: u64,
    pub recoveries: u64,
    pub longest_recovery_ms: f64,
    pub stalls: u64,
    pub lost: u64,
    pub back: u64,
    pub reopen_attempts: u64,
    pub paused_restarts: u64,
    pub short_writes: u64,
    pub bad_stamps: u64,
    /// The other live threads: cycles taken after a tick of the card,
    /// those of them with periods gone by, wake-ups with no cycle, and
    /// timers of the card opened and refused.
    pub follower_cycles: u64,
    pub follower_behind: u64,
    pub follower_empty: u64,
    pub timers_opened: u64,
    pub timers_refused: u64,
    /// Error number that made the card be given up, the last time.
    pub last_error: i32,
    /// Whether a card was in use when the run ended.
    pub in_use_at_the_end: bool,
    /// What was heard on the inputs listened to.
    pub heard: Vec<Heard>,
    /// What the count found, and what the bursts found.
    pub counted: Option<Counted>,
    pub timed: Option<Timed>,
    /// The first xruns: seconds from the start, and how the streams
    /// stood.
    pub xrun_log: Vec<(f64, XrunView)>,
}

/// The tone counts as heard above this level, in dB relative to full
/// scale; and as clean when it is at least this share of the power.
const TONE_HEARD_DB: f64 = -60.0;
const TONE_CLEAN_SHARE: f64 = 0.9;

/// The largest slip of the signal a run may have and still pass, in
/// samples. The Scarlett 18i20 of the second generation slips by one or
/// two samples now and then, with any buffer and in either mode of the
/// USB driver: an open point (A-58), counted and reported, not a failure
/// of the engine. Anything larger is. The quarter of a sample on top is
/// the error of the measure on a slip that falls across two quanta.
const SLIP_LIMIT_SAMPLES: f64 = 2.25;

/// What a run produced.
pub struct Outcome {
    pub format: Format,
    pub live_cpus: Vec<usize>,
    pub system_cpus: Vec<usize>,
    pub threads: usize,
    pub realtime: bool,
    pub memory_locked: bool,
    /// Whether the kernel stops the system calls of the compute phase.
    pub dispatch: Watched,
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
    /// The sound card, with the back end that has one.
    pub card: Option<CardOutcome>,
    /// When the first period was due.
    pub start_ns: u64,
    /// What the record of every cycle says, when it was kept.
    pub meter: Option<MeterReport>,
    /// The graph, when the run had one.
    pub graph: Option<GraphOutcome>,
}

/// The graph of a run, and what became of it.
pub struct GraphOutcome {
    /// The file that describes it.
    pub file: String,
    /// What the compiler says of it.
    pub summary: String,
    /// Nodes the threads run in a cycle when they run them all: every
    /// node but the inputs of the card.
    pub nodes: usize,
    /// Buffers and nodes that were in use by another thread when one
    /// asked for them: none in a healthy run. After the warm-up, and
    /// in it.
    pub refused: u64,
    pub refused_early: u64,
    /// The highest sample every meter of the graph saw, by the name of
    /// its node; full scale is 1.
    pub peaks: Vec<(String, f32)>,
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

/// Whether the system calls of the compute phase are stopped by the
/// kernel (ADR 0021).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Watched {
    /// Not asked for (`--no-dispatch`).
    Off,
    /// On every thread that runs the cycle, tried at its start.
    On,
    /// The kernel refused, or the trial failed: why.
    Refused(String),
}

/// Run and print; an error when the run could not start, or when a fault
/// was injected and not handled within the limits.
pub fn run(options: &RunOptions) -> Result<(), String> {
    let outcome = execute(options)?;
    print(&outcome);
    let mut failures = Vec::new();
    match verdict(&outcome) {
        Verdict::NotAsked => {}
        Verdict::Pass(text) => println!("k04: PASS ({text})"),
        Verdict::Fail(text) => {
            println!("k04: FAIL ({text})");
            failures.push("the fault was not handled as ADR 0023 requires");
        }
    }
    match audio_verdict(&outcome) {
        Verdict::NotAsked => {}
        Verdict::Pass(text) => println!("audio: PASS ({text})"),
        Verdict::Fail(text) => {
            println!("audio: FAIL ({text})");
            failures.push("the sound card did not do what was asked");
        }
    }
    match compute_verdict(&outcome) {
        Verdict::NotAsked => {}
        Verdict::Pass(text) => println!("compute: PASS ({text})"),
        Verdict::Fail(text) => {
            println!("compute: FAIL ({text})");
            failures.push("system calls in the compute phase are not as ADR 0021 requires");
        }
    }
    match graph_verdict(&outcome) {
        Verdict::NotAsked => {}
        Verdict::Pass(text) => println!("graph: PASS ({text})"),
        Verdict::Fail(text) => {
            println!("graph: FAIL ({text})");
            failures.push("the graph was not computed whole in every cycle");
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
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
    /// The sound card, when the back end is ALSA.
    card: Option<Arc<AlsaShared>>,
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
    /// Have the kernel stop the system calls of the compute phase, and
    /// why it does not on some thread, if so.
    dispatch: bool,
    dispatch_refused: Mutex<Option<String>>,
    /// What went wrong while starting, if anything.
    errors: Mutex<Vec<String>>,
}

impl Common {
    fn error(&self, message: String) {
        if let Ok(mut errors) = self.errors.lock() {
            errors.push(message);
        }
    }

    /// Have the kernel stop the system calls the calling thread makes
    /// in its compute phase, as the options say.
    fn watch_calls(&self, who: &str) {
        if !self.dispatch {
            return;
        }
        if let Err(error) = dispatch::enable_here() {
            if self.realtime == Realtime::Required {
                self.error(format!(
                    "{who}: cannot have its system calls stopped in the compute phase: {error}"
                ));
            }
            if let Ok(mut refused) = self.dispatch_refused.lock() {
                refused.get_or_insert_with(|| format!("{who}: {error}"));
            }
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
    let with_card = options.backend == BackendKind::Alsa;
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
    // ---- The graph: read, checked and given to the threads here, with
    // all the time it takes.
    let compiled = match &options.graph {
        Some(file) => Some(crate::graph::load(file, threads, format)?),
        None => None,
    };
    let on_the_card = |wanted: fn(&Kind) -> Option<u32>| -> Vec<usize> {
        compiled
            .iter()
            .flat_map(|compiled| &compiled.nodes)
            .filter_map(|node| wanted(&node.kind))
            .map(|channel| channel as usize - 1)
            .collect()
    };

    // ---- The handler of the system calls the kernel stops in the
    // compute phase: one for the process, before any thread exists.
    let (dispatch, handler_refused) = match options.dispatch.then(dispatch::install) {
        None => (false, None),
        Some(Ok(())) => (true, None),
        Some(Err(error)) if options.realtime == Realtime::Required => {
            return Err(format!(
                "cannot have the system calls of the compute phase stopped: {error}"
            ));
        }
        Some(Err(error)) => (false, Some(error.to_string())),
    };

    let memory_locked = match cpu::lock_memory() {
        Ok(()) => true,
        Err(error) if options.realtime == Realtime::Required => {
            return Err(format!("cannot lock the memory of the process: {error}"));
        }
        Err(_) => false,
    };

    // ---- The sound card: opened here, before any thread exists. It is
    // slow work, and what it finds decides whether the run can start.
    let card = match (&options.device, with_card) {
        (Some(device), true) => Some(AlsaShared::open(CardConfig {
            capture: options
                .capture_device
                .clone()
                .unwrap_or_else(|| device.clone()),
            playback: device.clone(),
            format,
            periods: options.periods,
        })?),
        _ => None,
    };
    // With more live threads than one, the others wake on the timer of
    // the input stream: better to know now that it is there.
    if let Some(card) = &card {
        if threads > 1 {
            card.followers_can_wake().map_err(|error| {
                format!("the other live threads cannot wake with the card ({error}): --threads 1 runs without them")
            })?;
        }
    }
    let patch = match &card {
        Some(card) => Some(Mutex::new(Patch::new(
            &PatchConfig {
                tone_output: options.tone,
                tone_hz: tone_hz(options, format),
                tone_db: -f64::from(options.tone_below_db),
                count: options.count,
                // The time of the start, in steps of a microsecond: no
                // two runs in a row have the same.
                count_key: (clock::now() >> 10) as u16,
                bursts: options.bursts.map(|(output, input)| {
                    let kind = if options.impulses {
                        BurstKind::Impulse
                    } else {
                        BurstKind::Noise
                    };
                    (output, input, kind)
                }),
                seconds: options.seconds,
                routes: options.routes.clone(),
                listen: options.listen.clone(),
                keep_seconds: if options.each_second {
                    options.seconds.min(3600) as usize
                } else {
                    0
                },
                warm_up_seconds: u32::try_from(options.warm_up_s).unwrap_or(u32::MAX),
                graph_inputs: on_the_card(|kind| match kind {
                    Kind::Input { channel } => Some(*channel),
                    _ => None,
                }),
                graph_outputs: on_the_card(|kind| match kind {
                    Kind::Output { channel } => Some(*channel),
                    _ => None,
                }),
            },
            card.info().inputs,
            card.info().outputs,
            format.frames as usize,
            format.rate,
        )?)),
        None => None,
    };

    let fault_cycle = options.fault_after_s * 1_000_000_000 / period_ns;
    // With a card, what it has left to play bounds the compute budget.
    let card_frames = if patch.is_some() { format.frames } else { 0 };
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
        frames: card_frames,
        dry_at: AtomicU64::new(0),
        dry_cycle: AtomicU64::new(0),
        fault: options.fault,
        fault_thread: options.fault_thread,
        fault_cycle,
        fault_made: AtomicBool::new(false),
        patch,
        graph: compiled.as_ref().map(Running::new),
        judged_from_ns: AtomicU64::new(0),
        graph_refused_early: AtomicU64::new(u64::MAX),
    });
    let spares: Arc<Vec<Spare>> = Arc::new((0..threads).map(|_| Spare::default()).collect());
    let events: Arc<Vec<Event>> = Arc::new((0..MAX_EVENTS).map(|_| Event::default()).collect());
    let heartbeat = Arc::new(AtomicU64::new(0));

    // ---- The trace of the kernel on the live CPUs, when asked for:
    // set up before the instant of the first period is fixed (it takes
    // the kernel some tenths of a second), read by the meter.
    // With --trace-also, other CPUs too: a list, or a domain of the
    // boot profile by its name.
    let also_traced: Vec<usize> = match options.trace_also.as_deref() {
        Some(asked) => {
            let named = asked.chars().all(|letter| letter.is_ascii_alphabetic());
            let found = if asked == "all" {
                // Every CPU that is on.
                let online = std::fs::read_to_string("/sys/devices/system/cpu/online")
                    .map_err(|error| format!("--trace-also all: {error}"))?;
                cpus(Some(online.trim()), "")
            } else if named {
                cpus(None, asked)
            } else {
                cpus(Some(asked), "")
            }
            .map_err(|error| format!("--trace-also: {error}"))?;
            if found.is_empty() {
                return Err(format!(
                    "--trace-also: the boot profile has no CPUs called '{asked}'"
                ));
            }
            found
                .into_iter()
                .filter(|cpu| !live_cpus.contains(cpu))
                .collect()
        }
        None => Vec::new(),
    };
    let tracer = if options.kernel_entries {
        if live_cpus.len() < threads {
            return Err(
                "--kernel-entries: the live CPUs are not known (no boot profile, no --live)"
                    .to_string(),
            );
        }
        let mut traced = live_cpus[..threads].to_vec();
        traced.extend(&also_traced);
        Some(Tracer::start(&traced).map_err(|error| format!("--kernel-entries: {error}"))?)
    } else {
        None
    };

    // The periods start a little from now: time for every thread to be
    // in place and waiting.
    let start_ns = clock::now() + 300_000_000;
    if let Some(card) = &card {
        card.set_start(start_ns);
        card.set_warm_up(options.warm_up_s * 1_000_000_000);
        // No live thread runs yet: they read it from their first cycle.
        shared.judged_from_ns.store(
            start_ns + options.warm_up_s * 1_000_000_000,
            Ordering::Release,
        );
    }
    let common = Arc::new(Common {
        shared: Arc::clone(&shared),
        spares: Arc::clone(&spares),
        card: card.clone(),
        format,
        start_ns,
        live_cpus: live_cpus.clone(),
        system_cpus: system_cpus.clone(),
        priority: options.priority,
        realtime: options.realtime,
        ready: AtomicUsize::new(0),
        without_realtime: AtomicUsize::new(0),
        dispatch,
        dispatch_refused: Mutex::new(None),
        errors: Mutex::new(Vec::new()),
    });

    // ---- The record of every cycle: a ring for each thread that may
    // run the cycle, spare ones too, and the meter that reads them all
    // from the system CPUs at normal priority.
    let mut readers = Vec::new();
    let mut ring_for = |slot: usize, spare: bool| {
        options.record.then(|| {
            let (writer, reader) = ring(RING_RECORDS);
            readers.push((slot, spare, reader));
            writer
        })
    };

    // ---- The spare threads: parked outside the live CPUs.
    let mut handles: Vec<(usize, bool, JoinHandle<Option<ThreadReport>>)> = Vec::new();
    for index in 0..threads {
        let common = Arc::clone(&common);
        let record = ring_for(index, true);
        let handle = spawn(format!("spare-{index}"), move || {
            spare_thread(&common, index, record)
        })?;
        handles.push((index, true, handle));
    }

    // ---- The live threads: one per live CPU.
    for index in 0..threads {
        let common = Arc::clone(&common);
        let record = ring_for(index, false);
        let handle = spawn(format!("live-{index}"), move || {
            Some(live_thread(&common, index, record))
        })?;
        handles.push((index, false, handle));
    }

    let records_done = Arc::new(AtomicBool::new(false));
    let meter = if options.record {
        let meter = Meter::new(
            readers,
            Shape {
                period_ns,
                frames: format.frames,
                periods: if card.is_some() { options.periods } else { 0 },
                threads,
                start_ns,
                // On the timer there is no warm-up to ask for, and the
                // first cycle is the one after the threads were set up:
                // it is counted apart, as a warm-up of one period.
                warm_up_ns: if card.is_some() {
                    options.warm_up_s * 1_000_000_000
                } else {
                    period_ns
                },
            },
        );
        let meter = match tracer {
            Some(tracer) => meter.with_trace(tracer, &common.live_cpus[..threads], &also_traced),
            None => meter,
        };
        let common = Arc::clone(&common);
        let done = Arc::clone(&records_done);
        Some(spawn("meter".to_string(), move || {
            if !common.system_cpus.is_empty() {
                let _ = cpu::set_affinity(0, &common.system_cpus);
            }
            meter::collect(meter, &done)
        })?)
    } else {
        None
    };

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

    // ---- The keeper of the card: on the system CPUs, at normal
    // priority. It closes a card that was given up and opens it again.
    let keeper = match &card {
        Some(card) => {
            let card = Arc::clone(card);
            let common = Arc::clone(&common);
            Some(spawn("card-keeper".to_string(), move || {
                if !common.system_cpus.is_empty() {
                    let _ = cpu::set_affinity(0, &common.system_cpus);
                }
                keeper_loop(&card);
            })?)
        }
        None => None,
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
        if let Some(card) = &card {
            card.stop();
        }
        for spare in spares.iter() {
            spare.gate.open();
        }
        records_done.store(true, Ordering::Release);
        // The meter ends, and with it the trace of the kernel.
        if let Some(handle) = meter {
            let _ = handle.join();
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
    // Every thread that can still write has ended: the meter reads what
    // is left and sums up.
    records_done.store(true, Ordering::Release);
    let meter = meter.and_then(|handle| handle.join().ok());
    let kernel_probe = probe.and_then(|handle| handle.join().ok());
    let card = card.map(|card| {
        let in_use_at_the_end = card.has_card();
        card.stop();
        if let Some(keeper) = keeper {
            let _ = keeper.join();
        }
        let stats = card.stats();
        let count = |counter: &AtomicU64| counter.load(Ordering::Acquire);
        CardOutcome {
            info: card.info().clone(),
            card_cycles: count(&stats.card_cycles),
            silent_cycles: count(&stats.silent_cycles),
            xruns: count(&stats.xruns),
            early_xruns: count(&stats.early_xruns),
            recoveries: count(&stats.recoveries),
            longest_recovery_ms: count(&stats.longest_recovery_ns) as f64 / 1e6,
            stalls: count(&stats.stalls),
            lost: count(&stats.lost),
            back: count(&stats.back),
            reopen_attempts: count(&stats.reopen_attempts),
            paused_restarts: count(&stats.paused_restarts),
            short_writes: count(&stats.short_writes),
            bad_stamps: count(&stats.bad_stamps),
            follower_cycles: count(&stats.follower_cycles),
            follower_behind: count(&stats.follower_behind),
            follower_empty: count(&stats.follower_empty),
            timers_opened: count(&stats.timers_opened),
            timers_refused: count(&stats.timers_refused),
            last_error: stats.last_error.load(Ordering::Acquire),
            in_use_at_the_end,
            xrun_log: card
                .xrun_log()
                .into_iter()
                .map(|(since, view)| (since as f64 / 1e9, view))
                .collect(),
            heard: shared
                .patch
                .as_ref()
                .and_then(|patch| patch.try_lock().ok().map(|patch| patch.heard()))
                .unwrap_or_default(),
            counted: shared
                .patch
                .as_ref()
                .and_then(|patch| patch.try_lock().ok().and_then(|patch| patch.counted())),
            // The run is over: the comparing of the bursts is done here,
            // on the thread that only kept time.
            timed: shared
                .patch
                .as_ref()
                .and_then(|patch| patch.try_lock().ok().and_then(|patch| patch.timed())),
        }
    });

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
        dispatch: {
            let refused = handler_refused.or_else(|| {
                common
                    .dispatch_refused
                    .lock()
                    .ok()
                    .and_then(|refused| refused.clone())
            });
            match refused {
                Some(why) => Watched::Refused(why),
                None if options.dispatch => Watched::On,
                None => Watched::Off,
            }
        },
        options: options.clone(),
        reports,
        never_returned,
        events,
        watchdog_rounds: heartbeat.load(Ordering::Acquire),
        systemd: notifier.active(),
        kernel_probe,
        card,
        start_ns,
        meter,
        graph: compiled
            .as_ref()
            .zip(shared.graph.as_ref())
            .map(|(compiled, graph)| {
                // Never marked: the run ended in the warm-up, all of it.
                let early = shared
                    .graph_refused_early
                    .load(Ordering::Acquire)
                    .min(graph.refused());
                GraphOutcome {
                    file: options.graph.clone().unwrap_or_default(),
                    summary: compiled.summary(options.margin_us),
                    nodes: compiled
                        .nodes
                        .iter()
                        .filter(|node| !matches!(node.kind, Kind::Input { .. }))
                        .count(),
                    refused: graph.refused() - early,
                    refused_early: early,
                    peaks: graph.peaks(),
                }
            }),
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
fn live_thread(common: &Common, index: usize, mut record: Option<Writer>) -> ThreadReport {
    if let Some(&cpu) = common.live_cpus.get(index) {
        if let Err(error) = cpu::set_affinity(0, &[cpu]) {
            common.error(format!(
                "live thread {index}: cannot run on CPU {cpu}: {error}"
            ));
        }
    }
    common.take_priority(common.priority, &format!("live thread {index}"));
    common.watch_calls(&format!("live thread {index}"));
    let slot = &common.shared.slots[index];
    slot.take(cpu::thread_id());
    let mut backend = backend(common, index, false);
    common.ready.fetch_add(1, Ordering::AcqRel);
    live_loop(
        &common.shared,
        index,
        slot.generation(),
        None,
        backend.as_mut(),
        record.as_mut(),
    )
}

/// The back end of the thread that holds a slot. With a sound card: the
/// card itself for the first slot, and for the others a sleep on the
/// same card and the cycle of the first. Without one: a timer.
/// `joining`: the periods have already begun.
fn backend(common: &Common, slot: usize, joining: bool) -> Box<dyn Backend> {
    match &common.card {
        Some(card) if slot == 0 => Box::new(AlsaBackend::new(Arc::clone(card))),
        // The others sleep on the card too, and follow the first.
        Some(card) => Box::new(AlsaFollower::new(Arc::clone(card), joining)),
        _ if joining => Box::new(NullBackend::joining(common.format, common.start_ns)),
        _ => Box::new(NullBackend::with_start(common.format, common.start_ns)),
    }
}

/// A spare thread: parked on the system CPUs until the watchdog opens its
/// gate, having already moved it to a live CPU and given it the priority.
fn spare_thread(common: &Common, index: usize, mut record: Option<Writer>) -> Option<ThreadReport> {
    if !common.system_cpus.is_empty() {
        if let Err(error) = cpu::set_affinity(0, &common.system_cpus) {
            common.error(format!(
                "spare thread {index}: cannot run on the system CPUs: {error}"
            ));
        }
    }
    common.watch_calls(&format!("spare thread {index}"));
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
    let mut backend = backend(common, slot_index, true);
    Some(live_loop(
        &common.shared,
        slot_index,
        generation,
        Some(&spare.first_cycle_ns),
        backend.as_mut(),
        record.as_mut(),
    ))
}

/// The answer to "was the fault handled?".
enum Verdict {
    NotAsked,
    Pass(String),
    Fail(String),
}

/// The answer to "was the compute phase free of system calls?", and,
/// when one is made on purpose, "was it stopped, named and carried
/// out?".
fn compute_verdict(outcome: &Outcome) -> Verdict {
    if outcome.dispatch != Watched::On {
        return Verdict::NotAsked;
    }
    let caught: u64 = outcome.reports.iter().map(|report| report.caught).sum();
    if outcome.options.fault != Fault::Syscall {
        return if caught == 0 {
            Verdict::Pass("no system call in the compute phase".to_string())
        } else {
            Verdict::Fail(format!("{caught} system calls in the compute phase"))
        };
    }
    // The call made on purpose: every one of them stopped and carried
    // out, on the thread asked, and no other call anywhere.
    let thread = outcome.options.fault_thread;
    let of_the_thread = |count: fn(&ThreadReport) -> u64| -> u64 {
        outcome
            .reports
            .iter()
            .filter(|report| report.slot == thread)
            .map(count)
            .sum()
    };
    let made = of_the_thread(|report| report.calls_on_purpose);
    let stopped = of_the_thread(|report| report.caught);
    let as_asked = outcome
        .reports
        .iter()
        .flat_map(|report| &report.caught_calls)
        .all(|call| call.number == 24 && call.carried_out);
    let text = format!(
        "{made} system calls made on purpose in the compute phase of thread {thread}, {stopped} stopped and carried out; {} stopped elsewhere",
        caught - stopped
    );
    if made > 0 && stopped == made && caught == stopped && as_asked {
        Verdict::Pass(text)
    } else {
        Verdict::Fail(text)
    }
}

/// Whether every cycle computed the whole graph (step P1.7c).
///
/// A run that is meant to be healthy fails for a cycle that did not get
/// to the end of the graph, and for a buffer or a node that was in use
/// when a thread asked for it (the cycles of two threads overlapped: one
/// was a period behind). Not so a run with a fault made on purpose, nor
/// one where xruns are allowed: a virtual machine does not keep the
/// time, and there it is enough that the graph was run.
fn graph_verdict(outcome: &Outcome) -> Verdict {
    let Some(graph) = &outcome.graph else {
        return Verdict::NotAsked;
    };
    let sum = |of: fn(&ThreadReport) -> u64| outcome.reports.iter().map(of).sum::<u64>();
    let cycles = outcome
        .reports
        .iter()
        .filter(|report| report.slot == 0)
        .map(|report| report.cycles)
        .sum::<u64>();
    let run = sum(|report| report.graph_nodes);
    let cut = sum(|report| report.graph_cut);
    let starved = sum(|report| report.graph_starved);
    let silent = sum(|report| report.graph_silent);
    if run == 0 {
        return Verdict::Fail("no node of the graph was ever run".to_string());
    }
    let mut what = format!(
        "{run} nodes run in {cycles} cycles of {} nodes; {cut} cycles cut, {starved} of them waiting for another thread; {silent} outputs left silent for a cycle; {} times a buffer or a node was in use by another thread",
        graph.nodes, graph.refused
    );
    let early = sum(|report| report.graph_cut_early);
    if early + graph.refused_early > 0 {
        what.push_str(&format!(
            "; in the warm-up {early} cycles cut, {} times in use",
            graph.refused_early
        ));
    }
    let tolerant = outcome.options.allow_xruns || outcome.options.fault != Fault::None;
    if !tolerant && (cut > 0 || silent > 0 || graph.refused > 0) {
        return Verdict::Fail(what);
    }
    Verdict::Pass(what)
}

fn verdict(outcome: &Outcome) -> Verdict {
    let period_ms = outcome.format.period_ns() as f64 / 1e6;
    // Three periods stuck, one of grain of the watchdog, half a
    // millisecond of slack: 8.5 ms with periods of 2 ms.
    let detect_limit_ms = (STUCK_PERIODS + 1) as f64 * period_ms + 0.5;
    match outcome.options.fault {
        // The faults of the card are judged with the sound.
        // The system call made on purpose has a verdict of its own.
        Fault::None | Fault::CardXrun | Fault::CardLost | Fault::Syscall => Verdict::NotAsked,
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

/// The frequency of the tone: so many whole cycles in a quantum when
/// asked that way, hertz otherwise.
fn tone_hz(options: &RunOptions, format: Format) -> f64 {
    match options.tone_cycles {
        Some(cycles) => f64::from(cycles) * f64::from(format.rate) / f64::from(format.frames),
        None => f64::from(options.tone_hz),
    }
}

/// Milliseconds a number of frames lasts.
fn frames_ms(frames: f64, rate: u32) -> f64 {
    frames * 1000.0 / f64::from(rate)
}

/// The middle value of a list (0 for an empty one).
fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted.get(sorted.len() / 2).copied().unwrap_or(0.0)
}

/// The answer to "did the sound card do what was asked?".
fn audio_verdict(outcome: &Outcome) -> Verdict {
    let Some(card) = &outcome.card else {
        return Verdict::NotAsked;
    };
    let options = &outcome.options;
    let mut good = Vec::new();
    let mut bad = Vec::new();

    if !card.in_use_at_the_end || card.back != card.lost {
        bad.push("the card was not in use at the end".to_string());
    }
    if card.recoveries + card.lost < card.xruns {
        bad.push(format!(
            "{} xruns, {} recovered",
            card.xruns, card.recoveries
        ));
    }
    match options.fault {
        Fault::CardXrun => {
            if card.xruns >= 1 && card.recoveries == card.xruns && card.lost == 0 {
                good.push(format!(
                    "the xrun asked for was recovered in {:.2} ms",
                    card.longest_recovery_ms
                ));
            } else {
                bad.push("the xrun asked for did not happen, or was not recovered".to_string());
            }
        }
        Fault::CardLost => {
            if card.lost >= 1 && card.back == card.lost {
                good.push("the card taken away came back".to_string());
            } else {
                bad.push("the card taken away did not come back".to_string());
            }
        }
        // A thread stuck for three periods and more is an xrun by
        // design: what matters is that the card came back.
        Fault::StuckNode | Fault::StuckThread => {
            good.push(format!("{} xruns after the stuck thread", card.xruns));
        }
        _ if options.allow_xruns => good.push(format!("{} xruns, allowed", card.xruns)),
        _ => {
            // The xruns of the warm-up are those of an interface that is
            // settling: reported, not judged.
            let judged = card.xruns - card.early_xruns.min(card.xruns);
            if judged == 0 && card.stalls == 0 && card.lost == 0 && card.short_writes == 0 {
                good.push(if card.early_xruns == 0 {
                    "no xrun".to_string()
                } else {
                    format!("no xrun after the warm-up ({} in it)", card.early_xruns)
                });
            } else {
                bad.push(format!(
                    "{judged} xruns after the warm-up, {} waits without a period, card lost {} times, {} short writes",
                    card.stalls, card.lost, card.short_writes
                ));
            }
        }
    }

    // With a tone, every input listened to must carry it; in every
    // second, unless the run was disturbed on purpose.
    let disturbed = options.allow_xruns
        || !matches!(
            options.fault,
            Fault::None | Fault::WatchdogOff | Fault::Syscall
        );
    if options.tone.is_some() {
        for heard in &card.heard {
            let input = heard.channel + 1;
            // A slip is a fault of its own kind: no xrun, and the signal
            // is not what was played. Judged only in a run that was not
            // disturbed on purpose, and said first: the second it falls
            // in is not a clean one, and that is the same fault.
            if !disturbed && heard.largest_slip > SLIP_LIMIT_SAMPLES {
                bad.push(format!(
                    "a slip of {:.0} samples on input {input}",
                    heard.largest_slip
                ));
                continue;
            }
            let found = if disturbed {
                heard.seconds > 0 && heard.tone_max_db >= TONE_HEARD_DB
            } else {
                heard.seconds > 0
                    && heard.tone_min_db >= TONE_HEARD_DB
                    && heard.share_min >= TONE_CLEAN_SHARE
            };
            if found {
                good.push(format!("the tone is on input {input}"));
                if !disturbed && heard.slip_count > 0 {
                    good.push(format!(
                        "{} slips of at most {:.0} samples on input {input} (open point A-58)",
                        heard.slip_count,
                        heard.largest_slip.round()
                    ));
                }
            } else {
                bad.push(format!("the tone is not on input {input}"));
            }
        }
    }

    // The count: found, and whole after the warm-up.
    if let Some(counted) = &card.counted {
        let input = counted.input + 1;
        if counted.locked_at.is_none() {
            bad.push(format!("the count never came back on input {input}"));
        } else if !disturbed && counted.largest_slip as f64 > SLIP_LIMIT_SAMPLES {
            bad.push(format!(
                "a slip of {} samples in the count on input {input}",
                counted.largest_slip
            ));
        } else if !disturbed && counted.slips > 0 {
            good.push(format!(
                "{} slips of at most {} samples in the count on input {input} (open point A-58)",
                counted.slips, counted.largest_slip
            ));
        } else {
            good.push(format!(
                "the count comes back on input {input} after {} samples",
                counted.last
            ));
        }
    }
    // The bursts: some, and all of them recognised.
    if let Some(timed) = &card.timed {
        let recognised = timed
            .each_match
            .iter()
            .fold(f64::INFINITY, |least, &likeness| least.min(likeness));
        let floor = if options.impulses { 0.001 } else { 0.5 };
        if timed.each_burst.is_empty() {
            bad.push("no burst was played whole".to_string());
        } else if recognised < floor {
            bad.push(format!(
                "the bursts do not come back on input {}",
                timed.input + 1
            ));
        } else {
            good.push(format!(
                "the bursts come back on input {} after {:.2} samples",
                timed.input + 1,
                median(&timed.each_burst)
            ));
        }
    }

    if bad.is_empty() {
        Verdict::Pass(good.join("; "))
    } else {
        Verdict::Fail(bad.join("; "))
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
    println!(
        "backend: {}",
        if outcome.card.is_some() {
            "alsa"
        } else {
            "null"
        }
    );
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
    println!(
        "syscall_dispatch: {}",
        match &outcome.dispatch {
            Watched::On => "on (every thread of the cycle tried it at its start)".to_string(),
            Watched::Off => "off (--no-dispatch)".to_string(),
            Watched::Refused(why) => format!("refused ({why})"),
        }
    );
    let caught: u64 = outcome.reports.iter().map(|report| report.caught).sum();
    println!("syscalls_stopped_in_compute: {caught}");
    for report in &outcome.reports {
        for call in &report.caught_calls {
            println!(
                "  slot {}{} cycle {} {}: {}, from address {:#x}; {}",
                report.slot,
                if report.spare { " (spare)" } else { "" },
                call.cycle,
                call.node.map_or_else(
                    || "outside any node".to_string(),
                    |node| format!("node {node}")
                ),
                crate::ktrace::syscall_name(call.number)
                    .map_or_else(|| format!("system call {}", call.number), str::to_string),
                call.address,
                if call.carried_out {
                    "carried out"
                } else {
                    "refused"
                }
            );
        }
        if report.caught > report.caught_calls.len() as u64 {
            println!(
                "  slot {}{}: ... and {} more",
                report.slot,
                if report.spare { " (spare)" } else { "" },
                report.caught - report.caught_calls.len() as u64
            );
        }
    }
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
            Fault::CardXrun => format!("card-xrun after {} s", options.fault_after_s),
            Fault::CardLost => format!("card-lost after {} s", options.fault_after_s),
            Fault::Syscall => format!(
                "a system call in every compute phase of thread {} after {} s",
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
        "{:<6} {:<6} {:<8} {:>8} {:>7} {:>10} {:>8} {:>9} {:>9} {:>11} {:>11} {:>12} {:>7}",
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
        "least_margin",
        "behind"
    );
    let mut reports: Vec<&ThreadReport> = outcome.reports.iter().collect();
    reports.sort_by_key(|report| (report.slot, report.spare));
    for report in reports {
        println!(
            "{:<6} {:<6} {:<8} {:>8} {:>7} {:>10} {:>8} {:>9} {:>9} {:>11} {:>11} {:>12} {:>7}",
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
            report.behind,
        );
    }
    for slot in &outcome.never_returned {
        println!(
            "{slot:<6} {:<6} never came back: stuck, moved off its CPU by the watchdog",
            "first"
        );
    }

    if let Some(graph) = &outcome.graph {
        print_graph(graph, outcome);
    }

    if let Some(card) = &outcome.card {
        print_card(card, options);
    }

    match &outcome.meter {
        Some(report) => meter::print(report, outcome.start_ns, outcome.format.period_ns()),
        None => {
            println!();
            println!("record_of_every_cycle: not kept");
        }
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

/// The graph of the run: what the compiler made of it, then what the
/// threads did with it.
fn print_graph(graph: &GraphOutcome, outcome: &Outcome) {
    println!();
    println!("graph_file: {}", graph.file);
    print!("{}", graph.summary);
    println!();
    println!("the graph in the cycles (times in microseconds; cut, starved and silent after the warm-up):");
    println!(
        "{:<6} {:<6} {:>10} {:>8} {:>9} {:>9} {:>6} {:>8} {:>7} {:>10} {:>13}",
        "slot",
        "kind",
        "nodes_run",
        "waits",
        "wait_avg",
        "wait_max",
        "cut",
        "starved",
        "silent",
        "cut_early",
        "silent_early"
    );
    let mut reports: Vec<&ThreadReport> = outcome.reports.iter().collect();
    reports.sort_by_key(|report| (report.slot, report.spare));
    for report in reports {
        println!(
            "{:<6} {:<6} {:>10} {:>8} {:>9} {:>9} {:>6} {:>8} {:>7} {:>10} {:>13}",
            report.slot,
            if report.spare { "spare" } else { "first" },
            report.graph_nodes,
            report.graph_waits.count(),
            us(report.graph_waits.mean_ns()),
            us(report.graph_waits.max_ns()),
            report.graph_cut,
            report.graph_starved,
            report.graph_silent,
            report.graph_cut_early,
            report.graph_silent_early,
        );
    }
    println!(
        "graph_refused: {} after the warm-up, {} in it (a buffer or a node in use by another thread when one asked for it)",
        graph.refused, graph.refused_early
    );
    for (name, peak) in &graph.peaks {
        println!(
            "graph_peak {name}: {} dBFS",
            decibels(20.0 * f64::from(*peak).log10())
        );
    }
}

fn decibels(value: f64) -> String {
    if value.is_finite() {
        format!("{value:.1}")
    } else {
        "-".to_string()
    }
}

fn print_counted(counted: &Counted, rate: u32) {
    let input = counted.input + 1;
    match counted.locked_at {
        None => println!(
            "count_on_input_{input}: never found ({} samples read)",
            counted.out_of_place + counted.in_place
        ),
        Some(second) => {
            println!(
                "count_on_input_{input}: way round {} samples ({:.3} ms), found at {second:.3} s; at the end {} samples ({:.3} ms)",
                counted.first,
                frames_ms(counted.first as f64, rate),
                counted.last,
                frames_ms(counted.last as f64, rate),
            );
            println!(
                "count_slips_on_input_{input}: {} (largest {} samples); in the warm-up {} (largest {})",
                counted.slips,
                counted.largest_slip,
                counted.early_slips,
                counted.largest_early_slip
            );
            println!(
                "count_samples_on_input_{input}: {} in place, {} out of place",
                counted.in_place, counted.out_of_place
            );
        }
    }
    for (second, change) in &counted.each_slip {
        println!("  count slip on input {input} at {second:.3} s: {change:+} samples");
    }
}

fn print_timed(timed: &Timed, rate: u32, impulses: bool) {
    let name = format!("bursts_{}_to_{}", timed.output + 1, timed.input + 1);
    if timed.each_burst.is_empty() {
        println!("{name}: none played whole");
        return;
    }
    let least = timed
        .each_burst
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let most = timed
        .each_burst
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let middle = median(&timed.each_burst);
    let recognised = timed
        .each_match
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    println!(
        "{name}: {} {}; way round {least:.2} to {most:.2} samples, median {middle:.2} ({:.3} ms); {} at least {recognised:.4}",
        timed.each_burst.len(),
        if impulses { "impulses" } else { "bursts of noise" },
        frames_ms(middle, rate),
        if impulses { "peak" } else { "match" },
    );
}

fn print_card(card: &CardOutcome, options: &RunOptions) {
    let info = &card.info;
    println!();
    println!("card_capture: {}", info.capture);
    println!("card_playback: {}", info.playback);
    println!(
        "card_format: S32_LE, {} Hz, {} inputs, {} outputs",
        info.rate, info.inputs, info.outputs
    );
    println!(
        "card_period_frames: {} (buffer: capture {}, playback {})",
        info.period, info.capture_buffer, info.playback_buffer
    );
    println!("card_linked: {}", if info.linked { "yes" } else { "no" });
    println!(
        "card_period_stamps: {}",
        if info.timestamps {
            "yes (the wake-up delay is counted from the stamp of the period)"
        } else {
            "no (no wake-up delay)"
        }
    );
    println!(
        "card_cycles: {} with the card, {} without",
        card.card_cycles, card.silent_cycles
    );
    println!("warm_up_seconds: {}", options.warm_up_s);
    println!(
        "card_xruns: {} ({} of them in the warm-up)",
        card.xruns, card.early_xruns
    );
    println!(
        "card_recoveries: {} (longest {:.2} ms)",
        card.recoveries, card.longest_recovery_ms
    );
    println!(
        "card_restarts_after_a_pause: {} (the start before had given no input)",
        card.paused_restarts
    );
    println!("card_waits_without_period: {}", card.stalls);
    println!(
        "card_lost: {} (last error number {}), back {}, attempts to open it again {}",
        card.lost, card.last_error, card.back, card.reopen_attempts
    );
    for (index, (second, view)) in card.xrun_log.iter().enumerate() {
        println!(
            "card_xrun_{}: at {:.3} s; capture {}, {} frames to read (most ever {}); playback {}, {} frames of room (most ever {}), delay {}",
            index + 1,
            second,
            view.capture_state,
            view.capture_avail,
            view.capture_avail_max,
            view.playback_state,
            view.playback_avail,
            view.playback_avail_max,
            view.playback_delay
        );
    }
    println!("card_short_writes: {}", card.short_writes);
    println!("card_stamps_not_believed: {}", card.bad_stamps);
    println!(
        "card_followers: {} cycles after a tick of the card, {} of them with periods gone by, {} wake-ups with no cycle; timers of the card opened {}, refused {}",
        card.follower_cycles,
        card.follower_behind,
        card.follower_empty,
        card.timers_opened,
        card.timers_refused
    );
    match options.tone {
        Some(output) => println!(
            "tone: output {}, {:.2} Hz, peak {} dB below full scale",
            output + 1,
            tone_hz(
                options,
                Format {
                    rate: info.rate,
                    frames: info.period as u32
                }
            ),
            options.tone_below_db
        ),
        None => println!("tone: none"),
    }
    if let Some(counted) = &card.counted {
        print_counted(counted, info.rate);
    }
    if let Some(timed) = &card.timed {
        print_timed(timed, info.rate, options.impulses);
    }
    println!(
        "routes: {}",
        if options.routes.is_empty() {
            "none".to_string()
        } else {
            options
                .routes
                .iter()
                .map(|(input, output)| format!("{}:{}", input + 1, output + 1))
                .collect::<Vec<_>>()
                .join(",")
        }
    );
    if card.heard.is_empty() {
        return;
    }
    println!();
    println!(
        "inputs listened to (dB relative to full scale; the first {} s and the last, incomplete second left out):",
        options.warm_up_s.max(1)
    );
    println!(
        "{:<6} {:>8} {:>8} {:>10} {:>10} {:>9} {:>9} {:>10}",
        "input", "seconds", "peak", "level_min", "level_max", "tone_min", "tone_max", "share_min"
    );
    for heard in &card.heard {
        println!(
            "{:<6} {:>8} {:>8} {:>10} {:>10} {:>9} {:>9} {:>10}",
            heard.channel + 1,
            heard.seconds,
            decibels(heard.peak_db),
            decibels(heard.level_min_db),
            decibels(heard.level_max_db),
            decibels(heard.tone_min_db),
            decibels(heard.tone_max_db),
            if heard.seconds == 0 {
                "-".to_string()
            } else {
                format!("{:.4}", heard.share_min)
            },
        );
    }
    for heard in &card.heard {
        println!(
            "slips_on_input_{}: {} (largest {:.2} samples); in the warm-up {} (largest {:.2})",
            heard.channel + 1,
            heard.slip_count,
            heard.largest_slip,
            heard.early_slip_count,
            heard.largest_early_slip
        );
        for slip in &heard.slips {
            println!(
                "  slip on input {} at {:.3} s: {:+.2} samples",
                heard.channel + 1,
                slip.second,
                slip.samples
            );
        }
    }
    if card.heard.iter().all(|heard| heard.each_second.is_empty()) {
        return;
    }
    println!();
    println!("second by second (the warm-up is not in the table above):");
    println!(
        "{:<6} {:>6} {:>8} {:>8} {:>8}",
        "second", "input", "level", "tone", "share"
    );
    let seconds = card
        .heard
        .iter()
        .map(|heard| heard.each_second.len())
        .max()
        .unwrap_or(0);
    for second in 0..seconds {
        for heard in &card.heard {
            if let Some(&(level, tone, share)) = heard.each_second.get(second) {
                println!(
                    "{:<6} {:>6} {:>8} {:>8} {:>8.4}",
                    second + 1,
                    heard.channel + 1,
                    decibels(level),
                    decibels(tone),
                    share
                );
            }
        }
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

    /// Cycles in a second of these tests: periods of 20 ms.
    const CYCLES_A_SECOND: u64 = 50;

    // No real-time priority: the logic, on whatever machine runs the
    // tests. Times are not judged here.
    fn options(fault: Fault) -> RunOptions {
        RunOptions {
            // A period of 20 ms. Without a real-time priority a thread
            // is kept off its CPU now and then, and the watchdog
            // rightly calls stuck one that makes no progress for three
            // periods. With periods of 5 ms that is 15 ms, and these
            // tests failed on machines that were doing other things
            // (and once on a quiet one); with 20 ms it is 60.
            frames: Some(960),
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

    /// A graph in a file of its own, gone when the test ends.
    struct GraphFile(std::path::PathBuf);

    impl GraphFile {
        fn new(name: &str, text: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "dawos-run-test-{}-{name}.graph",
                std::process::id()
            ));
            std::fs::write(&path, text).unwrap();
            Self(path)
        }

        fn options(&self, threads: usize) -> RunOptions {
            RunOptions {
                graph: Some(self.0.to_str().unwrap().to_string()),
                threads: Some(threads),
                load_percent: 0,
                ..options(Fault::None)
            }
        }
    }

    impl Drop for GraphFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn peak_db(outcome: &Outcome, name: &str) -> f64 {
        let graph = outcome.graph.as_ref().expect("a graph");
        let peak = graph
            .peaks
            .iter()
            .find(|(node, _)| node == name)
            .expect("the meter")
            .1;
        20.0 * f64::from(peak).log10()
    }

    /// A signal handed from thread to thread, every thread waiting for
    /// the one before: it reaches the meter whole, and nothing of the
    /// graph is ever refused.
    #[test]
    fn a_graph_is_run_by_the_threads_it_was_compiled_for() {
        let _alone = alone();
        let file = GraphFile::new(
            "relay",
            "impulse click every=1 db=-20\n\
             gain hop1 db=-1 thread=1\ngain hop2 db=-1 thread=2\ngain hop3 db=-1 thread=0\n\
             peak heard thread=1\noutput out channel=1\n\
             connect click -> hop1 -> hop2 -> hop3 -> heard\nconnect hop3 -> out\n",
        );
        let outcome = execute(&file.options(3)).expect("the run did not start");
        let graph = outcome.graph.as_ref().expect("a graph");
        // Every node but none is an input: six a cycle.
        assert_eq!(graph.nodes, 6);
        // The impulse reached the meter through the three gains, on
        // three threads: at least one cycle was whole. Times are not
        // judged here: on a machine that does other things a thread is
        // kept off its CPU for whole periods, and the cycles of the
        // others are cut while they wait for it.
        assert!((peak_db(&outcome, "heard") + 23.0).abs() < 0.01);
        assert_eq!(outcome.reports.len(), 3);
        for report in &outcome.reports {
            assert!(report.graph_nodes > 0, "slot {}", report.slot);
        }
        // Threads 0 and 2 wait for another thread in every cycle, by
        // the order of their nodes.
        let waited: u64 = outcome
            .reports
            .iter()
            .map(|report| report.graph_waits.count() + report.graph_starved)
            .sum();
        assert!(waited > 0);
        assert!(summary_of(&outcome).contains("graph_nodes: 6"));
    }

    fn summary_of(outcome: &Outcome) -> &str {
        &outcome.graph.as_ref().expect("a graph").summary
    }

    /// A graph with more work than a period holds: the budget cuts the
    /// cycles, the thread that waits for the node that never comes
    /// gives up at its own limit, and the run fails for it.
    #[test]
    fn a_graph_that_does_not_fit_is_cut_and_said() {
        let _alone = alone();
        // 30 ms of load in periods of 20: thread 1 never gets to the
        // gain, and thread 0 waits for it in vain.
        let file = GraphFile::new(
            "heavy",
            "noise n thread=1\nload a ns=15625 thread=1\nload b ns=15625 thread=1\n\
             gain late thread=1\npeak heard thread=0\n\
             connect n -> a:0 -> b:0 -> late -> heard\nconnect n -> a:1 -> b:1\n",
        );
        let outcome = execute(&file.options(2)).expect("the run did not start");
        assert!(summary_of(&outcome).contains("THE GRAPH DOES NOT FIT"));
        let of = |slot: usize| {
            outcome
                .reports
                .iter()
                .find(|report| report.slot == slot)
                .expect("the thread")
        };
        assert!(of(1).incomplete > 0);
        assert!(of(0).graph_starved > 0);
        assert!(matches!(graph_verdict(&outcome), Verdict::Fail(_)));
        // Nothing reached the meter.
        assert_eq!(peak_db(&outcome, "heard"), f64::NEG_INFINITY);
    }

    #[test]
    fn a_graph_with_a_mistake_stops_the_run_before_it_starts() {
        let file = GraphFile::new("wrong", "gain g thread=7\n");
        let error = execute(&file.options(2)).err().expect("an error");
        assert!(error.contains("line 1"), "{error}");
        let mut none = file.options(2);
        none.graph = Some("/nonexistent/x.graph".to_string());
        assert!(execute(&none).is_err());
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
            // The run is ended by a thread that looks at the clock every
            // 100 ms, with sleeps that last a little longer each: up to
            // one step more than the two seconds, and some slack.
            let cycles = report.cycles + report.missed;
            assert!(
                (2 * CYCLES_A_SECOND - 5..=2 * CYCLES_A_SECOND + 10).contains(&cycles),
                "{cycles} cycles in two seconds"
            );
        }
        assert!(outcome.watchdog_rounds > CYCLES_A_SECOND);
    }

    #[test]
    fn the_record_of_every_cycle_agrees_with_the_threads() {
        let _alone = alone();
        let outcome = execute(&options(Fault::None)).expect("the run did not start");
        let meter = outcome.meter.as_ref().expect("the record was not kept");
        assert_eq!(meter.threads.len(), 2);
        for report in &outcome.reports {
            let figures = meter
                .threads
                .iter()
                .find(|figures| figures.slot == report.slot && figures.spare == report.spare)
                .expect("a thread without figures");
            assert_eq!(figures.dropped, 0);
            assert_eq!(figures.cycles + figures.early_cycles, report.cycles);
            assert_eq!(figures.wake_up.max_ns(), report.lateness.max_ns());
            assert_eq!(figures.compute.max_ns(), report.compute.max_ns());
        }
        // Without the record the run is the same, and says so.
        let without = RunOptions {
            record: false,
            ..options(Fault::None)
        };
        let outcome = execute(&without).expect("the run did not start");
        assert!(outcome.meter.is_none());
        assert_eq!(outcome.reports.len(), 2);
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
        // The spare thread wrote its own cycles, under the slot it took.
        let meter = outcome.meter.as_ref().expect("the record was not kept");
        assert!(meter
            .threads
            .iter()
            .any(|figures| figures.spare && figures.slot == 1 && figures.cycles > 0));
        // The other thread went on, and a spare took the slot.
        let other = outcome
            .reports
            .iter()
            .find(|report| report.slot == 0)
            .expect("slot 0");
        assert!(other.cycles > CYCLES_A_SECOND);
        let spare = outcome
            .reports
            .iter()
            .find(|report| report.slot == 1 && report.spare)
            .expect("the spare thread of slot 1");
        assert!(
            spare.cycles > CYCLES_A_SECOND / 3,
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
            .any(|report| report.slot == 1 && report.spare && report.cycles > CYCLES_A_SECOND / 3));
    }

    #[test]
    fn too_much_work_is_cut_by_the_budget() {
        let _alone = alone();
        let mut options = options(Fault::None);
        options.load_percent = 150;
        // These threads want all of a CPU: see `options` for the period.
        let outcome = execute(&options).expect("the run did not start");
        assert!(
            outcome.events.is_empty(),
            "the watchdog acted on a heavy cycle"
        );
        for report in &outcome.reports {
            // Nearly every cycle is cut by the budget, however many of
            // them the machine let the thread run.
            assert!(report.cycles > 0);
            assert!(
                report.incomplete * 10 >= report.cycles * 9,
                "{} incomplete cycles of {}",
                report.incomplete,
                report.cycles
            );
        }
    }

    #[test]
    fn a_system_call_in_the_compute_phase_is_stopped_named_and_carried_out() {
        let _alone = alone();
        let outcome = execute(&options(Fault::Syscall)).expect("the run did not start");
        if let Watched::Refused(why) = &outcome.dispatch {
            // A kernel older than 5.11, or not built for it: nothing to
            // say here.
            eprintln!("the kernel does not stop system calls: {why}");
            return;
        }
        assert_eq!(outcome.dispatch, Watched::On);
        assert!(outcome.events.is_empty());
        // Thread 1 makes one call in every cycle of the second second;
        // thread 0 none.
        let of = |slot: usize| {
            outcome
                .reports
                .iter()
                .find(|report| report.slot == slot)
                .expect("no report for the slot")
        };
        assert_eq!(of(0).caught, 0);
        assert!(of(0).caught_calls.is_empty());
        // Every call made was stopped: not one more, not one less. How
        // many cycles the thread ran in that second depends on the
        // machine.
        assert!(of(1).caught > dispatch::KEPT as u64, "{}", of(1).caught);
        assert_eq!(of(1).calls_on_purpose, of(1).caught);
        assert_eq!(of(1).caught_calls.len(), dispatch::KEPT);
        for call in &of(1).caught_calls {
            // sched_yield, in the first node, and carried out.
            assert_eq!((call.number, call.node), (24, Some(0)));
            assert!(call.carried_out);
            assert!(call.cycle >= CYCLES_A_SECOND);
        }
        assert!(matches!(compute_verdict(&outcome), Verdict::Pass(_)));
        // The record of every cycle has them too.
        let meter = outcome.meter.as_ref().expect("the record was not kept");
        let recorded: u64 = meter.threads.iter().map(|figures| figures.caught).sum();
        assert_eq!(recorded, of(1).caught);
    }

    #[test]
    fn a_healthy_run_makes_no_system_call_in_the_compute_phase() {
        let _alone = alone();
        let outcome = execute(&options(Fault::None)).expect("the run did not start");
        if outcome.dispatch != Watched::On {
            return;
        }
        assert!(outcome.reports.iter().all(|report| report.caught == 0));
        assert!(matches!(compute_verdict(&outcome), Verdict::Pass(_)));
        // Without it nothing is watched, and no verdict is given.
        let outcome = execute(&RunOptions {
            dispatch: false,
            ..options(Fault::None)
        })
        .expect("the run did not start");
        assert_eq!(outcome.dispatch, Watched::Off);
        assert!(matches!(compute_verdict(&outcome), Verdict::NotAsked));
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
