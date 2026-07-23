//! Thin entrypoint for the `moe-sim` binary.
//!
//! `main` only parses arguments, delegates to [`moe_sim_cli::commands::run`],
//! and translates the outcome into streams and exit codes: the rendered
//! report goes to stdout on success (exit 0), the error chain goes to stderr
//! on failure (exit 3–5), and `clap` reports argument errors itself (exit 2).

#![expect(
    unused_crate_dependencies,
    reason = "binary targets receive every package dependency; this entrypoint only drives the moe_sim_cli library through clap"
)]

use std::process::ExitCode;

use clap::Parser;
use moe_sim_cli::cli::Cli;
use moe_sim_cli::commands;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match commands::run(&cli) {
        Ok(report) => {
            print!("{report}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}
