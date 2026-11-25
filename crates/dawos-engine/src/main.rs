//! `dawos-engine`: the audio engine of dawOS.
//!
//! Phase 1: the cycle, its protection, a sound card and a graph. The
//! command `run` starts the live threads, each with the two-phase cycle
//! of ADR 0021; a watchdog outside the live CPUs and one spare thread
//! per live CPU (ADR 0023); and, on request, a fault to prove that the
//! protection works. The periods come from a timer (back end `null`) or
//! from a sound card (back end `alsa`: the first live thread holds the
//! card, the others follow its cycle), with a test tone, copies from
//! inputs to outputs and a measure of what comes in. With `--graph` the
//! threads run a graph in every cycle, each the nodes it was given
//! (ADR 0025); the command `graph` says what a graph compiles to.

#![forbid(unsafe_code)]

mod cli;
mod entries;
mod graph;
mod ktrace;
mod live;
mod meter;
mod notify;
mod patch;
mod plan;
mod probe;
mod run;
mod swap;
mod watchdog;

use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match cli::parse(&arguments) {
        Ok(cli::Command::Help) => {
            print!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Ok(cli::Command::Version) => {
            println!("dawos-engine {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(cli::Command::Graph(options)) => match graph::describe(&options) {
            Ok(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            Err(message) => {
                eprintln!("dawos-engine: {message}");
                ExitCode::FAILURE
            }
        },
        Ok(cli::Command::Run(options)) => match run::run(&options) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("dawos-engine: {message}");
                ExitCode::FAILURE
            }
        },
        Err(message) => {
            eprintln!("dawos-engine: {message}");
            eprint!("{}", cli::USAGE);
            ExitCode::from(2)
        }
    }
}
