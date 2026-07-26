//! End-to-end tests for the `moe-sim` binary over committed fixtures.
//!
//! Each test spawns the real binary via `CARGO_BIN_EXE_moe-sim` and pins the
//! frozen process contract:
//!
//! - exit codes: 0 ok, 2 bad argv, 3 file I/O (read, UTF-8, path, write), 4
//!   parse/domain wire, 5 capacity rejection, 6 replay failure;
//! - stdout carries only complete success reports; failures never emit partial
//!   stdout;
//! - stderr carries one `error:` line with the full typed error chain.
//!
//! Commands run from the workspace root with relative fixture paths so
//! success reports are byte-identical across machines.

#![expect(
    unused_crate_dependencies,
    reason = "integration-test targets receive every package dependency; this test drives the compiled binary through std::process only"
)]
#![expect(
    clippy::unwrap_used,
    reason = "process spawning and UTF-8 decoding of captured output may unwrap; a panic with location is the most useful test diagnostic"
)]

use std::path::PathBuf;
use std::process::{Command, Output};

// Expected input digests, produced outside this crate with
// `shasum -a 256 <fixture>`. Pinning externally computed values keeps the
// provenance contract honest: a report a reader cannot reproduce with a
// standard tool would fail here.
const ACTIVE_SET_SHA256: &str = "ba96fdf54901d5f93e090714c539b63aa748b1b845434a92522a77dee3744556";
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const TWO_EXPERTS_SHA256: &str = "543e2c3b70c52392b615dec923aa0c6a99a90ee88248ae5106b3093a89165538";
const TWO_LAYERS_TRACE_SHA256: &str =
    "35c61891c72dba7d6eeac758215f320afbef900e106646face1c58a3b268f824";
const TWO_LAYERS_MANIFEST_SHA256: &str =
    "1c94b98f26c0f18f85a5aaca95b1a2d70ea8a1befeb55d8d8a893e792c0d7596";
const THREE_EXPERTS_CYCLE_TRACE_SHA256: &str =
    "8005f20747211b4fb2da49c5d68606f6229c940aa3f61a4f4828e5646b33eaaf";
const THREE_EXPERTS_UNIFORM_MANIFEST_SHA256: &str =
    "3b1a153a9c889a77c78229ff440ab8f071326f317eac6ace984454048f2694e5";

/// Runs the `moe-sim` binary from the workspace root with `args`.
fn moe_sim(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_moe-sim"))
        .args(args)
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .output()
        .unwrap()
}

/// Captured stdout as UTF-8.
fn stdout(output: &Output) -> &str {
    std::str::from_utf8(&output.stdout).unwrap()
}

/// Captured stderr as UTF-8.
fn stderr(output: &Output) -> &str {
    std::str::from_utf8(&output.stderr).unwrap()
}

/// Value of one `key: value` line in a report.
fn field<'a>(report: &'a str, key: &str) -> &'a str {
    let prefix = format!("{key}: ");
    report
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("report has no `{key}` line:\n{report}"))
}

// Success paths (exit 0).

#[test]
fn trace_inspect_summarizes_the_active_set_fixture() {
    let output = moe_sim(&[
        "trace",
        "inspect",
        "--trace",
        "fixtures/synthetic/active-set-0-1.jsonl",
    ]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert_eq!(
        stdout(&output),
        format!(
            "status: ok\n\
         tool_version: {}\n\
         input_format: v1\n\
         trace: fixtures/synthetic/active-set-0-1.jsonl\n\
         trace_sha256: {ACTIVE_SET_SHA256}\n\
         events: 2\n\
         requests: 1\n\
         layers: 1\n\
         expert_activations: 3\n\
         phase_prefill: 1\n\
         phase_decode: 1\n\
         phase_unknown: 0\n",
            env!("CARGO_PKG_VERSION")
        )
    );
    assert_eq!(stderr(&output), "");
}

#[test]
fn trace_inspect_of_the_empty_fixture_reports_zero_counts() {
    let output = moe_sim(&[
        "trace",
        "inspect",
        "--trace",
        "fixtures/synthetic/empty.jsonl",
    ]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert_eq!(
        stdout(&output),
        format!(
            "status: ok\n\
         tool_version: {}\n\
         input_format: v1\n\
         trace: fixtures/synthetic/empty.jsonl\n\
         trace_sha256: {EMPTY_SHA256}\n\
         events: 0\n\
         requests: 0\n\
         layers: 0\n\
         expert_activations: 0\n\
         phase_prefill: 0\n\
         phase_decode: 0\n\
         phase_unknown: 0\n",
            env!("CARGO_PKG_VERSION")
        )
    );
    assert_eq!(stderr(&output), "");
}

#[test]
fn capacity_check_accepts_the_exact_fit_budget_10() {
    let output = moe_sim(&[
        "capacity",
        "check",
        "--trace",
        "fixtures/synthetic/active-set-0-1.jsonl",
        "--model-manifest",
        "fixtures/models/two-experts-4-6.json",
        "--global-budget-bytes",
        "10",
    ]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert_eq!(
        stdout(&output),
        format!(
            "status: ok\n\
         tool_version: {}\n\
         input_format: v1\n\
         trace: fixtures/synthetic/active-set-0-1.jsonl\n\
         trace_sha256: {ACTIVE_SET_SHA256}\n\
         model_manifest: fixtures/models/two-experts-4-6.json\n\
         model_manifest_sha256: {TWO_EXPERTS_SHA256}\n\
         global_budget_bytes: 10\n\
         cache_scope: global\n\
         events: 2\n\
         manifest_experts: 2\n",
            env!("CARGO_PKG_VERSION")
        )
    );
    assert_eq!(stderr(&output), "");
}

// `run` (slice 1A): replay under one policy.

#[test]
fn run_no_cache_matches_the_hand_calculated_fixture() {
    // fixtures/synthetic/active-set-0-1.jsonl over two-experts-4-6.json:
    //   event 0 activates {0, 1} -> 4 + 6 = 10 bytes, 2 objects
    //   event 1 activates {1}    ->         6 bytes,  1 object
    // No retention, so every activation is a load: 3 objects, 16 bytes,
    // and residency peaks at the largest atomic set, 10 bytes.
    let output = moe_sim(&[
        "run",
        "--trace",
        "fixtures/synthetic/active-set-0-1.jsonl",
        "--model-manifest",
        "fixtures/models/two-experts-4-6.json",
        "--global-budget-bytes",
        "10",
        "--policy",
        "no-cache",
    ]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert_eq!(
        stdout(&output),
        format!(
            "status: ok\n\
         tool_version: {}\n\
         input_format: v1\n\
         trace: fixtures/synthetic/active-set-0-1.jsonl\n\
         trace_sha256: {ACTIVE_SET_SHA256}\n\
         model_manifest: fixtures/models/two-experts-4-6.json\n\
         model_manifest_sha256: {TWO_EXPERTS_SHA256}\n\
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
            env!("CARGO_PKG_VERSION")
        )
    );
    assert_eq!(stderr(&output), "");
}

#[test]
fn run_lru_retains_the_shared_expert_across_events() {
    // Same fixture: event 0 loads {0, 1} = 10 bytes, event 1 needs {1} alone.
    // LRU still holds expert 1, so the second event is a 6-byte hit instead of
    // the reload the no-cache baseline pays.
    let output = moe_sim(&[
        "run",
        "--trace",
        "fixtures/synthetic/active-set-0-1.jsonl",
        "--model-manifest",
        "fixtures/models/two-experts-4-6.json",
        "--global-budget-bytes",
        "10",
        "--policy",
        "lru",
    ]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let report = stdout(&output);
    assert_eq!(field(report, "policy"), "lru");
    assert_eq!(field(report, "object_loads"), "2");
    assert_eq!(field(report, "byte_loads"), "10");
    assert_eq!(field(report, "object_hits"), "1");
    assert_eq!(field(report, "byte_hits"), "6");
    assert_eq!(field(report, "object_reloads"), "0");
    assert_eq!(field(report, "evictions"), "0");
    assert_eq!(field(report, "peak_resident_bytes"), "10");
}

#[test]
fn every_policy_keeps_residency_within_the_budget() {
    // The gate invariant, checked through the real binary rather than only in
    // core: no policy may report residency above the budget it was given.
    for policy in ["no-cache", "lru", "lfu"] {
        let output = moe_sim(&[
            "run",
            "--trace",
            "fixtures/synthetic/active-set-0-1.jsonl",
            "--model-manifest",
            "fixtures/models/two-experts-4-6.json",
            "--global-budget-bytes",
            "10",
            "--policy",
            policy,
        ]);
        assert_eq!(output.status.code(), Some(0), "{policy}");
        let peak: u64 = field(stdout(&output), "peak_resident_bytes")
            .parse()
            .unwrap();
        assert!(peak <= 10, "{policy} reported peak residency {peak}");
    }
}

#[test]
fn every_policy_is_rejected_on_an_infeasible_budget() {
    for policy in ["no-cache", "lru", "lfu"] {
        let output = moe_sim(&[
            "run",
            "--trace",
            "fixtures/synthetic/active-set-0-1.jsonl",
            "--model-manifest",
            "fixtures/models/two-experts-4-6.json",
            "--global-budget-bytes",
            "9",
            "--policy",
            policy,
        ]);
        assert_eq!(output.status.code(), Some(5), "{policy}");
        assert_eq!(stdout(&output), "", "{policy} emitted metrics anyway");
    }
}

#[test]
fn run_object_loads_equal_the_activation_count_reported_by_trace_inspect() {
    // Cross-check between two independent commands: with no retention every
    // activation is one load, so `run` must agree with `trace inspect`.
    let inspect = moe_sim(&[
        "trace",
        "inspect",
        "--trace",
        "fixtures/synthetic/active-set-0-1.jsonl",
    ]);
    let run = moe_sim(&[
        "run",
        "--trace",
        "fixtures/synthetic/active-set-0-1.jsonl",
        "--model-manifest",
        "fixtures/models/two-experts-4-6.json",
        "--global-budget-bytes",
        "10",
        "--policy",
        "no-cache",
    ]);

    let activations = field(stdout(&inspect), "expert_activations");
    let object_loads = field(stdout(&run), "object_loads");
    assert_eq!(activations, object_loads);
}

#[test]
fn run_rejects_an_infeasible_budget_before_emitting_metrics() {
    // The atomic set {0, 1} needs 10 bytes; each expert alone fits in 9.
    let output = moe_sim(&[
        "run",
        "--trace",
        "fixtures/synthetic/active-set-0-1.jsonl",
        "--model-manifest",
        "fixtures/models/two-experts-4-6.json",
        "--global-budget-bytes",
        "9",
        "--policy",
        "no-cache",
    ]);
    assert_eq!(output.status.code(), Some(5));
    assert_eq!(stdout(&output), "", "rejection must not emit metrics");
    assert!(
        stderr(&output).starts_with("error: capacity check failed:"),
        "unexpected stderr: {}",
        stderr(&output)
    );
}

#[test]
fn run_on_the_empty_trace_reports_zeroed_metrics() {
    let output = moe_sim(&[
        "run",
        "--trace",
        "fixtures/synthetic/empty.jsonl",
        "--model-manifest",
        "fixtures/models/empty.json",
        "--global-budget-bytes",
        "0",
        "--policy",
        "no-cache",
    ]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let report = stdout(&output);
    for key in [
        "events",
        "object_loads",
        "byte_loads",
        "object_hits",
        "byte_hits",
        "object_reloads",
        "byte_reloads",
        "evictions",
        "evicted_bytes",
        "peak_resident_bytes",
    ] {
        assert_eq!(field(report, key), "0", "{key} must be zero: {report}");
    }
}

#[test]
fn run_without_a_policy_is_a_usage_error() {
    let output = moe_sim(&[
        "run",
        "--trace",
        "fixtures/synthetic/active-set-0-1.jsonl",
        "--model-manifest",
        "fixtures/models/two-experts-4-6.json",
        "--global-budget-bytes",
        "10",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("--policy"),
        "stderr must name the missing flag, got: {}",
        stderr(&output)
    );
}

#[test]
fn run_with_an_unknown_policy_is_a_usage_error() {
    let output = moe_sim(&[
        "run",
        "--trace",
        "fixtures/synthetic/active-set-0-1.jsonl",
        "--model-manifest",
        "fixtures/models/two-experts-4-6.json",
        "--global-budget-bytes",
        "10",
        "--policy",
        // Not a policy this tool declares: an unsupported policy must be
        // refused, never silently approximated by another one.
        "mru",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("mru"),
        "stderr must name the rejected policy, got: {}",
        stderr(&output)
    );
}

// Bad argv (exit 2, reported by clap).

#[test]
fn missing_subcommand_is_a_usage_error() {
    let output = moe_sim(&[]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "", "usage errors must not write stdout");
}

#[test]
fn trace_inspect_without_the_trace_flag_is_a_usage_error() {
    let output = moe_sim(&["trace", "inspect"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("--trace"),
        "stderr must name the missing flag, got: {}",
        stderr(&output)
    );
}

#[test]
fn positional_arguments_are_rejected() {
    let output = moe_sim(&["trace", "inspect", "fixtures/synthetic/empty.jsonl"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
}

#[test]
fn non_numeric_budget_is_a_usage_error() {
    let output = moe_sim(&[
        "capacity",
        "check",
        "--trace",
        "fixtures/synthetic/active-set-0-1.jsonl",
        "--model-manifest",
        "fixtures/models/two-experts-4-6.json",
        "--global-budget-bytes",
        "ten",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
}

#[test]
fn negative_budget_is_a_usage_error() {
    let output = moe_sim(&[
        "capacity",
        "check",
        "--trace",
        "fixtures/synthetic/active-set-0-1.jsonl",
        "--model-manifest",
        "fixtures/models/two-experts-4-6.json",
        "--global-budget-bytes=-1",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
}

// File I/O failures (exit 3): reads here, writes in the generate section.

#[test]
fn trace_inspect_of_a_missing_file_exits_3() {
    let output = moe_sim(&[
        "trace",
        "inspect",
        "--trace",
        "fixtures/synthetic/absent.jsonl",
    ]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output)
            .starts_with("error: failed to read input file fixtures/synthetic/absent.jsonl:"),
        "unexpected stderr: {}",
        stderr(&output)
    );
}

#[test]
fn capacity_check_with_a_missing_manifest_exits_3() {
    // The trace is valid; the manifest read fails afterwards.
    let output = moe_sim(&[
        "capacity",
        "check",
        "--trace",
        "fixtures/synthetic/active-set-0-1.jsonl",
        "--model-manifest",
        "fixtures/models/absent.json",
        "--global-budget-bytes",
        "10",
    ]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(stdout(&output), "", "failures must not emit partial stdout");
    assert!(
        stderr(&output)
            .starts_with("error: failed to read input file fixtures/models/absent.json:"),
        "unexpected stderr: {}",
        stderr(&output)
    );
}

#[test]
fn trace_inspect_of_a_non_utf8_file_exits_3() {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("non-utf8.jsonl");
    std::fs::write(&path, [0xff, 0xfe, 0xfd]).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_moe-sim"))
        .args(["trace", "inspect", "--trace"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).starts_with("error: failed to read input file "),
        "unexpected stderr: {}",
        stderr(&output)
    );
}

// Parse failures (exit 4).

#[test]
fn trace_inspect_of_the_blank_line_fixture_exits_4() {
    let output = moe_sim(&[
        "trace",
        "inspect",
        "--trace",
        "fixtures/synthetic/invalid/blank-line.jsonl",
    ]);
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "error: failed to parse trace fixtures/synthetic/invalid/blank-line.jsonl: \
         line 2: blank lines are not allowed in a JSONL trace\n"
    );
}

#[test]
fn trace_inspect_of_the_duplicate_expert_fixture_exits_4() {
    let output = moe_sim(&[
        "trace",
        "inspect",
        "--trace",
        "fixtures/synthetic/invalid/duplicate-expert-id.jsonl",
    ]);
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "error: failed to parse trace fixtures/synthetic/invalid/duplicate-expert-id.jsonl: \
         line 1: invalid activation event: duplicate expert id 3 in one activation event\n"
    );
}

#[test]
fn capacity_check_with_the_zero_size_manifest_exits_4() {
    let output = moe_sim(&[
        "capacity",
        "check",
        "--trace",
        "fixtures/synthetic/active-set-0-1.jsonl",
        "--model-manifest",
        "fixtures/models/invalid/zero-size.json",
        "--global-budget-bytes",
        "10",
    ]);
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "error: failed to parse model manifest fixtures/models/invalid/zero-size.json: \
         invalid model manifest entries: expert size must be positive: layer 0 expert 2 has size 0\n"
    );
}

#[test]
fn capacity_check_with_the_empty_document_manifest_exits_4() {
    let output = moe_sim(&[
        "capacity",
        "check",
        "--trace",
        "fixtures/synthetic/active-set-0-1.jsonl",
        "--model-manifest",
        "fixtures/models/invalid/empty-document.json",
        "--global-budget-bytes",
        "10",
    ]);
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).starts_with(
            "error: failed to parse model manifest fixtures/models/invalid/empty-document.json:"
        ),
        "unexpected stderr: {}",
        stderr(&output)
    );
}

// Capacity rejections (exit 5).

#[test]
fn capacity_check_rejects_the_atomic_active_set_at_budget_9() {
    // Both experts (4 B, 6 B) individually fit budget 9; the atomic active
    // set {0, 1} of the first event does not.
    let output = moe_sim(&[
        "capacity",
        "check",
        "--trace",
        "fixtures/synthetic/active-set-0-1.jsonl",
        "--model-manifest",
        "fixtures/models/two-experts-4-6.json",
        "--global-budget-bytes",
        "9",
    ]);
    assert_eq!(output.status.code(), Some(5));
    assert_eq!(stdout(&output), "", "failures must not emit partial stdout");
    assert_eq!(
        stderr(&output),
        "error: capacity check failed: active set exceeds global capacity: \
         event 0 request 1 layer 0 totals 10 bytes, global budget is 9 bytes\n"
    );
}

#[test]
fn capacity_check_rejects_an_oversize_manifest_expert_at_budget_5() {
    // The manifest pass rejects expert 1 (6 B) before any event is checked.
    let output = moe_sim(&[
        "capacity",
        "check",
        "--trace",
        "fixtures/synthetic/empty.jsonl",
        "--model-manifest",
        "fixtures/models/two-experts-4-6.json",
        "--global-budget-bytes",
        "5",
    ]);
    assert_eq!(output.status.code(), Some(5));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "error: capacity check failed: expert exceeds global capacity: \
         layer 0 expert 1 has size 6 bytes, global budget is 5 bytes\n"
    );
}

#[test]
fn capacity_check_reports_an_unknown_expert_with_event_context() {
    let output = moe_sim(&[
        "capacity",
        "check",
        "--trace",
        "fixtures/synthetic/expert-2.jsonl",
        "--model-manifest",
        "fixtures/models/two-experts-4-6.json",
        "--global-budget-bytes",
        "10",
    ]);
    assert_eq!(output.status.code(), Some(5));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "error: capacity check failed: failed to calculate active-set bytes for \
         event 0 (request 1, layer 0): unknown expert in model manifest: layer 0 expert 2\n"
    );
}

// Per-layer quotas (slice 1B, second half).

#[test]
fn run_per_layer_matches_the_hand_calculated_two_layer_fixture() {
    // fixtures/synthetic/two-layers.jsonl over two-layers.json, quotas
    // layer 0: 10, layer 1: 5, total budget 15, LRU:
    //   event 0  L0 {0, 1} -> loads 4 + 6, layer 0 full
    //   event 1  L1 {0}    -> loads 5, layer 1 full; total residency peaks at 15
    //   event 2  L0 {1}    -> 6-byte hit
    //   event 3  L1 {1}    -> evicts expert (1, 0) inside layer 1, loads 3
    let output = moe_sim(&[
        "run",
        "--trace",
        "fixtures/synthetic/two-layers.jsonl",
        "--model-manifest",
        "fixtures/models/two-layers.json",
        "--global-budget-bytes",
        "15",
        "--cache-scope",
        "per-layer",
        "--layer-quota-bytes",
        "0:10",
        "--layer-quota-bytes",
        "1:5",
        "--policy",
        "lru",
    ]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert_eq!(
        stdout(&output),
        format!(
            "status: ok\n\
         tool_version: {}\n\
         input_format: v1\n\
         trace: fixtures/synthetic/two-layers.jsonl\n\
         trace_sha256: {TWO_LAYERS_TRACE_SHA256}\n\
         model_manifest: fixtures/models/two-layers.json\n\
         model_manifest_sha256: {TWO_LAYERS_MANIFEST_SHA256}\n\
         global_budget_bytes: 15\n\
         cache_scope: per-layer\n\
         policy: lru\n\
         events: 4\n\
         object_loads: 4\n\
         byte_loads: 18\n\
         object_hits: 1\n\
         byte_hits: 6\n\
         object_reloads: 0\n\
         byte_reloads: 0\n\
         evictions: 1\n\
         evicted_bytes: 5\n\
         peak_resident_bytes: 15\n\
         layer 0: quota_bytes: 10, peak_resident_bytes: 10\n\
         layer 1: quota_bytes: 5, peak_resident_bytes: 5\n",
            env!("CARGO_PKG_VERSION")
        )
    );
    assert_eq!(stderr(&output), "");
}

#[test]
fn per_layer_peaks_stay_inside_each_quota_for_every_policy() {
    for policy in ["no-cache", "lru", "lfu"] {
        let output = moe_sim(&[
            "run",
            "--trace",
            "fixtures/synthetic/two-layers.jsonl",
            "--model-manifest",
            "fixtures/models/two-layers.json",
            "--global-budget-bytes",
            "15",
            "--cache-scope",
            "per-layer",
            "--layer-quota-bytes",
            "0:10",
            "--layer-quota-bytes",
            "1:5",
            "--policy",
            policy,
        ]);
        assert_eq!(output.status.code(), Some(0), "{policy}");
        let report = stdout(&output);
        let layer_line = |layer: &str| {
            report
                .lines()
                .find(|line| line.starts_with(layer))
                .unwrap_or_else(|| panic!("{policy}: report has no `{layer}` line:\n{report}"))
                .to_owned()
        };
        let peak_of = |line: &str| -> u64 { line.rsplit(": ").next().unwrap().parse().unwrap() };
        assert!(peak_of(&layer_line("layer 0:")) <= 10, "{policy}");
        assert!(peak_of(&layer_line("layer 1:")) <= 5, "{policy}");
    }
}

#[test]
fn capacity_check_accepts_the_per_layer_fixture_quotas() {
    let output = moe_sim(&[
        "capacity",
        "check",
        "--trace",
        "fixtures/synthetic/two-layers.jsonl",
        "--model-manifest",
        "fixtures/models/two-layers.json",
        "--global-budget-bytes",
        "15",
        "--cache-scope",
        "per-layer",
        "--layer-quota-bytes",
        "0:10",
        "--layer-quota-bytes",
        "1:5",
    ]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert_eq!(
        stdout(&output),
        format!(
            "status: ok\n\
         tool_version: {}\n\
         input_format: v1\n\
         trace: fixtures/synthetic/two-layers.jsonl\n\
         trace_sha256: {TWO_LAYERS_TRACE_SHA256}\n\
         model_manifest: fixtures/models/two-layers.json\n\
         model_manifest_sha256: {TWO_LAYERS_MANIFEST_SHA256}\n\
         global_budget_bytes: 15\n\
         cache_scope: per-layer\n\
         events: 4\n\
         manifest_experts: 4\n\
         layer 0: quota_bytes: 10\n\
         layer 1: quota_bytes: 5\n",
            env!("CARGO_PKG_VERSION")
        )
    );
    assert_eq!(stderr(&output), "");
}

#[test]
fn a_quota_sum_above_the_total_budget_is_a_capacity_rejection() {
    let output = moe_sim(&[
        "capacity",
        "check",
        "--trace",
        "fixtures/synthetic/two-layers.jsonl",
        "--model-manifest",
        "fixtures/models/two-layers.json",
        "--global-budget-bytes",
        "14",
        "--cache-scope",
        "per-layer",
        "--layer-quota-bytes",
        "0:10",
        "--layer-quota-bytes",
        "1:5",
    ]);
    assert_eq!(output.status.code(), Some(5));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "error: capacity check failed: layer quotas exceed the total budget: \
         quotas sum to 15 bytes, total budget is 14 bytes\n"
    );
}

#[test]
fn an_active_set_larger_than_its_layer_quota_is_rejected_before_any_run() {
    // Layer 0's atomic set {0, 1} is 10 bytes; a 9-byte quota rejects it even
    // though the total budget would hold it.
    for policy in ["no-cache", "lru", "lfu"] {
        let output = moe_sim(&[
            "run",
            "--trace",
            "fixtures/synthetic/two-layers.jsonl",
            "--model-manifest",
            "fixtures/models/two-layers.json",
            "--global-budget-bytes",
            "100",
            "--cache-scope",
            "per-layer",
            "--layer-quota-bytes",
            "0:9",
            "--layer-quota-bytes",
            "1:8",
            "--policy",
            policy,
        ]);
        assert_eq!(output.status.code(), Some(5), "{policy}");
        assert_eq!(stdout(&output), "", "{policy} emitted metrics anyway");
        assert_eq!(
            stderr(&output),
            "error: capacity check failed: active set exceeds layer quota: \
             event 0 request 1 layer 0 totals 10 bytes, layer quota is 9 bytes\n",
            "{policy}"
        );
    }
}

#[test]
fn an_activated_layer_without_a_quota_is_a_capacity_rejection() {
    let output = moe_sim(&[
        "run",
        "--trace",
        "fixtures/synthetic/two-layers.jsonl",
        "--model-manifest",
        "fixtures/models/two-layers.json",
        "--global-budget-bytes",
        "15",
        "--cache-scope",
        "per-layer",
        "--layer-quota-bytes",
        "0:10",
        "--policy",
        "lru",
    ]);
    assert_eq!(output.status.code(), Some(5));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "error: capacity check failed: missing layer quota: event 1 (request 1) \
         activates layer 1, which has no explicit quota\n"
    );
}

// Scope and quota flags that contradict each other are argument errors.

#[test]
fn quotas_under_a_global_scope_are_a_usage_error() {
    let output = moe_sim(&[
        "run",
        "--trace",
        "fixtures/synthetic/two-layers.jsonl",
        "--model-manifest",
        "fixtures/models/two-layers.json",
        "--global-budget-bytes",
        "15",
        "--layer-quota-bytes",
        "0:10",
        "--policy",
        "lru",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "error: --layer-quota-bytes requires --cache-scope per-layer\n"
    );
}

#[test]
fn a_per_layer_scope_without_quotas_is_a_usage_error() {
    let output = moe_sim(&[
        "run",
        "--trace",
        "fixtures/synthetic/two-layers.jsonl",
        "--model-manifest",
        "fixtures/models/two-layers.json",
        "--global-budget-bytes",
        "15",
        "--cache-scope",
        "per-layer",
        "--policy",
        "lru",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "error: --cache-scope per-layer requires at least one --layer-quota-bytes\n"
    );
}

#[test]
fn a_layer_quoted_twice_is_a_usage_error() {
    let output = moe_sim(&[
        "capacity",
        "check",
        "--trace",
        "fixtures/synthetic/two-layers.jsonl",
        "--model-manifest",
        "fixtures/models/two-layers.json",
        "--global-budget-bytes",
        "15",
        "--cache-scope",
        "per-layer",
        "--layer-quota-bytes",
        "0:10",
        "--layer-quota-bytes",
        "0:5",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "error: layer 0 has more than one --layer-quota-bytes\n"
    );
}

#[test]
fn a_malformed_quota_pair_is_rejected_by_argument_parsing() {
    for bad in ["0", "a:5", "0:b", ":5", "0:"] {
        let output = moe_sim(&[
            "run",
            "--trace",
            "fixtures/synthetic/two-layers.jsonl",
            "--model-manifest",
            "fixtures/models/two-layers.json",
            "--global-budget-bytes",
            "15",
            "--cache-scope",
            "per-layer",
            "--layer-quota-bytes",
            bad,
            "--policy",
            "lru",
        ]);
        assert_eq!(output.status.code(), Some(2), "`{bad}` was accepted");
        assert_eq!(stdout(&output), "", "`{bad}` emitted a report");
    }
}

// Belady offline reference (slice 1C).

#[test]
fn run_belady_matches_the_hand_calculated_cycle_fixture() {
    // fixtures/synthetic/three-experts-cycle.jsonl cycles [0],[1],[2] twice
    // over three uniform 2-byte experts with room for two. Farthest-next-use
    // loads 4 objects (LRU thrashes to 6), and the report labels the offline
    // objective right after the policy.
    let output = moe_sim(&[
        "run",
        "--trace",
        "fixtures/synthetic/three-experts-cycle.jsonl",
        "--model-manifest",
        "fixtures/models/three-experts-uniform.json",
        "--global-budget-bytes",
        "4",
        "--policy",
        "belady",
    ]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert_eq!(
        stdout(&output),
        format!(
            "status: ok\n\
         tool_version: {}\n\
         input_format: v1\n\
         trace: fixtures/synthetic/three-experts-cycle.jsonl\n\
         trace_sha256: {THREE_EXPERTS_CYCLE_TRACE_SHA256}\n\
         model_manifest: fixtures/models/three-experts-uniform.json\n\
         model_manifest_sha256: {THREE_EXPERTS_UNIFORM_MANIFEST_SHA256}\n\
         global_budget_bytes: 4\n\
         cache_scope: global\n\
         policy: belady\n\
         objective: minimum object loads (offline reference, uniform expert sizes, whole-trace lookahead)\n\
         events: 6\n\
         object_loads: 4\n\
         byte_loads: 8\n\
         object_hits: 2\n\
         byte_hits: 4\n\
         object_reloads: 1\n\
         byte_reloads: 2\n\
         evictions: 2\n\
         evicted_bytes: 4\n\
         peak_resident_bytes: 4\n",
            env!("CARGO_PKG_VERSION")
        )
    );
    assert_eq!(stderr(&output), "");
}

#[test]
fn run_lru_thrashes_on_the_cycle_fixture() {
    // The same cycle under LRU always evicts the expert needed next: every
    // activation is a load, and no objective line appears because LRU is an
    // online policy, not an offline reference.
    let output = moe_sim(&[
        "run",
        "--trace",
        "fixtures/synthetic/three-experts-cycle.jsonl",
        "--model-manifest",
        "fixtures/models/three-experts-uniform.json",
        "--global-budget-bytes",
        "4",
        "--policy",
        "lru",
    ]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let report = stdout(&output);
    assert!(!report.contains("objective:"), "report:\n{report}");
    assert_eq!(field(report, "policy"), "lru");
    assert_eq!(field(report, "object_loads"), "6");
    assert_eq!(field(report, "byte_loads"), "12");
    assert_eq!(field(report, "object_hits"), "0");
    assert_eq!(field(report, "object_reloads"), "3");
    assert_eq!(field(report, "evictions"), "4");
    assert_eq!(field(report, "peak_resident_bytes"), "4");
}

#[test]
fn run_belady_under_per_layer_quotas_reports_the_layer_line() {
    // A single-layer quota equal to the global budget must reproduce the
    // global belady metrics and append the layer's quota/peak pairing.
    let output = moe_sim(&[
        "run",
        "--trace",
        "fixtures/synthetic/three-experts-cycle.jsonl",
        "--model-manifest",
        "fixtures/models/three-experts-uniform.json",
        "--global-budget-bytes",
        "4",
        "--cache-scope",
        "per-layer",
        "--layer-quota-bytes",
        "0:4",
        "--policy",
        "belady",
    ]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let report = stdout(&output);
    assert_eq!(field(report, "cache_scope"), "per-layer");
    assert_eq!(
        field(report, "objective"),
        "minimum object loads (offline reference, uniform expert sizes, whole-trace lookahead)"
    );
    assert_eq!(field(report, "object_loads"), "4");
    assert_eq!(field(report, "object_hits"), "2");
    assert_eq!(field(report, "peak_resident_bytes"), "4");
    assert!(
        report.ends_with("layer 0: quota_bytes: 4, peak_resident_bytes: 4\n"),
        "report:\n{report}"
    );
}

#[test]
fn run_belady_on_a_variable_size_manifest_exits_6() {
    // Applicability is rejected at replay time with the two differing
    // experts named, after the capacity pass accepted the configuration:
    // exit 6, no partial stdout.
    let output = moe_sim(&[
        "run",
        "--trace",
        "fixtures/synthetic/two-layers.jsonl",
        "--model-manifest",
        "fixtures/models/two-layers.json",
        "--global-budget-bytes",
        "15",
        "--policy",
        "belady",
    ]);
    assert_eq!(output.status.code(), Some(6));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "error: replay failed: belady requires a uniform expert size: \
         layer 0 expert 0 has 4 bytes, layer 0 expert 1 has 6 bytes\n"
    );
}

// `compare` (slice 1D): one policy and budget matrix, three output formats.

/// The compare invocation every matrix test uses: two budgets, all four
/// policies, over the committed cycle fixture.
fn compare_cycle(extra: &[&str]) -> Output {
    let mut args = vec![
        "compare",
        "--trace",
        "fixtures/synthetic/three-experts-cycle.jsonl",
        "--model-manifest",
        "fixtures/models/three-experts-uniform.json",
        "--global-budgets-bytes",
        "4,6",
        "--policies",
        "no-cache,lru,lfu,belady",
    ];
    args.extend_from_slice(extra);
    moe_sim(&args)
}

const COMPARE_OBJECTIVE: &str =
    "minimum object loads (offline reference, uniform expert sizes, whole-trace lookahead)";

#[test]
fn compare_text_report_matches_the_hand_calculated_matrix() {
    // Metrics per cell mirror the byte-exact `run` tests on this fixture:
    // a two-object budget where LRU/LFU thrash and belady loads 4, and a
    // three-object budget where every retaining policy holds the set.
    let output = compare_cycle(&[]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert_eq!(
        stdout(&output),
        format!(
            "status: ok\n\
         tool_version: {}\n\
         input_format: v1\n\
         trace: fixtures/synthetic/three-experts-cycle.jsonl\n\
         trace_sha256: {THREE_EXPERTS_CYCLE_TRACE_SHA256}\n\
         model_manifest: fixtures/models/three-experts-uniform.json\n\
         model_manifest_sha256: {THREE_EXPERTS_UNIFORM_MANIFEST_SHA256}\n\
         cache_scope: global\n\
         policies: no-cache,lru,lfu,belady\n\
         global_budgets_bytes: 4,6\n\
         events: 6\n\
         belady_objective: {COMPARE_OBJECTIVE}\n\
         results: 8\n\
         policy no-cache budget 4: object_loads: 6, byte_loads: 12, object_hits: 0, byte_hits: 0, object_reloads: 3, byte_reloads: 6, evictions: 0, evicted_bytes: 0, peak_resident_bytes: 2\n\
         policy no-cache budget 6: object_loads: 6, byte_loads: 12, object_hits: 0, byte_hits: 0, object_reloads: 3, byte_reloads: 6, evictions: 0, evicted_bytes: 0, peak_resident_bytes: 2\n\
         policy lru budget 4: object_loads: 6, byte_loads: 12, object_hits: 0, byte_hits: 0, object_reloads: 3, byte_reloads: 6, evictions: 4, evicted_bytes: 8, peak_resident_bytes: 4\n\
         policy lru budget 6: object_loads: 3, byte_loads: 6, object_hits: 3, byte_hits: 6, object_reloads: 0, byte_reloads: 0, evictions: 0, evicted_bytes: 0, peak_resident_bytes: 6\n\
         policy lfu budget 4: object_loads: 6, byte_loads: 12, object_hits: 0, byte_hits: 0, object_reloads: 3, byte_reloads: 6, evictions: 4, evicted_bytes: 8, peak_resident_bytes: 4\n\
         policy lfu budget 6: object_loads: 3, byte_loads: 6, object_hits: 3, byte_hits: 6, object_reloads: 0, byte_reloads: 0, evictions: 0, evicted_bytes: 0, peak_resident_bytes: 6\n\
         policy belady budget 4: object_loads: 4, byte_loads: 8, object_hits: 2, byte_hits: 4, object_reloads: 1, byte_reloads: 2, evictions: 2, evicted_bytes: 4, peak_resident_bytes: 4\n\
         policy belady budget 6: object_loads: 3, byte_loads: 6, object_hits: 3, byte_hits: 6, object_reloads: 0, byte_reloads: 0, evictions: 0, evicted_bytes: 0, peak_resident_bytes: 6\n",
            env!("CARGO_PKG_VERSION")
        )
    );
    assert_eq!(stderr(&output), "");
}

#[test]
fn compare_json_report_is_byte_stable_and_labels_belady_rows() {
    let output = compare_cycle(&["--output", "json"]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let report = stdout(&output);
    assert!(report.starts_with("{\n  \"status\": \"ok\",\n"), "{report}");
    assert!(
        report.contains(&format!(
            "\"tool_version\": \"{}\",\n",
            env!("CARGO_PKG_VERSION")
        )),
        "{report}"
    );
    assert!(
        report.contains("\"policies\": [\"no-cache\",\"lru\",\"lfu\",\"belady\"],"),
        "{report}"
    );
    assert!(
        report.contains("\"global_budgets_bytes\": [4,6],"),
        "{report}"
    );
    assert!(
        report.contains(
            "{\"policy\": \"belady\", \"global_budget_bytes\": 4, \"objective\": \
             \"minimum object loads (offline reference, uniform expert sizes, whole-trace \
             lookahead)\", \"object_loads\": 4, \"byte_loads\": 8, \"object_hits\": 2, \
             \"byte_hits\": 4, \"object_reloads\": 1, \"byte_reloads\": 2, \"evictions\": 2, \
             \"evicted_bytes\": 4, \"peak_resident_bytes\": 4}"
        ),
        "{report}"
    );
    assert!(
        report.contains(
            "{\"policy\": \"lru\", \"global_budget_bytes\": 6, \"object_loads\": 3, \
             \"byte_loads\": 6, \"object_hits\": 3, \"byte_hits\": 6, \"object_reloads\": 0, \
             \"byte_reloads\": 0, \"evictions\": 0, \"evicted_bytes\": 0, \
             \"peak_resident_bytes\": 6}"
        ),
        "{report}"
    );
    assert!(report.ends_with("  ]\n}\n"), "{report}");
}

#[test]
fn compare_csv_rows_are_self_contained_and_quote_the_objective() {
    let output = compare_cycle(&["--output", "csv"]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let report = stdout(&output);
    let provenance = format!(
        "{},v1,fixtures/synthetic/three-experts-cycle.jsonl,{THREE_EXPERTS_CYCLE_TRACE_SHA256},\
         fixtures/models/three-experts-uniform.json,{THREE_EXPERTS_UNIFORM_MANIFEST_SHA256},global",
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(
        report,
        format!(
            "tool_version,input_format,trace,trace_sha256,model_manifest,model_manifest_sha256,\
             cache_scope,policy,global_budget_bytes,objective,events,object_loads,byte_loads,\
             object_hits,byte_hits,object_reloads,byte_reloads,evictions,evicted_bytes,\
             peak_resident_bytes\n\
             {provenance},no-cache,4,,6,6,12,0,0,3,6,0,0,2\n\
             {provenance},no-cache,6,,6,6,12,0,0,3,6,0,0,2\n\
             {provenance},lru,4,,6,6,12,0,0,3,6,4,8,4\n\
             {provenance},lru,6,,6,3,6,3,6,0,0,0,0,6\n\
             {provenance},lfu,4,,6,6,12,0,0,3,6,4,8,4\n\
             {provenance},lfu,6,,6,3,6,3,6,0,0,0,0,6\n\
             {provenance},belady,4,\"{COMPARE_OBJECTIVE}\",6,4,8,2,4,1,2,2,4,4\n\
             {provenance},belady,6,\"{COMPARE_OBJECTIVE}\",6,3,6,3,6,0,0,0,0,6\n"
        )
    );
}

#[test]
fn compare_reports_are_identical_across_repeated_runs() {
    for format in ["text", "json", "csv"] {
        let first = compare_cycle(&["--output", format]);
        let second = compare_cycle(&["--output", format]);
        assert_eq!(first.status.code(), Some(0));
        assert_eq!(stdout(&first), stdout(&second), "format {format}");
    }
}

#[test]
fn compare_rejects_duplicates_and_per_layer_flags_as_usage_errors() {
    for (extra, expected) in [
        (
            vec!["--policies", "lru,lru", "--global-budgets-bytes", "4"],
            "error: policy lru appears more than once in --policies\n",
        ),
        (
            vec!["--policies", "lru", "--global-budgets-bytes", "4,4"],
            "error: budget 4 appears more than once in --global-budgets-bytes\n",
        ),
        (
            vec![
                "--policies",
                "lru",
                "--global-budgets-bytes",
                "4",
                "--cache-scope",
                "per-layer",
            ],
            "error: --cache-scope per-layer is not supported by compare in v0.1; \
             use run for per-layer replays\n",
        ),
        (
            vec![
                "--policies",
                "lru",
                "--global-budgets-bytes",
                "4",
                "--layer-quota-bytes",
                "0:4",
            ],
            "error: --layer-quota-bytes is not supported by compare in v0.1; \
             use run for per-layer replays\n",
        ),
    ] {
        let mut args = vec![
            "compare",
            "--trace",
            "fixtures/synthetic/three-experts-cycle.jsonl",
            "--model-manifest",
            "fixtures/models/three-experts-uniform.json",
        ];
        args.extend_from_slice(&extra);
        let output = moe_sim(&args);
        assert_eq!(output.status.code(), Some(2), "{extra:?}");
        assert_eq!(stdout(&output), "", "{extra:?}");
        assert_eq!(stderr(&output), expected, "{extra:?}");
    }
}

#[test]
fn compare_names_an_unknown_policy_value_precisely() {
    let output = moe_sim(&[
        "compare",
        "--trace",
        "t",
        "--model-manifest",
        "m",
        "--global-budgets-bytes",
        "4",
        "--policies",
        "lru,mru",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("invalid value 'mru' for '--policies"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn compare_emits_nothing_when_one_selected_combination_is_inapplicable() {
    // belady is selected against a variable-size manifest: the whole
    // comparison is rejected before one byte of report exists, even though
    // the lru cells alone would have succeeded.
    let output = moe_sim(&[
        "compare",
        "--trace",
        "fixtures/synthetic/two-layers.jsonl",
        "--model-manifest",
        "fixtures/models/two-layers.json",
        "--global-budgets-bytes",
        "15",
        "--policies",
        "lru,belady",
    ]);
    assert_eq!(output.status.code(), Some(6));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "error: replay failed: belady requires a uniform expert size: \
         layer 0 expert 0 has 4 bytes, layer 0 expert 1 has 6 bytes\n"
    );
}

#[test]
fn compare_rejects_an_infeasible_budget_anywhere_in_the_sweep() {
    let output = moe_sim(&[
        "compare",
        "--trace",
        "fixtures/synthetic/three-experts-cycle.jsonl",
        "--model-manifest",
        "fixtures/models/three-experts-uniform.json",
        "--global-budgets-bytes",
        "6,1",
        "--policies",
        "lru",
    ]);
    assert_eq!(output.status.code(), Some(5));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "error: capacity check failed: expert exceeds global capacity: \
         layer 0 expert 0 has size 2 bytes, global budget is 1 bytes\n"
    );
}

// `trace generate` (slice 1D): deterministic synthetic inputs on disk.

/// Digests of the generated cyclic 3-expert / 6-event pair, produced outside
/// this crate with `shasum -a 256` on the written files.
const GENERATED_CYCLE_TRACE_SHA256: &str =
    "0681a6723000b94373ab6809ef5ed2d50d8e2a00a4c80c87e5ee9558616a7932";
const GENERATED_CYCLE_MANIFEST_SHA256: &str =
    "822c6fa7b3cd162ec189d5c70c6acf006daad1b2d3ca5535e1240e73d3e04f9e";

/// Workspace-root-relative scratch path for one generated file.
fn generated_path(name: &str) -> String {
    std::fs::create_dir_all(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/test-generate"
    ))
    .unwrap();
    format!("target/test-generate/{name}")
}

#[test]
fn trace_generate_writes_the_pinned_cycle_pair_and_reports_its_digests() {
    let trace_path = generated_path("pinned-cycle.jsonl");
    let manifest_path = generated_path("pinned-cycle-manifest.json");
    let output = moe_sim(&[
        "trace",
        "generate",
        "--pattern",
        "cyclic",
        "--experts",
        "3",
        "--events",
        "6",
        "--out-trace",
        &trace_path,
        "--out-model-manifest",
        &manifest_path,
    ]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert_eq!(
        stdout(&output),
        format!(
            "status: ok\n\
         tool_version: {}\n\
         input_format: v1\n\
         source: synthetic\n\
         pattern: cyclic\n\
         experts: 3\n\
         events: 6\n\
         out_trace: {trace_path}\n\
         out_trace_sha256: {GENERATED_CYCLE_TRACE_SHA256}\n\
         out_model_manifest: {manifest_path}\n\
         out_model_manifest_sha256: {GENERATED_CYCLE_MANIFEST_SHA256}\n",
            env!("CARGO_PKG_VERSION")
        )
    );
    assert_eq!(stderr(&output), "");

    // The tool's own provenance path must agree with the generation report:
    // inspect and capacity check recompute the digests from the files.
    let inspect = moe_sim(&["trace", "inspect", "--trace", &trace_path]);
    assert_eq!(inspect.status.code(), Some(0));
    assert_eq!(
        field(stdout(&inspect), "trace_sha256"),
        GENERATED_CYCLE_TRACE_SHA256
    );
    assert_eq!(field(stdout(&inspect), "events"), "6");

    let check = moe_sim(&[
        "capacity",
        "check",
        "--trace",
        &trace_path,
        "--model-manifest",
        &manifest_path,
        "--global-budget-bytes",
        "2",
    ]);
    assert_eq!(check.status.code(), Some(0), "stderr: {}", stderr(&check));
    assert_eq!(
        field(stdout(&check), "model_manifest_sha256"),
        GENERATED_CYCLE_MANIFEST_SHA256
    );
}

#[test]
fn trace_generate_random_reproduces_from_its_seed_and_diverges_without_it() {
    let mut digests = Vec::new();
    for (name, seed) in [("seed7-a", "7"), ("seed7-b", "7"), ("seed8", "8")] {
        let trace_path = generated_path(&format!("random-{name}.jsonl"));
        let manifest_path = generated_path(&format!("random-{name}-manifest.json"));
        let output = moe_sim(&[
            "trace",
            "generate",
            "--pattern",
            "random",
            "--experts",
            "8",
            "--events",
            "10",
            "--active-per-event",
            "2",
            "--seed",
            seed,
            "--out-trace",
            &trace_path,
            "--out-model-manifest",
            &manifest_path,
        ]);
        assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
        let report = stdout(&output);
        assert_eq!(field(report, "seed"), seed);
        digests.push(field(report, "out_trace_sha256").to_owned());
    }
    assert_eq!(digests[0], digests[1], "same seed must reproduce the trace");
    assert_ne!(digests[0], digests[2], "another seed must diverge");
}

#[test]
fn trace_generate_rejects_parameters_that_do_not_match_the_pattern() {
    for (extra, expected) in [
        (
            vec!["--pattern", "cyclic", "--seed", "1"],
            "error: --seed only applies to --pattern random\n",
        ),
        (
            vec!["--pattern", "random", "--active-per-event", "1"],
            "error: --seed is required by --pattern random\n",
        ),
        (
            vec!["--pattern", "repetition"],
            "error: --active-per-event is required by --pattern repetition\n",
        ),
        (
            vec!["--pattern", "hotset-shift", "--hot", "2"],
            "error: --period is required by --pattern hotset-shift\n",
        ),
        (
            vec!["--pattern", "cyclic", "--hot", "2"],
            "error: --hot only applies to --pattern hotset-shift\n",
        ),
        (
            vec!["--pattern", "variable-sizes", "--active-per-event", "2"],
            "error: --active-per-event only applies to --pattern repetition or random\n",
        ),
    ] {
        let mut args = vec![
            "trace",
            "generate",
            "--experts",
            "4",
            "--events",
            "6",
            "--out-trace",
            "target/test-generate/never-written.jsonl",
            "--out-model-manifest",
            "target/test-generate/never-written.json",
        ];
        args.extend_from_slice(&extra);
        let output = moe_sim(&args);
        assert_eq!(output.status.code(), Some(2), "{extra:?}");
        assert_eq!(stdout(&output), "", "{extra:?}");
        assert_eq!(stderr(&output), expected, "{extra:?}");
    }
}

#[test]
fn trace_generate_rejects_impossible_parameters_from_the_domain() {
    let output = moe_sim(&[
        "trace",
        "generate",
        "--pattern",
        "adversarial-lru",
        "--experts",
        "1",
        "--events",
        "6",
        "--out-trace",
        "target/test-generate/never-written.jsonl",
        "--out-model-manifest",
        "target/test-generate/never-written.json",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "error: synthetic generation failed: \
         the adversarial-lru pattern needs at least two experts: got 1\n"
    );
}

#[test]
fn trace_generate_reports_an_unwritable_path_with_exit_3() {
    let output = moe_sim(&[
        "trace",
        "generate",
        "--pattern",
        "cyclic",
        "--experts",
        "3",
        "--events",
        "6",
        "--out-trace",
        "/nonexistent-moe-sim-dir/t.jsonl",
        "--out-model-manifest",
        "target/test-generate/never-written.json",
    ]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).starts_with("error: failed to write /nonexistent-moe-sim-dir/t.jsonl:"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn compare_preserves_the_callers_policy_and_budget_order() {
    // Descending budgets and a non-canonical policy order must come back
    // exactly as given: the report never sorts the caller's lists.
    let output = moe_sim(&[
        "compare",
        "--trace",
        "fixtures/synthetic/three-experts-cycle.jsonl",
        "--model-manifest",
        "fixtures/models/three-experts-uniform.json",
        "--global-budgets-bytes",
        "6,4",
        "--policies",
        "belady,no-cache",
    ]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let report = stdout(&output);
    assert_eq!(field(report, "policies"), "belady,no-cache");
    assert_eq!(field(report, "global_budgets_bytes"), "6,4");
    let rows: Vec<&str> = report
        .lines()
        .filter(|line| line.starts_with("policy "))
        .collect();
    assert_eq!(rows.len(), 4);
    assert!(rows[0].starts_with("policy belady budget 6:"), "{report}");
    assert!(rows[1].starts_with("policy belady budget 4:"), "{report}");
    assert!(rows[2].starts_with("policy no-cache budget 6:"), "{report}");
    assert!(rows[3].starts_with("policy no-cache budget 4:"), "{report}");
}

#[test]
fn trace_generate_rejects_identical_output_paths() {
    // One file silently overwriting the other would report success while
    // destroying the trace it just advertised.
    let path = generated_path("aliased.json");
    let output = moe_sim(&[
        "trace",
        "generate",
        "--pattern",
        "cyclic",
        "--experts",
        "3",
        "--events",
        "6",
        "--out-trace",
        &path,
        "--out-model-manifest",
        &path,
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "error: --out-trace and --out-model-manifest resolve to the same file; \
         name two different destinations\n"
    );
    let workspace = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    assert!(
        !workspace.join(&path).exists(),
        "nothing may be written on a rejected collision"
    );
}

#[cfg(unix)]
#[test]
fn trace_generate_rejects_outputs_aliased_through_a_symlinked_directory() {
    // Two different spellings, one physical file: the second directory is a
    // symlink to the first. Writing would let the manifest overwrite the
    // trace while the report advertises two digests for one file.
    let workspace = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let real_dir = workspace.join("target/test-generate/real-dir");
    std::fs::create_dir_all(&real_dir).unwrap();
    let link_dir = workspace.join("target/test-generate/link-dir");
    match std::os::unix::fs::symlink(&real_dir, &link_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => panic!("cannot create the symlinked directory: {error}"),
    }

    let output = moe_sim(&[
        "trace",
        "generate",
        "--pattern",
        "cyclic",
        "--experts",
        "3",
        "--events",
        "6",
        "--out-trace",
        "target/test-generate/real-dir/pair.jsonl",
        "--out-model-manifest",
        "target/test-generate/link-dir/pair.jsonl",
    ]);
    assert_eq!(output.status.code(), Some(2), "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "error: --out-trace and --out-model-manifest resolve to the same file; \
         name two different destinations\n"
    );
    assert!(
        !real_dir.join("pair.jsonl").exists(),
        "nothing may be written on a rejected collision"
    );
}

#[cfg(unix)]
#[test]
fn trace_generate_rejects_a_dangling_file_symlink_alias() {
    // `canonicalize` cannot follow a final symlink while its target is still
    // absent. The preflight must nevertheless see where the first write would
    // land, or the manifest can overwrite the trace on the second path.
    let workspace = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let case_dir = workspace.join("target/test-generate/dangling-file-link");
    std::fs::create_dir_all(&case_dir).unwrap();
    let alias = case_dir.join("alias.json");
    let target = case_dir.join("shared.json");
    for path in [&alias, &target] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("cannot reset {}: {error}", path.display()),
        }
    }
    std::os::unix::fs::symlink("shared.json", &alias).unwrap();

    let output = moe_sim(&[
        "trace",
        "generate",
        "--pattern",
        "cyclic",
        "--experts",
        "3",
        "--events",
        "6",
        "--out-trace",
        "target/test-generate/dangling-file-link/alias.json",
        "--out-model-manifest",
        "target/test-generate/dangling-file-link/shared.json",
    ]);
    assert_eq!(output.status.code(), Some(2), "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "error: --out-trace and --out-model-manifest resolve to the same file; \
         name two different destinations\n"
    );
    assert!(
        !target.exists(),
        "nothing may be written through the dangling symlink"
    );
}

#[test]
fn a_failed_second_write_leaves_the_first_file_and_no_report() {
    // The two writes are not atomic, and that is documented behavior: the
    // trace lands, the manifest write fails, exit 3, and the report — the
    // only success signal — is never printed.
    let trace_path = generated_path("half-pair.jsonl");
    let output = moe_sim(&[
        "trace",
        "generate",
        "--pattern",
        "cyclic",
        "--experts",
        "3",
        "--events",
        "6",
        "--out-trace",
        &trace_path,
        "--out-model-manifest",
        "/nonexistent-moe-sim-dir/m.json",
    ]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(stdout(&output), "");
    let on_disk = std::fs::read_to_string(
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).join(&trace_path),
    )
    .unwrap();
    assert_eq!(on_disk.lines().count(), 6);
}

#[test]
fn compare_requires_both_lists_explicitly() {
    // The non-empty rule must hold even if the `required` derive attribute
    // is dropped: omitting either list is a usage error, never an empty
    // zero-row success report.
    for missing in [
        vec!["--policies", "lru"],
        vec!["--global-budgets-bytes", "4"],
    ] {
        let mut args = vec![
            "compare",
            "--trace",
            "fixtures/synthetic/three-experts-cycle.jsonl",
            "--model-manifest",
            "fixtures/models/three-experts-uniform.json",
        ];
        args.extend_from_slice(&missing);
        let output = moe_sim(&args);
        assert_eq!(output.status.code(), Some(2), "{missing:?}");
        assert_eq!(stdout(&output), "", "{missing:?}");
        assert!(
            stderr(&output).contains("required"),
            "{missing:?}: {}",
            stderr(&output)
        );
    }
}
