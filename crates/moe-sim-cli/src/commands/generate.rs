//! The `trace generate` command: deterministic synthetic traces and their
//! twin manifests, written to disk with recorded provenance.
//!
//! The report opens with a `source: synthetic` label — the strict v1 wire
//! formats have no field for it, so a generated file is indistinguishable
//! from a measured one on its own, and the report is where the label lives.
//! It then echoes the pattern, every parameter it consumed — including the
//! seed for the stochastic pattern — and the SHA-256 of each written file,
//! so a generated input can be reproduced and audited exactly like a
//! hand-written one.
//!
//! The two writes are not atomic: when the second write fails, the first
//! file stays on disk and the report (the only success signal) is never
//! printed. Output paths that resolve to the same physical file — spelled
//! identically, aliased through symlinks, or (for existing files on Unix)
//! hard-linked — are rejected before anything is written, so one file
//! cannot silently overwrite the other.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use moe_sim_core::{ModelManifest, synthetic};

use super::CliError;
use crate::cli::{TraceGenerateArgs, synthetic_pattern};
use crate::manifest_json::encode_manifest_json;
use crate::provenance::{INPUT_FORMAT_VERSION, sha256_hex, tool_version};
use crate::trace_jsonl::encode_trace_jsonl;

/// Executes `trace generate`: build the pattern, generate, encode, write.
pub(super) fn run_generate(args: &TraceGenerateArgs) -> Result<String, CliError> {
    ensure_distinct_destinations(&args.out_trace, &args.out_model_manifest)?;
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

/// Rejects output paths that land on the same physical file, before any
/// write happens.
///
/// Three aliases are caught: identical spellings, paths that resolve to the
/// same destination once symlinks in them are followed, and (for files that
/// already exist, on Unix) hard links sharing one inode. A parent directory
/// that does not resolve falls back to the spelled path — the write itself
/// will then fail with the underlying error.
fn ensure_distinct_destinations(out_trace: &Path, out_manifest: &Path) -> Result<(), CliError> {
    let collision = out_trace == out_manifest
        || physical_destination(out_trace) == physical_destination(out_manifest)
        || same_existing_file(out_trace, out_manifest);
    if collision {
        return Err(CliError::Usage {
            message: "--out-trace and --out-model-manifest resolve to the same file; \
                      name two different destinations"
                .to_owned(),
        });
    }
    Ok(())
}

/// The physical file a write to `path` would land on.
///
/// An existing path is canonicalized outright (following a symlinked final
/// component too); otherwise the parent directory is canonicalized and the
/// file name reattached, so a symlinked directory cannot alias two spelled
/// paths onto one file.
fn physical_destination(path: &Path) -> PathBuf {
    if let Ok(resolved) = path.canonicalize() {
        return resolved;
    }
    let Some(file_name) = path.file_name() else {
        return path.to_path_buf();
    };
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    match parent.canonicalize() {
        Ok(parent) => parent.join(file_name),
        Err(_) => path.to_path_buf(),
    }
}

/// Whether both paths name one existing physical file (Unix: same device
/// and inode, which also catches hard links that canonicalization keeps
/// apart).
#[cfg(unix)]
fn same_existing_file(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata(left), fs::metadata(right)) {
        (Ok(left), Ok(right)) => left.ino() == right.ino() && left.dev() == right.dev(),
        _ => false,
    }
}

/// Non-Unix fallback: hard-link identity is not detectable portably; the
/// spelled-path and canonicalization checks still apply.
#[cfg(not(unix))]
fn same_existing_file(_left: &Path, _right: &Path) -> bool {
    false
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
         source: synthetic\n\
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
