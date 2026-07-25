//! Command-line argument definitions for the `moe-sim` binary.
//!
//! The v1 surface is flag-driven throughout:
//!
//! ```text
//! moe-sim trace inspect --trace <PATH>
//! moe-sim capacity check --trace <PATH> --model-manifest <PATH> --global-budget-bytes <BYTES>
//! moe-sim run --trace <PATH> --model-manifest <PATH> --global-budget-bytes <BYTES> --policy <POLICY>
//! ```
//!
//! Budgets are plain byte counts; there are no short aliases, no positional
//! arguments, and no stdin or JSON modes. Argument errors are reported by
//! `clap` on stderr with exit code 2.
//!
//! `--policy` is required and has no default: selecting a simulation policy is
//! the caller's decision, and an implicit one would silently pick the baseline.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

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
    /// Replay a trace under one policy and report byte and object metrics.
    Run(RunArgs),
}

/// Cache policy selected for one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PolicyArg {
    /// Retain nothing between events: the baseline every policy is measured
    /// against.
    NoCache,
}

/// Arguments for `moe-sim run`.
#[derive(Debug, Args)]
pub struct RunArgs {
    /// Path to a strict v1 JSONL activation trace.
    #[arg(long, value_name = "PATH")]
    pub trace: PathBuf,

    /// Path to a strict v1 JSON model manifest.
    #[arg(long, value_name = "PATH")]
    pub model_manifest: PathBuf,

    /// Global capacity budget in bytes.
    #[arg(long, value_name = "BYTES")]
    pub global_budget_bytes: u64,

    /// Policy to replay the trace under.
    #[arg(long, value_name = "POLICY")]
    pub policy: PolicyArg,
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
