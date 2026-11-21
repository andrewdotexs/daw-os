//! The command `run`: one thread, one CPU, a cycle that computes nothing.

use std::path::Path;

use dawos_audio_io::{Backend, Format, NullBackend};
use dawos_rt::stats::Durations;
use dawos_rt::{clock, cpu, topology};

use crate::cli::{Realtime, RunOptions};

/// Run the cycle and print the report.
pub fn run(options: &RunOptions) -> Result<(), String> {
    let format = Format {
        rate: options.rate,
        frames: options
            .frames
            .unwrap_or(Format::default_frames(options.rate)),
    };
    let mut backend = NullBackend::new(format);

    // Which CPU: the one asked for, else the first live CPU of the boot
    // profile, else none (a development PC has no profile).
    let cpu = match options.cpu {
        Some(cpu) => Some(cpu),
        None => first_live_cpu()?,
    };

    // Everything that can fail or allocate happens here, before the
    // cycle starts.
    let memory_locked = match cpu::lock_memory() {
        Ok(()) => true,
        Err(error) if options.realtime == Realtime::Required => {
            return Err(format!("cannot lock the memory of the process: {error}"));
        }
        Err(_) => false,
    };
    if let Some(cpu) = cpu {
        cpu::pin_current_thread(cpu)
            .map_err(|error| format!("cannot run on CPU {cpu}: {error}"))?;
    }
    let realtime = match options.realtime {
        Realtime::Off => false,
        Realtime::IfAllowed | Realtime::Required => {
            match cpu::set_fifo_priority(options.priority) {
                Ok(()) => true,
                Err(error) if options.realtime == Realtime::Required => {
                    return Err(format!(
                        "cannot take the real-time priority {}: {error}",
                        options.priority
                    ));
                }
                Err(_) => false,
            }
        }
    };

    println!("dawos-engine {}", env!("CARGO_PKG_VERSION"));
    println!("backend: {}", backend.name());
    println!("rate_hz: {}", format.rate);
    println!("quantum_frames: {}", format.frames);
    println!("period_us: {:.3}", format.period_ns() as f64 / 1000.0);
    match cpu {
        Some(cpu) => println!("cpu: {cpu}"),
        None => println!("cpu: not pinned (no boot profile and no --cpu)"),
    }
    if realtime {
        println!("scheduling: fifo {}", options.priority);
    } else if options.realtime == Realtime::Off {
        println!("scheduling: normal (--no-rt)");
    } else {
        println!("scheduling: normal (the kernel refused the real-time priority: figures below are not those of a live CPU)");
    }
    println!(
        "memory_locked: {}",
        if memory_locked { "yes" } else { "no" }
    );
    println!("seconds: {}", options.seconds);

    let report = cycle(&mut backend, options.seconds);

    println!("cycles: {}", report.lateness.count());
    println!("missed_periods: {}", report.missed);
    println!("ran_on_cpus: {}", report.cpus_seen());
    print_microseconds("wake_late_us_min", report.lateness.min_ns());
    print_microseconds("wake_late_us_avg", report.lateness.mean_ns());
    print_microseconds("wake_late_us_p99.999", report.lateness.quantile_ns(99_999));
    print_microseconds("wake_late_us_max", report.lateness.max_ns());
    Ok(())
}

/// What the cycle measured.
struct Report {
    /// How late each wake-up was.
    lateness: Durations,
    /// Periods that went by unserved.
    missed: u64,
    /// The CPU of the first cycle and whether any later cycle ran
    /// elsewhere.
    first_cpu: Option<usize>,
    moved: bool,
}

impl Report {
    fn cpus_seen(&self) -> String {
        match (self.first_cpu, self.moved) {
            (Some(cpu), false) => cpu.to_string(),
            (Some(cpu), true) => format!("{cpu} and others"),
            (None, _) => "unknown".to_string(),
        }
    }
}

/// The cycle. Two phases (ADR 0021): the wait, one blocking call inside
/// the back end; and the compute phase, which here only takes notes.
/// Nothing in this function allocates or prints.
fn cycle(backend: &mut dyn Backend, seconds: u64) -> Report {
    let mut report = Report {
        lateness: Durations::new(),
        missed: 0,
        first_cpu: None,
        moved: false,
    };
    let end = clock::now() + seconds * 1_000_000_000;
    loop {
        // Wait phase.
        let wake = backend.wait();
        // Compute phase.
        report.lateness.add(wake.lateness_ns());
        report.missed += wake.missed;
        let cpu = cpu::current_cpu();
        if report.first_cpu.is_none() {
            report.first_cpu = cpu;
        } else if cpu != report.first_cpu {
            report.moved = true;
        }
        if wake.woke >= end {
            return report;
        }
    }
}

/// The first CPU of the live domain of the boot profile; `None` when
/// there is no report of `dawos-tune`.
fn first_live_cpu() -> Result<Option<usize>, String> {
    let Some(list) = topology::domain(Path::new(topology::REPORT), "live") else {
        return Ok(None);
    };
    let cpus =
        cpu::parse_cpu_list(&list).map_err(|error| format!("{}: {error}", topology::REPORT))?;
    Ok(cpus.first().copied())
}

fn print_microseconds(name: &str, ns: Option<u64>) {
    match ns {
        Some(ns) => println!("{name}: {:.2}", ns as f64 / 1000.0),
        None => println!("{name}: -"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_run_counts_its_cycles() {
        // One second of periods of one millisecond, without real-time
        // priority: about a thousand cycles, none on another CPU than
        // the kernel chose at each wake-up.
        let mut backend = NullBackend::new(Format {
            rate: 48_000,
            frames: 48,
        });
        let report = cycle(&mut backend, 1);
        let cycles = report.lateness.count() + report.missed;
        assert!(
            (990..=1_010).contains(&cycles),
            "{cycles} cycles in one second"
        );
        assert!(report.first_cpu.is_some());
    }
}
