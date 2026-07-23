//! Command-line argument definitions for the `moe-sim` binary.
//!
//! The frozen v1 surface is exactly two commands, both flag-driven:
//!
//! ```text
//! moe-sim trace inspect --trace <PATH>
//! moe-sim capacity check --trace <PATH> --model-manifest <PATH> --global-budget-bytes <BYTES>
//! ```
//!
//! Budgets are plain byte counts; there are no short aliases, no positional
//! arguments, and no stdin or JSON modes. Argument errors are reported by
//! `clap` on stderr with exit code 2.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// Trace-driven simulator for out-of-core Mixture-of-Experts inference.
#[derive(Debug, Parser)]
#[command(name = "moe-sim")]
pub struct Cli {
    /// The selected top-level command.
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level `moe-sim` commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Inspect activation traces.
    #[command(subcommand)]
    Trace(TraceCommand),
    /// Check capacity feasibility before any simulation.
    #[command(subcommand)]
    Capacity(CapacityCommand),
}

/// `moe-sim trace` subcommands.
#[derive(Debug, Subcommand)]
pub enum TraceCommand {
    /// Parse a strict v1 JSONL trace and print a deterministic summary.
    Inspect(TraceInspectArgs),
}

/// Arguments for `moe-sim trace inspect`.
#[derive(Debug, Args)]
pub struct TraceInspectArgs {
    /// Path to a strict v1 JSONL activation trace.
    #[arg(long, value_name = "PATH")]
    pub trace: PathBuf,
}

/// `moe-sim capacity` subcommands.
#[derive(Debug, Subcommand)]
pub enum CapacityCommand {
    /// Validate a trace and manifest against a global byte budget.
    Check(CapacityCheckArgs),
}

/// Arguments for `moe-sim capacity check`.
#[derive(Debug, Args)]
pub struct CapacityCheckArgs {
    /// Path to a strict v1 JSONL activation trace.
    #[arg(long, value_name = "PATH")]
    pub trace: PathBuf,

    /// Path to a strict v1 JSON model manifest.
    #[arg(long, value_name = "PATH")]
    pub model_manifest: PathBuf,

    /// Global capacity budget in bytes.
    #[arg(long, value_name = "BYTES")]
    pub global_budget_bytes: u64,
}
