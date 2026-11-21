//! The command line, read by hand: a handful of options do not justify a
//! dependency (question D-18).

use crate::plan::Fault;

/// What the user asked for.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    Run(RunOptions),
}

/// Options of `run`.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RunOptions {
    /// Sample rate in hertz.
    pub rate: u32,
    /// Samples per period; `None`: the quantum of 2.667 ms for the rate.
    pub frames: Option<u32>,
    /// How long to run.
    pub seconds: u64,
    /// CPUs of the live threads; `None`: the live domain of the boot
    /// profile, or no pinning when there is no profile.
    pub live: Option<String>,
    /// CPUs of the watchdog and of the parked spare threads; `None`: the
    /// system domain of the boot profile, or no pinning.
    pub system: Option<String>,
    /// Number of live threads; `None`: one per live CPU, or one.
    pub threads: Option<usize>,
    /// Real-time priority of the live threads (SCHED_FIFO).
    pub priority: u8,
    /// What to do about the real-time priority.
    pub realtime: Realtime,
    /// Microseconds before the end of the period at which a thread stops
    /// taking nodes (the compute budget, ADR 0023).
    pub margin_us: u64,
    /// Share of the period each thread keeps busy, in percent.
    pub load_percent: u64,
    /// The fault to inject, the thread it strikes and after how long.
    pub fault: Fault,
    pub fault_thread: usize,
    pub fault_after_s: u64,
    /// Ask the kernel, again and again, for a piece of work on every
    /// CPU, and time it: the proof that the kernel threads of the live
    /// CPUs get to run.
    pub kernel_probe: bool,
}

/// Whether the cycle must run with a real-time priority.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Realtime {
    /// Ask for it; go on without, saying so, if the kernel refuses. The
    /// default: the same command works on a development PC.
    IfAllowed,
    /// Ask for it and stop if the kernel refuses (`--require-rt`).
    Required,
    /// Do not ask (`--no-rt`).
    Off,
}

pub const USAGE: &str = "\
Usage:
  dawos-engine run [OPTIONS]   run the cycle on the live CPUs and report
  dawos-engine --version
  dawos-engine --help

Options of run:
  --backend NAME     source of the periods: null (a timer; the only one
                     yet)
  --rate HZ          sample rate (default 48000)
  --quantum N        samples per period (default: 128 up to 48 kHz, 256 up
                     to 96 kHz, 512 above: always 2.667 ms)
  --seconds N        how long to run (default 10)
  --live LIST        CPUs of the live threads, like 4-7 (default: the live
                     domain of the boot profile; none without a profile)
  --system LIST      CPUs of the watchdog and of the parked spare threads
                     (default: the system domain of the boot profile)
  --threads N        live threads (default: one per live CPU; one without
                     a profile)
  --priority N       real-time priority of the live threads, 1 to 94
                     (default 80; the watchdog runs at 95)
  --margin-us N      compute budget: stop taking work this long before
                     the end of the period (default 150)
  --load PERCENT     keep each thread busy for this share of the period,
                     0 to 300 (default 0)
  --fault NAME       make something go wrong: stuck-node, stuck-thread,
                     watchdog-off (default: nothing)
  --fault-after N    seconds before the fault (default 3)
  --fault-thread N   the live thread it strikes, from 0 (default 0)
  --kernel-probe     again and again, ask the kernel for a piece of work
                     on every CPU and time it (needs root; it disturbs
                     the live CPUs: for the tests of the protection only)
  --require-rt       stop if the real-time priority or the memory lock is
                     refused, instead of going on without
  --no-rt            do not ask for a real-time priority
";

/// Priority of the engine's live threads unless told otherwise: above
/// the kernel's interrupt threads (50), below the priority planned for
/// the interrupt of the sound card's USB controller (90).
const DEFAULT_PRIORITY: u8 = 80;

/// Margin of the compute budget unless told otherwise (ADR 0023): a
/// starting value, to be replaced by the measured one.
const DEFAULT_MARGIN_US: u64 = 150;

/// Read the arguments after the program name.
pub fn parse(arguments: &[String]) -> Result<Command, String> {
    let mut rest = arguments.iter().map(String::as_str);
    match rest.next() {
        None | Some("--help" | "-h" | "help") => Ok(Command::Help),
        Some("--version" | "-V") => Ok(Command::Version),
        Some("run") => parse_run(rest).map(Command::Run),
        Some(other) => Err(format!("unknown command '{other}'")),
    }
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            rate: 48_000,
            frames: None,
            seconds: 10,
            live: None,
            system: None,
            threads: None,
            priority: DEFAULT_PRIORITY,
            realtime: Realtime::IfAllowed,
            margin_us: DEFAULT_MARGIN_US,
            load_percent: 0,
            fault: Fault::None,
            fault_thread: 0,
            fault_after_s: 3,
            kernel_probe: false,
        }
    }
}

fn parse_run<'a>(mut rest: impl Iterator<Item = &'a str>) -> Result<RunOptions, String> {
    let mut options = RunOptions::default();
    while let Some(option) = rest.next() {
        let mut value = |name: &str| rest.next().ok_or_else(|| format!("{name} needs a value"));
        match option {
            "--backend" => {
                let name = value("--backend")?;
                if name != "null" {
                    return Err(format!("unknown back end '{name}': only 'null' exists yet"));
                }
            }
            "--rate" => options.rate = number(value("--rate")?, "--rate", 8_000, 768_000)?,
            "--quantum" => {
                options.frames = Some(number(value("--quantum")?, "--quantum", 8, 65_536)?)
            }
            "--seconds" => {
                options.seconds = number(value("--seconds")?, "--seconds", 1, 7 * 24 * 3600)?
            }
            "--live" => options.live = Some(value("--live")?.to_string()),
            "--system" => options.system = Some(value("--system")?.to_string()),
            "--threads" => options.threads = Some(number(value("--threads")?, "--threads", 1, 64)?),
            "--priority" => options.priority = number(value("--priority")?, "--priority", 1, 94)?,
            "--margin-us" => {
                options.margin_us = number(value("--margin-us")?, "--margin-us", 0, 100_000)?
            }
            "--load" => options.load_percent = number(value("--load")?, "--load", 0, 300)?,
            "--fault" => {
                options.fault = match value("--fault")? {
                    "none" => Fault::None,
                    "stuck-node" => Fault::StuckNode,
                    "stuck-thread" => Fault::StuckThread,
                    "watchdog-off" => Fault::WatchdogOff,
                    other => {
                        return Err(format!(
                            "unknown fault '{other}': stuck-node, stuck-thread or watchdog-off"
                        ))
                    }
                };
            }
            "--fault-after" => {
                options.fault_after_s = number(value("--fault-after")?, "--fault-after", 1, 3600)?
            }
            "--fault-thread" => {
                options.fault_thread = number(value("--fault-thread")?, "--fault-thread", 0, 63)?
            }
            "--kernel-probe" => options.kernel_probe = true,
            "--require-rt" => options.realtime = Realtime::Required,
            "--no-rt" => options.realtime = Realtime::Off,
            other => return Err(format!("unknown option '{other}'")),
        }
    }
    Ok(options)
}

/// A number between `lowest` and `highest`, or a message naming the
/// option.
fn number<T>(text: &str, name: &str, lowest: u64, highest: u64) -> Result<T, String>
where
    T: TryFrom<u64>,
{
    let value: u64 = text
        .parse()
        .map_err(|_| format!("{name}: '{text}' is not a number"))?;
    if value < lowest || value > highest {
        return Err(format!(
            "{name}: {value} is not between {lowest} and {highest}"
        ));
    }
    T::try_from(value).map_err(|_| format!("{name}: {value} is out of range"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn no_argument_is_a_request_for_help() {
        assert_eq!(parse(&[]), Ok(Command::Help));
        assert_eq!(parse(&arguments("--help")), Ok(Command::Help));
        assert_eq!(parse(&arguments("--version")), Ok(Command::Version));
    }

    #[test]
    fn run_has_defaults() {
        assert_eq!(
            parse(&arguments("run")),
            Ok(Command::Run(RunOptions::default()))
        );
        let options = RunOptions::default();
        assert_eq!(options.rate, 48_000);
        assert_eq!(options.priority, 80);
        assert_eq!(options.margin_us, 150);
        assert_eq!(options.fault, Fault::None);
    }

    #[test]
    fn run_takes_its_options() {
        let text = "run --backend null --rate 96000 --quantum 256 --seconds 60 --live 4-7 --system 16-19 \
                    --threads 2 --priority 70 --margin-us 100 --load 60 --fault stuck-node --fault-after 5 \
                    --fault-thread 1 --kernel-probe --require-rt";
        let Ok(Command::Run(options)) = parse(&arguments(text)) else {
            panic!("run was not understood");
        };
        assert_eq!(options.rate, 96_000);
        assert_eq!(options.frames, Some(256));
        assert_eq!(options.seconds, 60);
        assert_eq!(options.live.as_deref(), Some("4-7"));
        assert_eq!(options.system.as_deref(), Some("16-19"));
        assert_eq!(options.threads, Some(2));
        assert_eq!(options.priority, 70);
        assert_eq!(options.margin_us, 100);
        assert_eq!(options.load_percent, 60);
        assert_eq!(options.fault, Fault::StuckNode);
        assert_eq!(options.fault_after_s, 5);
        assert_eq!(options.fault_thread, 1);
        assert!(options.kernel_probe);
        assert_eq!(options.realtime, Realtime::Required);
    }

    #[test]
    fn mistakes_are_named() {
        for (text, expected) in [
            ("walk", "unknown command"),
            ("run --rate", "needs a value"),
            ("run --rate fast", "not a number"),
            ("run --priority 0", "not between"),
            ("run --priority 95", "not between"),
            ("run --backend alsa", "unknown back end"),
            ("run --fault explode", "unknown fault"),
            ("run --loud", "unknown option"),
        ] {
            let error = parse(&arguments(text)).expect_err(text);
            assert!(error.contains(expected), "'{text}' gave '{error}'");
        }
    }
}
