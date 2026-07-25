//! End-to-end tests for the `moe-sim` binary over committed fixtures.
//!
//! Each test spawns the real binary via `CARGO_BIN_EXE_moe-sim` and pins the
//! frozen process contract:
//!
//! - exit codes: 0 ok, 2 bad argv, 3 read/UTF-8/path, 4 parse/domain wire, 5
//!   capacity rejection;
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
        // Belongs to slice 1C and is not implemented: an unsupported policy
        // must be refused, never silently approximated by another one.
        "belady",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("belady"),
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

// Read failures (exit 3).

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
