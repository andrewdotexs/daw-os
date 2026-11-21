//! `dawos-engine`: the audio engine of dawOS.
//!
//! Phase 1, step P1.3: the cycle and its protection. One command, `run`,
//! starts one thread per live CPU, each with the two-phase cycle of
//! ADR 0021 on the back end `null`; a watchdog outside the live CPUs and
//! one spare thread per live CPU (ADR 0023); and, on request, a fault to
//! prove that the protection works. No sound and no graph yet.

#![forbid(unsafe_code)]

mod cli;
mod live;
mod notify;
mod plan;
mod run;
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
