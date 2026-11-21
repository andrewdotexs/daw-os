//! `dawos-engine`: the audio engine of dawOS.
//!
//! Phase 1, step P1.2: the skeleton. One command, `run`, drives a cycle
//! from the back end `null` on one CPU and reports how late each wake-up
//! was. No sound, no graph: what it proves is that the engine builds on
//! the development PC and into the image, takes a live CPU with a
//! real-time priority, and keeps a period.

#![forbid(unsafe_code)]

mod cli;
mod run;

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
