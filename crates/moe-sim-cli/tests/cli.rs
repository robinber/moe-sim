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
         events: 2\n\
         manifest_experts: 2\n",
            env!("CARGO_PKG_VERSION")
        )
    );
    assert_eq!(stderr(&output), "");
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
