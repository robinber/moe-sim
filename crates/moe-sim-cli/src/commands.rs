//! Orchestration and report rendering for the `moe-sim` binary commands.
//!
//! [`run`] executes one parsed command and returns the complete stdout report
//! as a string; it never writes to any stream itself, so a failure produces
//! no partial stdout. Inputs are handled strictly in argument order: the
//! trace is read and parsed first, then (for `capacity check`) the manifest,
//! then the capacity feasibility pass. Equal inputs render byte-identical
//! reports.
//!
//! Every success report opens with provenance: the tool version, the input
//! contract version, and a SHA-256 digest of each input document beside its
//! path. See [`crate::provenance`].

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use moe_sim_core::{
    CacheScope, CapacityError, Event, ModelManifest, Phase, Policy, ReplayError, ReplayMetrics,
    replay,
};

use crate::cli::{
    CapacityCheckArgs, CapacityCommand, Cli, Command, RunArgs, TraceCommand, TraceInspectArgs,
    cache_scope,
};
use crate::provenance::{INPUT_FORMAT_VERSION, sha256_hex, tool_version};
use crate::{ManifestParseError, TraceParseError, parse_manifest_json, parse_trace_jsonl};

/// Errors surfaced by the `moe-sim` binary commands.
///
/// Each variant maps to one process exit code via [`CliError::exit_code`].
/// Most argument errors exit with code 2 and are produced and reported by
/// `clap` before these commands run; [`CliError::Usage`] covers the scope and
/// quota combinations `clap` cannot express declaratively.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// The scope and quota flags contradict each other. Exit code 2.
    ///
    /// The rules depend on the value of `--cache-scope`, so they are checked
    /// here rather than by `clap`: quotas require a per-layer scope, a
    /// per-layer scope requires quotas, and no layer may be quoted twice.
    #[error("{message}")]
    Usage {
        /// Description of the contradictory flags.
        message: String,
    },
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
    /// The trace, manifest, and budget configuration — global budget, or
    /// total budget plus layer quotas — failed capacity validation.
    /// Exit code 5.
    #[error("capacity check failed: {source}")]
    Capacity {
        /// Underlying capacity feasibility error.
        source: CapacityError,
    },
    /// Replay failed after the configuration was accepted. Exit code 6.
    ///
    /// `run` validates capacity before replaying, and that pass already
    /// rejects unknown experts and per-event byte overflow, so this variant is
    /// not reachable through the current command flow. It exists because
    /// replay returns a fallible result that must not be discarded, and a
    /// cumulative counter overflow has no earlier gate.
    #[error("replay failed: {source}")]
    Replay {
        /// Underlying replay error.
        source: ReplayError,
    },
}

impl CliError {
    /// Process exit code for this error: 2 usage, 3 read, 4 parse, 5
    /// capacity, 6 replay.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage { .. } => 2,
            Self::Read { .. } => 3,
            Self::TraceParse { .. } | Self::ManifestParse { .. } => 4,
            Self::Capacity { .. } => 5,
            Self::Replay { .. } => 6,
        }
    }
}

/// Runs one parsed command and returns the rendered stdout report.
///
/// # Errors
///
/// Returns [`CliError::Usage`] when the scope and quota flags contradict
/// each other, [`CliError::Read`] when an input file cannot be read as UTF-8
/// text, [`CliError::TraceParse`] or [`CliError::ManifestParse`] when a file
/// is not a valid strict v1 document, [`CliError::Capacity`] when the
/// capacity feasibility pass rejects the configuration, and
/// [`CliError::Replay`] when replay itself fails afterwards.
pub fn run(cli: &Cli) -> Result<String, CliError> {
    match &cli.command {
        Command::Trace(TraceCommand::Inspect(args)) => trace_inspect(args),
        Command::Capacity(CapacityCommand::Check(args)) => capacity_check(args),
        Command::Run(args) => run_replay(args),
    }
}

/// Executes `run`: read and parse both inputs, validate capacity, then replay.
///
/// Capacity is validated before replay so an infeasible configuration is
/// rejected instead of producing metrics that describe a run which could not
/// happen. The scope flags are resolved first: a contradictory combination is
/// a usage error that never touches the filesystem.
fn run_replay(args: &RunArgs) -> Result<String, CliError> {
    let scope = cache_scope(
        args.cache_scope,
        &args.layer_quota_bytes,
        args.global_budget_bytes,
    )
    .map_err(|message| CliError::Usage { message })?;

    let trace = load_trace(&args.trace)?;
    let manifest = load_manifest(&args.model_manifest)?;
    validate_capacity(&manifest.value, &scope, &trace.value)?;

    let policy = Policy::from(args.policy);
    let metrics = replay(&manifest.value, trace.value.iter(), policy, &scope)
        .map_err(|source| CliError::Replay { source })?;

    Ok(render_run(
        args,
        &scope,
        policy,
        &trace.digest,
        &manifest.digest,
        &metrics,
    ))
}

/// Executes `trace inspect`: read, parse, summarize.
fn trace_inspect(args: &TraceInspectArgs) -> Result<String, CliError> {
    let trace = load_trace(&args.trace)?;
    Ok(render_trace_inspect(
        &args.trace,
        &trace.digest,
        &summarize(&trace.value),
    ))
}

/// Executes `capacity check`: read and parse both inputs, then validate.
fn capacity_check(args: &CapacityCheckArgs) -> Result<String, CliError> {
    let scope = cache_scope(
        args.cache_scope,
        &args.layer_quota_bytes,
        args.global_budget_bytes,
    )
    .map_err(|message| CliError::Usage { message })?;

    let trace = load_trace(&args.trace)?;
    let manifest = load_manifest(&args.model_manifest)?;
    validate_capacity(&manifest.value, &scope, &trace.value)?;
    Ok(render_capacity_check(
        args,
        &scope,
        &trace.digest,
        &manifest.digest,
        trace.value.len(),
        manifest.value.len(),
    ))
}

/// Runs the capacity feasibility pass that matches the selected scope.
fn validate_capacity(
    manifest: &ModelManifest,
    scope: &CacheScope,
    events: &[Event],
) -> Result<(), CliError> {
    match scope {
        CacheScope::Global { budget_bytes } => {
            manifest.validate_global_capacity(*budget_bytes, events)
        }
        CacheScope::PerLayer {
            total_budget_bytes,
            layer_quota_bytes,
        } => manifest.validate_per_layer_capacity(*total_budget_bytes, layer_quota_bytes, events),
    }
    .map_err(|source| CliError::Capacity { source })
}

/// One parsed input document beside the digest of the exact bytes it came from.
struct Loaded<T> {
    /// The parsed document.
    value: T,
    /// Lowercase hexadecimal SHA-256 of the raw input bytes.
    digest: String,
}

/// Reads and parses one strict v1 JSONL trace file.
fn load_trace(path: &Path) -> Result<Loaded<Vec<Event>>, CliError> {
    let raw = read_input(path)?;
    let value = parse_trace_jsonl(&raw).map_err(|source| CliError::TraceParse {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(Loaded {
        value,
        digest: sha256_hex(raw.as_bytes()),
    })
}

/// Reads and parses one strict v1 JSON model manifest file.
fn load_manifest(path: &Path) -> Result<Loaded<ModelManifest>, CliError> {
    let raw = read_input(path)?;
    let value = parse_manifest_json(&raw).map_err(|source| CliError::ManifestParse {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(Loaded {
        value,
        digest: sha256_hex(raw.as_bytes()),
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
fn render_trace_inspect(path: &Path, digest: &str, summary: &TraceSummary) -> String {
    format!(
        "status: ok\n\
         tool_version: {}\n\
         input_format: {INPUT_FORMAT_VERSION}\n\
         trace: {}\n\
         trace_sha256: {digest}\n\
         events: {}\n\
         requests: {}\n\
         layers: {}\n\
         expert_activations: {}\n\
         phase_prefill: {}\n\
         phase_decode: {}\n\
         phase_unknown: {}\n",
        tool_version(),
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
    scope: &CacheScope,
    trace_digest: &str,
    manifest_digest: &str,
    events: usize,
    manifest_experts: usize,
) -> String {
    let mut report = format!(
        "status: ok\n\
         tool_version: {}\n\
         input_format: {INPUT_FORMAT_VERSION}\n\
         trace: {}\n\
         trace_sha256: {trace_digest}\n\
         model_manifest: {}\n\
         model_manifest_sha256: {manifest_digest}\n\
         global_budget_bytes: {}\n\
         cache_scope: {scope}\n\
         events: {}\n\
         manifest_experts: {}\n",
        tool_version(),
        args.trace.display(),
        args.model_manifest.display(),
        args.global_budget_bytes,
        events,
        manifest_experts,
    );
    if let CacheScope::PerLayer {
        layer_quota_bytes, ..
    } = scope
    {
        for (layer_id, quota_bytes) in layer_quota_bytes {
            // fmt::Write to a String is infallible; the Result exists only
            // for the trait.
            let _ = writeln!(report, "layer {layer_id}: quota_bytes: {quota_bytes}");
        }
    }
    report
}

/// Renders the `run` success report.
///
/// The policy and scope names come from the domain types' `Display`, not from
/// `clap`, so the report contract cannot shift with an argument-parsing
/// detail. Under a per-layer scope the aggregate metrics are followed by one
/// line per quota'd layer pairing the quota with that cache's high-water
/// mark, so the per-layer capacity invariant is auditable from the report
/// itself.
fn render_run(
    args: &RunArgs,
    scope: &CacheScope,
    policy: Policy,
    trace_digest: &str,
    manifest_digest: &str,
    metrics: &ReplayMetrics,
) -> String {
    let mut report = format!(
        "status: ok\n\
         tool_version: {}\n\
         input_format: {INPUT_FORMAT_VERSION}\n\
         trace: {}\n\
         trace_sha256: {trace_digest}\n\
         model_manifest: {}\n\
         model_manifest_sha256: {manifest_digest}\n\
         global_budget_bytes: {}\n\
         cache_scope: {scope}\n\
         policy: {policy}\n\
         events: {}\n\
         object_loads: {}\n\
         byte_loads: {}\n\
         object_hits: {}\n\
         byte_hits: {}\n\
         object_reloads: {}\n\
         byte_reloads: {}\n\
         evictions: {}\n\
         evicted_bytes: {}\n\
         peak_resident_bytes: {}\n",
        tool_version(),
        args.trace.display(),
        args.model_manifest.display(),
        args.global_budget_bytes,
        metrics.events(),
        metrics.object_loads(),
        metrics.byte_loads(),
        metrics.object_hits(),
        metrics.byte_hits(),
        metrics.object_reloads(),
        metrics.byte_reloads(),
        metrics.evictions(),
        metrics.evicted_bytes(),
        metrics.peak_resident_bytes(),
    );
    if let CacheScope::PerLayer {
        layer_quota_bytes, ..
    } = scope
    {
        for (layer_id, quota_bytes) in layer_quota_bytes {
            let peak = metrics
                .layer_peak_resident_bytes()
                .get(layer_id)
                .copied()
                .unwrap_or(0);
            let _ = writeln!(
                report,
                "layer {layer_id}: quota_bytes: {quota_bytes}, peak_resident_bytes: {peak}"
            );
        }
    }
    report
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "tests construct valid events directly; direct unwraps keep failure diagnostics close to the fixture data"
)]
mod tests {
    use moe_sim_core::{EventParts, ExpertKey, ExpertSizeEntry, ManifestError, ReplayCounter};

    use super::*;

    // Stand-in digests: rendering must place whatever digest it is given, so
    // these need not hash the placeholder paths. Real inputs are covered
    // end-to-end in `tests/cli.rs` against externally computed checksums.
    const DIGEST: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const OTHER_DIGEST: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

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
        let rendered = render_trace_inspect(Path::new("t.jsonl"), DIGEST, &summarize(&events));
        assert_eq!(
            rendered,
            format!(
                "status: ok\n\
             tool_version: {}\n\
             input_format: v1\n\
             trace: t.jsonl\n\
             trace_sha256: {DIGEST}\n\
             events: 2\n\
             requests: 1\n\
             layers: 1\n\
             expert_activations: 3\n\
             phase_prefill: 1\n\
             phase_decode: 1\n\
             phase_unknown: 0\n",
                tool_version()
            )
        );
    }

    #[test]
    fn capacity_check_report_is_stable() {
        let args = CapacityCheckArgs {
            trace: PathBuf::from("t.jsonl"),
            model_manifest: PathBuf::from("m.json"),
            global_budget_bytes: 10,
            cache_scope: crate::cli::CacheScopeArg::Global,
            layer_quota_bytes: vec![],
        };
        let scope = CacheScope::Global { budget_bytes: 10 };
        assert_eq!(
            render_capacity_check(&args, &scope, DIGEST, OTHER_DIGEST, 2, 2),
            format!(
                "status: ok\n\
             tool_version: {}\n\
             input_format: v1\n\
             trace: t.jsonl\n\
             trace_sha256: {DIGEST}\n\
             model_manifest: m.json\n\
             model_manifest_sha256: {OTHER_DIGEST}\n\
             global_budget_bytes: 10\n\
             cache_scope: global\n\
             events: 2\n\
             manifest_experts: 2\n",
                tool_version()
            )
        );
    }

    #[test]
    fn per_layer_capacity_check_report_lists_each_quota() {
        let args = CapacityCheckArgs {
            trace: PathBuf::from("t.jsonl"),
            model_manifest: PathBuf::from("m.json"),
            global_budget_bytes: 15,
            cache_scope: crate::cli::CacheScopeArg::PerLayer,
            layer_quota_bytes: vec![(0, 10), (1, 5)],
        };
        let scope = CacheScope::PerLayer {
            total_budget_bytes: 15,
            layer_quota_bytes: [(0, 10), (1, 5)].into_iter().collect(),
        };
        let rendered = render_capacity_check(&args, &scope, DIGEST, OTHER_DIGEST, 4, 4);
        assert!(rendered.contains("cache_scope: per-layer\n"), "{rendered}");
        assert!(
            rendered.ends_with(
                "manifest_experts: 4\n\
                 layer 0: quota_bytes: 10\n\
                 layer 1: quota_bytes: 5\n"
            ),
            "{rendered}"
        );
    }

    #[test]
    fn run_report_is_stable() {
        let args = RunArgs {
            trace: PathBuf::from("t.jsonl"),
            model_manifest: PathBuf::from("m.json"),
            global_budget_bytes: 10,
            cache_scope: crate::cli::CacheScopeArg::Global,
            layer_quota_bytes: vec![],
            policy: crate::cli::PolicyArg::NoCache,
        };
        let scope = CacheScope::Global { budget_bytes: 10 };
        let manifest = ModelManifest::try_from_entries(vec![
            ExpertSizeEntry {
                key: ExpertKey::new(0, 0),
                size_bytes: 4,
            },
            ExpertSizeEntry {
                key: ExpertKey::new(0, 1),
                size_bytes: 6,
            },
        ])
        .unwrap();
        let events = [
            event(1, Phase::Prefill, 0, vec![0, 1]),
            event(1, Phase::Decode, 0, vec![1]),
        ];
        let metrics = replay(&manifest, events.iter(), Policy::NoCache, &scope).unwrap();

        assert_eq!(
            render_run(
                &args,
                &scope,
                Policy::NoCache,
                DIGEST,
                OTHER_DIGEST,
                &metrics
            ),
            format!(
                "status: ok\n\
             tool_version: {}\n\
             input_format: v1\n\
             trace: t.jsonl\n\
             trace_sha256: {DIGEST}\n\
             model_manifest: m.json\n\
             model_manifest_sha256: {OTHER_DIGEST}\n\
             global_budget_bytes: 10\n\
             cache_scope: global\n\
             policy: no-cache\n\
             events: 2\n\
             object_loads: 3\n\
             byte_loads: 16\n\
             object_hits: 0\n\
             byte_hits: 0\n\
             object_reloads: 1\n\
             byte_reloads: 6\n\
             evictions: 0\n\
             evicted_bytes: 0\n\
             peak_resident_bytes: 10\n",
                tool_version()
            )
        );
    }

    #[test]
    fn per_layer_run_report_pairs_each_quota_with_its_peak() {
        let args = RunArgs {
            trace: PathBuf::from("t.jsonl"),
            model_manifest: PathBuf::from("m.json"),
            global_budget_bytes: 15,
            cache_scope: crate::cli::CacheScopeArg::PerLayer,
            layer_quota_bytes: vec![(0, 10), (1, 5)],
            policy: crate::cli::PolicyArg::Lru,
        };
        let scope = CacheScope::PerLayer {
            total_budget_bytes: 15,
            layer_quota_bytes: [(0, 10), (1, 5)].into_iter().collect(),
        };
        let manifest = ModelManifest::try_from_entries(vec![
            ExpertSizeEntry {
                key: ExpertKey::new(0, 0),
                size_bytes: 4,
            },
            ExpertSizeEntry {
                key: ExpertKey::new(1, 0),
                size_bytes: 5,
            },
        ])
        .unwrap();
        let l0 = event(1, Phase::Decode, 0, vec![0]);
        let l1 = Event::new(EventParts {
            request_id: 1,
            phase: Phase::Decode,
            step_id: 0,
            token_position: 0,
            layer_id: 1,
            expert_ids: vec![0],
        })
        .unwrap();
        let events = [l0, l1];
        let metrics = replay(&manifest, events.iter(), Policy::Lru, &scope).unwrap();

        let rendered = render_run(&args, &scope, Policy::Lru, DIGEST, OTHER_DIGEST, &metrics);
        assert!(rendered.contains("cache_scope: per-layer\n"), "{rendered}");
        assert!(
            rendered.ends_with(
                "peak_resident_bytes: 9\n\
                 layer 0: quota_bytes: 10, peak_resident_bytes: 4\n\
                 layer 1: quota_bytes: 5, peak_resident_bytes: 5\n"
            ),
            "{rendered}"
        );
    }

    #[test]
    fn the_report_names_each_policy_from_the_domain_type() {
        for (policy, expected) in [
            (Policy::NoCache, "no-cache"),
            (Policy::Lru, "lru"),
            (Policy::Lfu, "lfu"),
        ] {
            let args = RunArgs {
                trace: PathBuf::from("t.jsonl"),
                model_manifest: PathBuf::from("m.json"),
                global_budget_bytes: 10,
                cache_scope: crate::cli::CacheScopeArg::Global,
                layer_quota_bytes: vec![],
                policy: crate::cli::PolicyArg::NoCache,
            };
            let rendered = render_run(
                &args,
                &CacheScope::Global { budget_bytes: 10 },
                policy,
                DIGEST,
                OTHER_DIGEST,
                &ReplayMetrics::default(),
            );
            assert!(
                rendered.contains(&format!("policy: {expected}\n")),
                "{policy} rendered as: {rendered}"
            );
        }
    }

    #[test]
    fn replay_failures_exit_with_their_own_code() {
        let replay = CliError::Replay {
            source: ReplayError::CounterOverflow {
                counter: ReplayCounter::ByteLoads,
                event_index: 0,
            },
        };
        assert_eq!(replay.exit_code(), 6);
    }

    #[test]
    fn exit_codes_map_error_families_to_the_frozen_contract() {
        let usage = CliError::Usage {
            message: "--layer-quota-bytes requires --cache-scope per-layer".to_owned(),
        };
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

        assert_eq!(usage.exit_code(), 2);
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
