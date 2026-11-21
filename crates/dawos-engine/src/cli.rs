//! The command line, read by hand: a handful of options do not justify a
//! dependency (question D-18).

/// What the user asked for.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    Run(RunOptions),
}

/// Options of `run`.
#[derive(Debug, PartialEq, Eq)]
pub struct RunOptions {
    /// Sample rate in hertz.
    pub rate: u32,
    /// Samples per period; `None`: the quantum of 2.667 ms for the rate.
    pub frames: Option<u32>,
    /// How long to run.
    pub seconds: u64,
    /// CPU to run on; `None`: the first live CPU of the boot profile, or
    /// wherever the kernel puts the thread when there is no profile.
    pub cpu: Option<usize>,
    /// Real-time priority (SCHED_FIFO).
    pub priority: u8,
    /// What to do about the real-time priority.
    pub realtime: Realtime,
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
  dawos-engine run [OPTIONS]   drive a cycle and report how late each
                               wake-up was
  dawos-engine --version
  dawos-engine --help

Options of run:
  --backend NAME   source of the periods: null (a timer; the only one yet)
  --rate HZ        sample rate (default 48000)
  --quantum N      samples per period (default: 128 up to 48 kHz, 256 up
                   to 96 kHz, 512 above: always 2.667 ms)
  --seconds N      how long to run (default 10)
  --cpu N          CPU to run on (default: the first live CPU of the boot
                   profile; none if there is no profile)
  --priority N     real-time priority, 1 to 99 (default 80)
  --require-rt     stop if the real-time priority or the memory lock is
                   refused, instead of going on without
  --no-rt          do not ask for a real-time priority
";

/// Priority of the engine's live threads unless told otherwise: above
/// the kernel's interrupt threads (50), below the priority planned for
/// the interrupt of the sound card's USB controller (90).
const DEFAULT_PRIORITY: u8 = 80;

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

fn parse_run<'a>(mut rest: impl Iterator<Item = &'a str>) -> Result<RunOptions, String> {
    let mut options = RunOptions {
        rate: 48_000,
        frames: None,
        seconds: 10,
        cpu: None,
        priority: DEFAULT_PRIORITY,
        realtime: Realtime::IfAllowed,
    };
    while let Some(option) = rest.next() {
        let mut value = |name: &str| rest.next().ok_or_else(|| format!("{name} needs a value"));
        match option {
            "--backend" => {
                let name = value("--backend")?;
                if name != "null" {
                    return Err(format!("unknown back end '{name}': only 'null' exists yet"));
                }
            }
            "--rate" => {
                options.rate = number(value("--rate")?, "--rate", 8_000, 768_000)?;
            }
            "--quantum" => {
                options.frames = Some(number(value("--quantum")?, "--quantum", 8, 65_536)?);
            }
            "--seconds" => {
                options.seconds = number(value("--seconds")?, "--seconds", 1, 7 * 24 * 3600)?;
            }
            "--cpu" => {
                options.cpu = Some(number(value("--cpu")?, "--cpu", 0, 4095)?);
            }
            "--priority" => {
                options.priority = number(value("--priority")?, "--priority", 1, 99)?;
            }
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
        let Ok(Command::Run(options)) = parse(&arguments("run")) else {
            panic!("run was not understood");
        };
        assert_eq!(options.rate, 48_000);
        assert_eq!(options.frames, None);
        assert_eq!(options.seconds, 10);
        assert_eq!(options.cpu, None);
        assert_eq!(options.priority, 80);
        assert_eq!(options.realtime, Realtime::IfAllowed);
    }

    #[test]
    fn run_takes_its_options() {
        let text = "run --backend null --rate 96000 --quantum 256 --seconds 60 --cpu 4 --priority 70 --require-rt";
        let Ok(Command::Run(options)) = parse(&arguments(text)) else {
            panic!("run was not understood");
        };
        assert_eq!(options.rate, 96_000);
        assert_eq!(options.frames, Some(256));
        assert_eq!(options.seconds, 60);
        assert_eq!(options.cpu, Some(4));
        assert_eq!(options.priority, 70);
        assert_eq!(options.realtime, Realtime::Required);
    }

    #[test]
    fn mistakes_are_named() {
        for (text, expected) in [
            ("walk", "unknown command"),
            ("run --rate", "needs a value"),
            ("run --rate fast", "not a number"),
            ("run --priority 0", "not between"),
            ("run --priority 100", "not between"),
            ("run --backend alsa", "unknown back end"),
            ("run --loud", "unknown option"),
        ] {
            let error = parse(&arguments(text)).expect_err(text);
            assert!(error.contains(expected), "'{text}' gave '{error}'");
        }
    }
}
