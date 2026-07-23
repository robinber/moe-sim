//! Orchestration and report rendering for the `moe-sim` binary commands.
//!
//! [`run`] executes one parsed command and returns the complete stdout report
//! as a string; it never writes to any stream itself, so a failure produces
//! no partial stdout. Inputs are handled strictly in argument order: the
//! trace is read and parsed first, then (for `capacity check`) the manifest,
//! then the capacity feasibility pass. Equal inputs render byte-identical
//! reports.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use moe_sim_core::{CapacityError, Event, ModelManifest, Phase};

use crate::cli::{
    CapacityCheckArgs, CapacityCommand, Cli, Command, TraceCommand, TraceInspectArgs,
};
use crate::{ManifestParseError, TraceParseError, parse_manifest_json, parse_trace_jsonl};

/// Errors surfaced by the `moe-sim` binary commands.
///
/// Each variant maps to one process exit code via [`CliError::exit_code`].
/// Argument errors exit with code 2 but are produced and reported by `clap`
/// before these commands run, so they have no variant here.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// An input file could not be read as UTF-8 text (missing path, I/O
    /// failure, or invalid UTF-8). Exit code 3.
    #[error("failed to read input file {}: {source}", path.display())]
    Read {
        /// Path that failed to read.
        path: PathBuf,
        /// Underlying I/O or UTF-8 error.
        source: std::io::Error,
    },
    /// A trace file was read but is not a valid strict v1 JSONL trace.
    /// Exit code 4.
    #[error("failed to parse trace {}: {source}", path.display())]
    TraceParse {
        /// Path of the invalid trace.
        path: PathBuf,
        /// Underlying wire or domain parse error.
        source: TraceParseError,
    },
    /// A manifest file was read but is not a valid strict v1 model manifest.
    /// Exit code 4.
    #[error("failed to parse model manifest {}: {source}", path.display())]
    ManifestParse {
        /// Path of the invalid manifest.
        path: PathBuf,
        /// Underlying wire or domain parse error.
        source: ManifestParseError,
    },
    /// The `(trace, manifest, budget)` triple failed capacity validation.
    /// Exit code 5.
    #[error("capacity check failed: {source}")]
    Capacity {
        /// Underlying capacity feasibility error.
        source: CapacityError,
    },
}

impl CliError {
    /// Process exit code for this error: 3 read, 4 parse, 5 capacity.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Read { .. } => 3,
            Self::TraceParse { .. } | Self::ManifestParse { .. } => 4,
            Self::Capacity { .. } => 5,
        }
    }
}

/// Runs one parsed command and returns the rendered stdout report.
///
/// # Errors
///
/// Returns [`CliError::Read`] when an input file cannot be read as UTF-8
/// text, [`CliError::TraceParse`] or [`CliError::ManifestParse`] when a file
/// is not a valid strict v1 document, and [`CliError::Capacity`] when the
/// capacity feasibility pass rejects the configuration.
pub fn run(cli: &Cli) -> Result<String, CliError> {
    match &cli.command {
        Command::Trace(TraceCommand::Inspect(args)) => trace_inspect(args),
        Command::Capacity(CapacityCommand::Check(args)) => capacity_check(args),
    }
}

/// Executes `trace inspect`: read, parse, summarize.
fn trace_inspect(args: &TraceInspectArgs) -> Result<String, CliError> {
    let events = load_trace(&args.trace)?;
    Ok(render_trace_inspect(&args.trace, &summarize(&events)))
}

/// Executes `capacity check`: read and parse both inputs, then validate.
fn capacity_check(args: &CapacityCheckArgs) -> Result<String, CliError> {
    let events = load_trace(&args.trace)?;
    let manifest = load_manifest(&args.model_manifest)?;
    manifest
        .validate_global_capacity(args.global_budget_bytes, events.iter())
        .map_err(|source| CliError::Capacity { source })?;
    Ok(render_capacity_check(args, events.len(), manifest.len()))
}

/// Reads and parses one strict v1 JSONL trace file.
fn load_trace(path: &Path) -> Result<Vec<Event>, CliError> {
    let raw = read_input(path)?;
    parse_trace_jsonl(&raw).map_err(|source| CliError::TraceParse {
        path: path.to_path_buf(),
        source,
    })
}

/// Reads and parses one strict v1 JSON model manifest file.
fn load_manifest(path: &Path) -> Result<ModelManifest, CliError> {
    let raw = read_input(path)?;
    parse_manifest_json(&raw).map_err(|source| CliError::ManifestParse {
        path: path.to_path_buf(),
        source,
    })
}

/// Reads one input file as UTF-8 text.
fn read_input(path: &Path) -> Result<String, CliError> {
    std::fs::read_to_string(path).map_err(|source| CliError::Read {
        path: path.to_path_buf(),
        source,
    })
}

/// Deterministic per-trace counts shown by `trace inspect`.
struct TraceSummary {
    /// Total canonical events in file order.
    events: usize,
    /// Distinct `request_id` values.
    requests: usize,
    /// Distinct `layer_id` values.
    layers: usize,
    /// Sum of active-set lengths over all events.
    expert_activations: usize,
    /// Events in the prefill phase.
    prefill: usize,
    /// Events in the decode phase.
    decode: usize,
    /// Events with an explicitly unknown phase.
    unknown: usize,
}

/// Computes the [`TraceSummary`] of parsed events in one pass.
fn summarize(events: &[Event]) -> TraceSummary {
    let mut requests = BTreeSet::new();
    let mut layers = BTreeSet::new();
    let mut expert_activations = 0usize;
    let mut prefill = 0usize;
    let mut decode = 0usize;
    let mut unknown = 0usize;
    for event in events {
        requests.insert(event.request_id());
        layers.insert(event.layer_id());
        expert_activations += event.expert_ids().len();
        match event.phase() {
            Phase::Prefill => prefill += 1,
            Phase::Decode => decode += 1,
            Phase::Unknown => unknown += 1,
        }
    }
    TraceSummary {
        events: events.len(),
        requests: requests.len(),
        layers: layers.len(),
        expert_activations,
        prefill,
        decode,
        unknown,
    }
}

/// Renders the `trace inspect` success report.
fn render_trace_inspect(path: &Path, summary: &TraceSummary) -> String {
    format!(
        "status: ok\n\
         trace: {}\n\
         events: {}\n\
         requests: {}\n\
         layers: {}\n\
         expert_activations: {}\n\
         phase_prefill: {}\n\
         phase_decode: {}\n\
         phase_unknown: {}\n",
        path.display(),
        summary.events,
        summary.requests,
        summary.layers,
        summary.expert_activations,
        summary.prefill,
        summary.decode,
        summary.unknown,
    )
}

/// Renders the `capacity check` success report.
fn render_capacity_check(
    args: &CapacityCheckArgs,
    events: usize,
    manifest_experts: usize,
) -> String {
    format!(
        "status: ok\n\
         trace: {}\n\
         model_manifest: {}\n\
         global_budget_bytes: {}\n\
         events: {}\n\
         manifest_experts: {}\n",
        args.trace.display(),
        args.model_manifest.display(),
        args.global_budget_bytes,
        events,
        manifest_experts,
    )
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "tests construct valid events directly; direct unwraps keep failure diagnostics close to the fixture data"
)]
mod tests {
    use moe_sim_core::{EventParts, ManifestError};

    use super::*;

    fn event(request_id: u64, phase: Phase, layer_id: u32, expert_ids: Vec<u32>) -> Event {
        Event::new(EventParts {
            request_id,
            phase,
            step_id: 0,
            token_position: 0,
            layer_id,
            expert_ids,
        })
        .unwrap()
    }

    #[test]
    fn summarize_counts_distinct_requests_layers_and_activations() {
        let events = vec![
            event(1, Phase::Prefill, 0, vec![0, 1]),
            event(1, Phase::Decode, 0, vec![1]),
            event(2, Phase::Unknown, 3, vec![]),
        ];

        let summary = summarize(&events);
        assert_eq!(summary.events, 3);
        assert_eq!(summary.requests, 2);
        assert_eq!(summary.layers, 2);
        assert_eq!(summary.expert_activations, 3);
        assert_eq!(summary.prefill, 1);
        assert_eq!(summary.decode, 1);
        assert_eq!(summary.unknown, 1);
    }

    #[test]
    fn summarize_of_empty_trace_is_all_zero() {
        let summary = summarize(&[]);
        assert_eq!(summary.events, 0);
        assert_eq!(summary.requests, 0);
        assert_eq!(summary.layers, 0);
        assert_eq!(summary.expert_activations, 0);
        assert_eq!(summary.prefill, 0);
        assert_eq!(summary.decode, 0);
        assert_eq!(summary.unknown, 0);
    }

    #[test]
    fn trace_inspect_report_is_stable() {
        let events = vec![
            event(1, Phase::Prefill, 0, vec![0, 1]),
            event(1, Phase::Decode, 0, vec![1]),
        ];
        let rendered = render_trace_inspect(Path::new("t.jsonl"), &summarize(&events));
        assert_eq!(
            rendered,
            "status: ok\n\
             trace: t.jsonl\n\
             events: 2\n\
             requests: 1\n\
             layers: 1\n\
             expert_activations: 3\n\
             phase_prefill: 1\n\
             phase_decode: 1\n\
             phase_unknown: 0\n"
        );
    }

    #[test]
    fn capacity_check_report_is_stable() {
        let args = CapacityCheckArgs {
            trace: PathBuf::from("t.jsonl"),
            model_manifest: PathBuf::from("m.json"),
            global_budget_bytes: 10,
        };
        assert_eq!(
            render_capacity_check(&args, 2, 2),
            "status: ok\n\
             trace: t.jsonl\n\
             model_manifest: m.json\n\
             global_budget_bytes: 10\n\
             events: 2\n\
             manifest_experts: 2\n"
        );
    }

    #[test]
    fn exit_codes_map_error_families_to_the_frozen_contract() {
        let read = CliError::Read {
            path: PathBuf::from("x"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
        };
        let trace_parse = CliError::TraceParse {
            path: PathBuf::from("x"),
            source: TraceParseError::BlankLine { line: 1 },
        };
        let manifest_parse = CliError::ManifestParse {
            path: PathBuf::from("x"),
            source: ManifestParseError::Manifest {
                source: ManifestError::ZeroSize {
                    layer_id: 0,
                    expert_id: 0,
                },
            },
        };
        let capacity = CliError::Capacity {
            source: CapacityError::ExpertExceedsGlobalCapacity {
                layer_id: 0,
                expert_id: 1,
                size_bytes: 6,
                global_budget_bytes: 5,
            },
        };

        assert_eq!(read.exit_code(), 3);
        assert_eq!(trace_parse.exit_code(), 4);
        assert_eq!(manifest_parse.exit_code(), 4);
        assert_eq!(capacity.exit_code(), 5);
    }

    #[test]
    fn cli_error_display_keeps_path_and_source_context() {
        let error = CliError::TraceParse {
            path: PathBuf::from("bad.jsonl"),
            source: TraceParseError::BlankLine { line: 2 },
        };
        assert_eq!(
            error.to_string(),
            "failed to parse trace bad.jsonl: line 2: blank lines are not allowed in a JSONL trace"
        );
        assert!(std::error::Error::source(&error).is_some());
    }
}
