//! Command-line argument definitions for the `moe-sim` binary.
//!
//! The v1 surface is flag-driven throughout:
//!
//! ```text
//! moe-sim trace inspect --trace <PATH>
//! moe-sim trace generate --pattern <PATTERN> --experts <N> --events <N> \
//!   --out-trace <PATH> --out-model-manifest <PATH>
//! moe-sim capacity check --trace <PATH> --model-manifest <PATH> --global-budget-bytes <BYTES>
//! moe-sim run --trace <PATH> --model-manifest <PATH> --global-budget-bytes <BYTES> --policy <POLICY>
//! moe-sim compare --trace <PATH> --model-manifest <PATH> \
//!   --global-budgets-bytes <BYTES,...> --policies <POLICY,...>
//! ```
//!
//! Budgets are plain byte counts; there are no short aliases, no positional
//! arguments, and no stdin modes. Argument errors are reported on stderr
//! with exit code 2: by `clap` for spelling and value errors, and by the
//! rule helpers below for combinations `clap` cannot express — scope and
//! quota agreement, duplicate-free `compare` lists, per-layer flags on
//! `compare`, and generation parameters that must match their `--pattern`.
//!
//! `--policy` is required and has no default: selecting a simulation policy is
//! the caller's decision, and an implicit one would silently pick the
//! baseline. `--cache-scope` defaults to `global`, which is not a silent
//! choice among equals: it is the established single-cache behaviour, and the
//! only scope that works without further flags.

use std::collections::BTreeMap;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use moe_sim_core::{CacheScope, Policy, SyntheticPattern};

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
    /// Replay one trace across policies and budgets and report the matrix.
    Compare(CompareArgs),
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
    /// Offline reference: evict the unpinned expert whose next use is
    /// farthest away, never-reused first, breaking ties by lowest expert
    /// key. Requires a uniform-size manifest and reads the whole trace.
    Belady,
}

impl From<PolicyArg> for Policy {
    fn from(policy: PolicyArg) -> Self {
        match policy {
            PolicyArg::NoCache => Self::NoCache,
            PolicyArg::Lru => Self::Lru,
            PolicyArg::Lfu => Self::Lfu,
            PolicyArg::Belady => Self::Belady,
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

/// Report format selected for one comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormatArg {
    /// Human-readable key/value report.
    Text,
    /// One machine-readable JSON object.
    Json,
    /// One CSV table whose rows repeat the provenance columns, so every
    /// line is self-contained.
    Csv,
}

/// Arguments for `moe-sim compare`.
#[derive(Debug, Args)]
pub struct CompareArgs {
    /// Path to a strict v1 JSONL activation trace.
    #[arg(long, value_name = "PATH")]
    pub trace: PathBuf,

    /// Path to a strict v1 JSON model manifest.
    #[arg(long, value_name = "PATH")]
    pub model_manifest: PathBuf,

    /// Byte budgets to sweep, comma-separated; order is preserved in the
    /// report.
    #[arg(long, value_name = "BYTES,...", value_delimiter = ',', required = true)]
    pub global_budgets_bytes: Vec<u64>,

    /// Policies to compare, comma-separated; order is preserved in the
    /// report.
    #[arg(
        long,
        value_name = "POLICY,...",
        value_delimiter = ',',
        required = true
    )]
    pub policies: Vec<PolicyArg>,

    /// Cache scope the budgets apply through; only `global` in `v0.1`.
    #[arg(long, value_name = "SCOPE", default_value = "global")]
    pub cache_scope: CacheScopeArg,

    /// Byte quota for one layer as `LAYER:BYTES`; not supported by compare.
    #[arg(long, value_name = "LAYER:BYTES", value_parser = parse_layer_quota)]
    pub layer_quota_bytes: Vec<(u32, u64)>,

    /// Report format.
    #[arg(long, value_name = "FORMAT", default_value = "text")]
    pub output: OutputFormatArg,
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
    /// Generate a synthetic trace and its manifest, deterministically.
    Generate(TraceGenerateArgs),
}

/// Synthetic pattern selected for one generation.
///
/// Mirrors [`moe_sim_core::SyntheticPattern`] as a command-line surface;
/// the per-pattern parameters travel as separate flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PatternArg {
    /// The same atomic set repeated on every event.
    Repetition,
    /// Single-expert events cycling round-robin over every expert.
    Cyclic,
    /// Seeded near-uniform draws of distinct experts per event.
    Random,
    /// A hot window of experts that shifts through the expert space.
    HotsetShift,
    /// The cyclic scan over experts of linearly growing size.
    VariableSizes,
    /// A hammered hot expert aged out by full cold scans: worst for
    /// recency, kind to frequency.
    AdversarialLru,
}

impl PatternArg {
    /// The stable report name of this pattern.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Repetition => "repetition",
            Self::Cyclic => "cyclic",
            Self::Random => "random",
            Self::HotsetShift => "hotset-shift",
            Self::VariableSizes => "variable-sizes",
            Self::AdversarialLru => "adversarial-lru",
        }
    }
}

/// Arguments for `moe-sim trace generate`.
#[derive(Debug, Args)]
pub struct TraceGenerateArgs {
    /// Synthetic pattern family.
    #[arg(long, value_name = "PATTERN")]
    pub pattern: PatternArg,

    /// Experts declared in the generated manifest.
    #[arg(long, value_name = "COUNT")]
    pub experts: u32,

    /// Events to generate.
    #[arg(long, value_name = "COUNT")]
    pub events: u64,

    /// Atomic-set width; required by `repetition` and `random`.
    #[arg(long, value_name = "COUNT")]
    pub active_per_event: Option<u32>,

    /// Hot-window width; required by `hotset-shift`.
    #[arg(long, value_name = "COUNT")]
    pub hot: Option<u32>,

    /// Events between hot-window shifts; required by `hotset-shift`.
    #[arg(long, value_name = "COUNT")]
    pub period: Option<u64>,

    /// Seed for the stochastic pattern; required by `random` and rejected
    /// for the deterministic patterns.
    #[arg(long, value_name = "SEED")]
    pub seed: Option<u64>,

    /// Path the generated JSONL trace is written to.
    #[arg(long, value_name = "PATH")]
    pub out_trace: PathBuf,

    /// Path the generated JSON manifest is written to.
    #[arg(long, value_name = "PATH")]
    pub out_model_manifest: PathBuf,
}

/// Builds the domain pattern from the parsed generation flags.
///
/// # Errors
///
/// Returns a usage message when a parameter another pattern owns is present,
/// or when a parameter the selected pattern requires is missing. Which flags
/// apply depends on `--pattern`, which `clap` cannot express declaratively;
/// callers report the message with exit code 2.
pub fn synthetic_pattern(args: &TraceGenerateArgs) -> Result<SyntheticPattern, String> {
    let require_u32 = |value: Option<u32>, flag: &str| {
        value.ok_or_else(|| format!("--{flag} is required by --pattern {}", args.pattern.name()))
    };
    let require_u64 = |value: Option<u64>, flag: &str| {
        value.ok_or_else(|| format!("--{flag} is required by --pattern {}", args.pattern.name()))
    };
    let reject_u32 = |value: Option<u32>, flag: &str, owner: &str| match value {
        Some(_) => Err(format!("--{flag} only applies to --pattern {owner}")),
        None => Ok(()),
    };
    let reject_u64 = |value: Option<u64>, flag: &str, owner: &str| match value {
        Some(_) => Err(format!("--{flag} only applies to --pattern {owner}")),
        None => Ok(()),
    };

    match args.pattern {
        PatternArg::Repetition => {
            reject_u32(args.hot, "hot", "hotset-shift")?;
            reject_u64(args.period, "period", "hotset-shift")?;
            reject_u64(args.seed, "seed", "random")?;
            Ok(SyntheticPattern::Repetition {
                experts: args.experts,
                active_per_event: require_u32(args.active_per_event, "active-per-event")?,
                events: args.events,
            })
        }
        PatternArg::Cyclic | PatternArg::VariableSizes | PatternArg::AdversarialLru => {
            reject_u32(
                args.active_per_event,
                "active-per-event",
                "repetition or random",
            )?;
            reject_u32(args.hot, "hot", "hotset-shift")?;
            reject_u64(args.period, "period", "hotset-shift")?;
            reject_u64(args.seed, "seed", "random")?;
            Ok(match args.pattern {
                PatternArg::Cyclic => SyntheticPattern::Cyclic {
                    experts: args.experts,
                    events: args.events,
                },
                PatternArg::VariableSizes => SyntheticPattern::VariableSizes {
                    experts: args.experts,
                    events: args.events,
                },
                _ => SyntheticPattern::AdversarialLru {
                    experts: args.experts,
                    events: args.events,
                },
            })
        }
        PatternArg::Random => {
            reject_u32(args.hot, "hot", "hotset-shift")?;
            reject_u64(args.period, "period", "hotset-shift")?;
            Ok(SyntheticPattern::Random {
                experts: args.experts,
                active_per_event: require_u32(args.active_per_event, "active-per-event")?,
                events: args.events,
                seed: require_u64(args.seed, "seed")?,
            })
        }
        PatternArg::HotsetShift => {
            reject_u32(
                args.active_per_event,
                "active-per-event",
                "repetition or random",
            )?;
            reject_u64(args.seed, "seed", "random")?;
            Ok(SyntheticPattern::HotsetShift {
                experts: args.experts,
                hot: require_u32(args.hot, "hot")?,
                period: require_u64(args.period, "period")?,
                events: args.events,
            })
        }
    }
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
