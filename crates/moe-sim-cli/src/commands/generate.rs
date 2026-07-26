//! The `trace generate` command: deterministic synthetic traces and their
//! twin manifests, written to disk with recorded provenance.
//!
//! The report echoes the pattern, every parameter it consumed — including
//! the seed for the stochastic pattern — and the SHA-256 of each written
//! file, so a generated input can be reproduced and audited exactly like a
//! hand-written one.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use moe_sim_core::{ModelManifest, synthetic};

use super::CliError;
use crate::cli::{TraceGenerateArgs, synthetic_pattern};
use crate::manifest_json::encode_manifest_json;
use crate::provenance::{INPUT_FORMAT_VERSION, sha256_hex, tool_version};
use crate::trace_jsonl::encode_trace_jsonl;

/// Executes `trace generate`: build the pattern, generate, encode, write.
pub(super) fn run_generate(args: &TraceGenerateArgs) -> Result<String, CliError> {
    let pattern = synthetic_pattern(args).map_err(|message| CliError::Usage { message })?;
    let case = synthetic::generate(&pattern).map_err(|source| CliError::Synthetic {
        message: source.to_string(),
    })?;
    // The generators only emit unique, positive-size entries, so these
    // conversions are defensive rather than expected failure paths.
    let manifest = ModelManifest::try_from_entries(case.manifest_entries.iter().copied()).map_err(
        |source| CliError::Synthetic {
            message: source.to_string(),
        },
    )?;
    let trace_bytes = encode_trace_jsonl(&case.events).map_err(|source| CliError::Synthetic {
        message: source.to_string(),
    })?;
    let manifest_bytes = encode_manifest_json(&manifest).map_err(|source| CliError::Synthetic {
        message: source.to_string(),
    })?;

    write_output(&args.out_trace, &trace_bytes)?;
    write_output(&args.out_model_manifest, &manifest_bytes)?;

    Ok(render_generate(
        args,
        case.events.len(),
        &sha256_hex(trace_bytes.as_bytes()),
        &sha256_hex(manifest_bytes.as_bytes()),
    ))
}

/// Writes one generated document, naming the path on failure.
fn write_output(path: &Path, contents: &str) -> Result<(), CliError> {
    fs::write(path, contents).map_err(|source| CliError::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// Renders the generation report: pattern, consumed parameters, and the
/// digest of each written file.
fn render_generate(
    args: &TraceGenerateArgs,
    events: usize,
    trace_digest: &str,
    manifest_digest: &str,
) -> String {
    let mut report = format!(
        "status: ok\n\
         tool_version: {}\n\
         input_format: {INPUT_FORMAT_VERSION}\n\
         pattern: {}\n\
         experts: {}\n\
         events: {events}\n",
        tool_version(),
        args.pattern.name(),
        args.experts,
    );
    // Only the parameters the validated pattern consumed are echoed; the
    // seed line is what makes the stochastic trace reproducible.
    if let Some(active_per_event) = args.active_per_event {
        // fmt::Write to a String is infallible; the Result exists only for
        // the trait.
        let _ = writeln!(report, "active_per_event: {active_per_event}");
    }
    if let Some(hot) = args.hot {
        let _ = writeln!(report, "hot: {hot}");
    }
    if let Some(period) = args.period {
        let _ = writeln!(report, "period: {period}");
    }
    if let Some(seed) = args.seed {
        let _ = writeln!(report, "seed: {seed}");
    }
    let _ = write!(
        report,
        "out_trace: {}\n\
         out_trace_sha256: {trace_digest}\n\
         out_model_manifest: {}\n\
         out_model_manifest_sha256: {manifest_digest}\n",
        args.out_trace.display(),
        args.out_model_manifest.display(),
    );
    report
}
