//! The `compare` command: one trace and manifest replayed across a policy
//! and budget matrix, rendered as text, JSON, or CSV.
//!
//! Every replay in the matrix completes before one byte of report exists,
//! so an inapplicable combination — an infeasible budget, or `belady` on a
//! variable-size manifest — rejects the whole comparison instead of
//! emitting a partial table. Row order is the caller's: policies in
//! `--policies` order, budgets in `--global-budgets-bytes` order within
//! each policy.

use std::fmt::Write as _;

use moe_sim_core::{CacheScope, Policy, ReplayMetrics, replay};

use super::{BELADY_OBJECTIVE, CliError, load_manifest, load_trace, validate_capacity};
use crate::cli::{CacheScopeArg, CompareArgs, OutputFormatArg};
use crate::provenance::{INPUT_FORMAT_VERSION, tool_version};

/// One matrix cell: a policy replayed under one budget.
struct CompareRow {
    policy: Policy,
    budget_bytes: u64,
    metrics: ReplayMetrics,
}

/// Executes `compare`: validate the matrix, replay every cell, then render.
pub(super) fn run_compare(args: &CompareArgs) -> Result<String, CliError> {
    ensure_global_scope(args)?;
    let policies = distinct_policies(args)?;
    ensure_distinct_budgets(&args.global_budgets_bytes)?;

    let trace = load_trace(&args.trace)?;
    let manifest = load_manifest(&args.model_manifest)?;
    for &budget_bytes in &args.global_budgets_bytes {
        validate_capacity(
            &manifest.value,
            &CacheScope::Global { budget_bytes },
            &trace.value,
        )?;
    }

    let mut rows = Vec::new();
    for &policy in &policies {
        for &budget_bytes in &args.global_budgets_bytes {
            let metrics = replay(
                &manifest.value,
                trace.value.iter(),
                policy,
                &CacheScope::Global { budget_bytes },
            )
            .map_err(|source| CliError::Replay { source })?;
            rows.push(CompareRow {
                policy,
                budget_bytes,
                metrics,
            });
        }
    }

    let events = trace.value.len();
    Ok(match args.output {
        OutputFormatArg::Text => render_text(
            args,
            &policies,
            &trace.digest,
            &manifest.digest,
            events,
            &rows,
        ),
        OutputFormatArg::Json => render_json(
            args,
            &policies,
            &trace.digest,
            &manifest.digest,
            events,
            &rows,
        ),
        OutputFormatArg::Csv => render_csv(args, &trace.digest, &manifest.digest, events, &rows),
    })
}

/// Rejects per-layer flags: `compare` sweeps one global budget in `v0.1`.
fn ensure_global_scope(args: &CompareArgs) -> Result<(), CliError> {
    let message = if !args.layer_quota_bytes.is_empty() {
        "--layer-quota-bytes is not supported by compare in v0.1; use run for per-layer replays"
    } else if args.cache_scope == CacheScopeArg::PerLayer {
        "--cache-scope per-layer is not supported by compare in v0.1; use run for per-layer replays"
    } else {
        return Ok(());
    };
    Err(CliError::Usage {
        message: message.to_owned(),
    })
}

/// Resolves the policy list, rejecting duplicates but preserving order.
fn distinct_policies(args: &CompareArgs) -> Result<Vec<Policy>, CliError> {
    let policies: Vec<Policy> = args.policies.iter().map(|&arg| Policy::from(arg)).collect();
    for (index, policy) in policies.iter().enumerate() {
        if policies[..index].contains(policy) {
            return Err(CliError::Usage {
                message: format!("policy {policy} appears more than once in --policies"),
            });
        }
    }
    Ok(policies)
}

/// Rejects duplicate budgets while preserving the caller's order.
fn ensure_distinct_budgets(budgets: &[u64]) -> Result<(), CliError> {
    for (index, budget_bytes) in budgets.iter().enumerate() {
        if budgets[..index].contains(budget_bytes) {
            return Err(CliError::Usage {
                message: format!(
                    "budget {budget_bytes} appears more than once in --global-budgets-bytes"
                ),
            });
        }
    }
    Ok(())
}

/// Comma-joined display values, preserving order.
fn joined<T: std::fmt::Display>(values: &[T]) -> String {
    let mut out = String::new();
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        // fmt::Write to a String is infallible; the Result exists only for
        // the trait.
        let _ = write!(out, "{value}");
    }
    out
}

/// Renders the human-readable report.
fn render_text(
    args: &CompareArgs,
    policies: &[Policy],
    trace_digest: &str,
    manifest_digest: &str,
    events: usize,
    rows: &[CompareRow],
) -> String {
    let mut report = format!(
        "status: ok\n\
         tool_version: {}\n\
         input_format: {INPUT_FORMAT_VERSION}\n\
         trace: {}\n\
         trace_sha256: {trace_digest}\n\
         model_manifest: {}\n\
         model_manifest_sha256: {manifest_digest}\n\
         cache_scope: global\n\
         policies: {}\n\
         global_budgets_bytes: {}\n\
         events: {events}\n",
        tool_version(),
        args.trace.display(),
        args.model_manifest.display(),
        joined(policies),
        joined(&args.global_budgets_bytes),
    );
    if policies.contains(&Policy::Belady) {
        // fmt::Write to a String is infallible; the Result exists only for
        // the trait.
        let _ = writeln!(report, "belady_objective: {BELADY_OBJECTIVE}");
    }
    let _ = writeln!(report, "results: {}", rows.len());
    for row in rows {
        let m = &row.metrics;
        let _ = writeln!(
            report,
            "policy {} budget {}: object_loads: {}, byte_loads: {}, object_hits: {}, \
             byte_hits: {}, object_reloads: {}, byte_reloads: {}, evictions: {}, \
             evicted_bytes: {}, peak_resident_bytes: {}",
            row.policy,
            row.budget_bytes,
            m.object_loads(),
            m.byte_loads(),
            m.object_hits(),
            m.byte_hits(),
            m.object_reloads(),
            m.byte_reloads(),
            m.evictions(),
            m.evicted_bytes(),
            m.peak_resident_bytes(),
        );
    }
    report
}

/// One JSON string literal with proper escaping.
fn json_string(value: &str) -> String {
    serde_json::Value::String(value.to_owned()).to_string()
}

/// Renders one machine-readable JSON object, two-space indented, with the
/// provenance first and one row object per matrix cell.
fn render_json(
    args: &CompareArgs,
    policies: &[Policy],
    trace_digest: &str,
    manifest_digest: &str,
    events: usize,
    rows: &[CompareRow],
) -> String {
    let policy_names = joined(
        &policies
            .iter()
            .map(|policy| json_string(&policy.to_string()))
            .collect::<Vec<_>>(),
    );
    let mut report = format!(
        "{{\n  \"status\": \"ok\",\n  \"tool_version\": {},\n  \"input_format\": {},\n  \
         \"trace\": {},\n  \"trace_sha256\": {},\n  \"model_manifest\": {},\n  \
         \"model_manifest_sha256\": {},\n  \"cache_scope\": \"global\",\n  \
         \"policies\": [{policy_names}],\n  \"global_budgets_bytes\": [{}],\n  \
         \"events\": {events},\n  \"rows\": [\n",
        json_string(tool_version()),
        json_string(INPUT_FORMAT_VERSION),
        json_string(&args.trace.display().to_string()),
        json_string(trace_digest),
        json_string(&args.model_manifest.display().to_string()),
        json_string(manifest_digest),
        joined(&args.global_budgets_bytes),
    );
    for (index, row) in rows.iter().enumerate() {
        let m = &row.metrics;
        let objective = if row.policy == Policy::Belady {
            format!(" \"objective\": {},", json_string(BELADY_OBJECTIVE))
        } else {
            String::new()
        };
        let separator = if index + 1 == rows.len() { "" } else { "," };
        let _ = writeln!(
            report,
            "    {{\"policy\": {}, \"global_budget_bytes\": {},{objective} \
             \"object_loads\": {}, \"byte_loads\": {}, \"object_hits\": {}, \
             \"byte_hits\": {}, \"object_reloads\": {}, \"byte_reloads\": {}, \
             \"evictions\": {}, \"evicted_bytes\": {}, \"peak_resident_bytes\": {}}}{separator}",
            json_string(&row.policy.to_string()),
            row.budget_bytes,
            m.object_loads(),
            m.byte_loads(),
            m.object_hits(),
            m.byte_hits(),
            m.object_reloads(),
            m.byte_reloads(),
            m.evictions(),
            m.evicted_bytes(),
            m.peak_resident_bytes(),
        );
    }
    report.push_str("  ]\n}\n");
    report
}

/// Escapes one CSV field per RFC 4180: quoted when it contains a comma,
/// quote, or line break, with embedded quotes doubled.
fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

/// Renders one CSV table; every row repeats the provenance columns so each
/// line is self-contained.
fn render_csv(
    args: &CompareArgs,
    trace_digest: &str,
    manifest_digest: &str,
    events: usize,
    rows: &[CompareRow],
) -> String {
    let mut report = String::from(
        "tool_version,input_format,trace,trace_sha256,model_manifest,model_manifest_sha256,\
         cache_scope,policy,global_budget_bytes,objective,events,object_loads,byte_loads,\
         object_hits,byte_hits,object_reloads,byte_reloads,evictions,evicted_bytes,\
         peak_resident_bytes\n",
    );
    let provenance = format!(
        "{},{},{},{},{},{},global",
        csv_field(tool_version()),
        csv_field(INPUT_FORMAT_VERSION),
        csv_field(&args.trace.display().to_string()),
        csv_field(trace_digest),
        csv_field(&args.model_manifest.display().to_string()),
        csv_field(manifest_digest),
    );
    for row in rows {
        let m = &row.metrics;
        let objective = if row.policy == Policy::Belady {
            csv_field(BELADY_OBJECTIVE)
        } else {
            String::new()
        };
        let _ = writeln!(
            report,
            "{provenance},{},{},{objective},{events},{},{},{},{},{},{},{},{},{}",
            row.policy,
            row.budget_bytes,
            m.object_loads(),
            m.byte_loads(),
            m.object_hits(),
            m.byte_hits(),
            m.object_reloads(),
            m.byte_reloads(),
            m.evictions(),
            m.evicted_bytes(),
            m.peak_resident_bytes(),
        );
    }
    report
}
