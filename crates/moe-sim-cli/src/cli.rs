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
//! arguments, and no stdin or JSON modes. Argument errors are reported on
//! stderr with exit code 2: by `clap` for spelling and value errors, and by
//! the scope rules below for combinations `clap` cannot express (quotas under
//! a global scope, a per-layer scope without quotas, one layer quoted twice).
//!
//! `--policy` is required and has no default: selecting a simulation policy is
//! the caller's decision, and an implicit one would silently pick the
//! baseline. `--cache-scope` defaults to `global`, which is not a silent
//! choice among equals: it is the established single-cache behaviour, and the
//! only scope that works without further flags.

use std::collections::BTreeMap;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use moe_sim_core::{CacheScope, Policy};

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
///
/// Mirrors [`moe_sim_core::Policy`] as a command-line surface. The two are
/// kept separate so argument spelling can change without touching simulation
/// semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PolicyArg {
    /// Retain nothing between events: the baseline every policy is measured
    /// against.
    NoCache,
    /// Evict the least recently used unpinned expert, breaking ties by
    /// lowest expert key.
    Lru,
    /// Evict the least frequently used unpinned expert, breaking ties by
    /// least recent use, then by lowest expert key.
    Lfu,
}

impl From<PolicyArg> for Policy {
    fn from(policy: PolicyArg) -> Self {
        match policy {
            PolicyArg::NoCache => Self::NoCache,
            PolicyArg::Lru => Self::Lru,
            PolicyArg::Lfu => Self::Lfu,
        }
    }
}

/// Cache scope selected for one run.
///
/// Mirrors [`moe_sim_core::CacheScope`] as a command-line surface, minus the
/// budgets that other flags carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CacheScopeArg {
    /// One cache over the total budget.
    Global,
    /// One independent cache per layer, each bounded by its explicit
    /// `--layer-quota-bytes` quota.
    PerLayer,
}

/// Parses one `LAYER:BYTES` quota pair.
fn parse_layer_quota(raw: &str) -> Result<(u32, u64), String> {
    let Some((layer, bytes)) = raw.split_once(':') else {
        return Err(format!("expected LAYER:BYTES, got `{raw}`"));
    };
    let layer_id: u32 = layer.parse().map_err(|error| {
        format!("layer id must be a non-negative integer, got `{layer}`: {error}")
    })?;
    let quota_bytes: u64 = bytes
        .parse()
        .map_err(|error| format!("quota must be a plain byte count, got `{bytes}`: {error}"))?;
    Ok((layer_id, quota_bytes))
}

/// Builds the domain scope from the parsed scope and quota flags.
///
/// # Errors
///
/// Returns a usage message when quotas appear under a global scope, when a
/// per-layer scope has no quotas, or when one layer is quoted twice. These
/// rules depend on the value of `--cache-scope`, which `clap` cannot express
/// declaratively; callers report the message with exit code 2.
pub fn cache_scope(
    scope: CacheScopeArg,
    layer_quota_bytes: &[(u32, u64)],
    total_budget_bytes: u64,
) -> Result<CacheScope, String> {
    match scope {
        CacheScopeArg::Global => {
            if layer_quota_bytes.is_empty() {
                Ok(CacheScope::Global {
                    budget_bytes: total_budget_bytes,
                })
            } else {
                Err("--layer-quota-bytes requires --cache-scope per-layer".to_owned())
            }
        }
        CacheScopeArg::PerLayer => {
            if layer_quota_bytes.is_empty() {
                return Err(
                    "--cache-scope per-layer requires at least one --layer-quota-bytes".to_owned(),
                );
            }
            let mut quotas = BTreeMap::new();
            for &(layer_id, quota_bytes) in layer_quota_bytes {
                if quotas.insert(layer_id, quota_bytes).is_some() {
                    return Err(format!(
                        "layer {layer_id} has more than one --layer-quota-bytes"
                    ));
                }
            }
            Ok(CacheScope::PerLayer {
                total_budget_bytes,
                layer_quota_bytes: quotas,
            })
        }
    }
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

    /// Cache scope the budget applies through.
    #[arg(long, value_name = "SCOPE", default_value = "global")]
    pub cache_scope: CacheScopeArg,

    /// Byte quota for one layer as `LAYER:BYTES`; repeat once per layer.
    #[arg(long, value_name = "LAYER:BYTES", value_parser = parse_layer_quota)]
    pub layer_quota_bytes: Vec<(u32, u64)>,

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
    /// Validate a trace and manifest against the selected cache scope's byte
    /// capacity.
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

    /// Cache scope the budget applies through.
    #[arg(long, value_name = "SCOPE", default_value = "global")]
    pub cache_scope: CacheScopeArg,

    /// Byte quota for one layer as `LAYER:BYTES`; repeat once per layer.
    #[arg(long, value_name = "LAYER:BYTES", value_parser = parse_layer_quota)]
    pub layer_quota_bytes: Vec<(u32, u64)>,
}
