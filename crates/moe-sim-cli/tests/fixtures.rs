//! Contract tests backed by the committed fixtures under `fixtures/`.
//!
//! Three contracts are pinned here:
//!
//! 1. **Golden encoding** — every valid fixture is canonical encoder output:
//!    parsing and re-encoding it reproduces the file bytes exactly.
//! 2. **Rejection layer** — every invalid fixture fails at its declared layer:
//!    wire errors (`Json`, `BlankLine`) versus domain errors carried as
//!    `source` (`Event`, `Manifest`).
//! 3. **Capacity matrix** — frozen `(trace, manifest, budget)` triples yield
//!    the exact `validate_global_capacity` results, including field-level error
//!    payloads.

#![expect(
    unused_crate_dependencies,
    reason = "integration-test targets receive every package dependency; this test only exercises the adapter API"
)]

use std::path::PathBuf;

use moe_sim_cli::{
    ManifestParseError, TraceParseError, encode_manifest_json, encode_trace_jsonl,
    parse_manifest_json, parse_trace_jsonl,
};
use moe_sim_core::{CapacityError, Event, EventError, ManifestError, ModelManifest};

/// Valid trace fixtures relative to `fixtures/synthetic/`.
const VALID_TRACE_FIXTURES: [&str; 3] = ["active-set-0-1.jsonl", "expert-2.jsonl", "empty.jsonl"];

/// Valid manifest fixtures relative to `fixtures/models/`.
const VALID_MODEL_FIXTURES: [&str; 2] = ["two-experts-4-6.json", "empty.json"];

/// Reads one committed fixture, addressed relative to `fixtures/`.
fn read_fixture(relative_path: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(relative_path);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read fixture {}: {error}", path.display()))
}

/// Parses a valid trace fixture from `fixtures/synthetic/`.
fn trace_fixture(name: &str) -> Vec<Event> {
    let raw = read_fixture(&format!("synthetic/{name}"));
    parse_trace_jsonl(&raw)
        .unwrap_or_else(|error| panic!("valid trace fixture {name} must parse: {error}"))
}

/// Parses a valid manifest fixture from `fixtures/models/`.
fn model_fixture(name: &str) -> ModelManifest {
    let raw = read_fixture(&format!("models/{name}"));
    parse_manifest_json(&raw)
        .unwrap_or_else(|error| panic!("valid model fixture {name} must parse: {error}"))
}

// Golden encoding: valid fixtures are canonical encoder output.

#[test]
fn valid_trace_fixtures_reencode_byte_identically() {
    for name in VALID_TRACE_FIXTURES {
        let raw = read_fixture(&format!("synthetic/{name}"));
        let events = parse_trace_jsonl(&raw)
            .unwrap_or_else(|error| panic!("valid trace fixture {name} must parse: {error}"));
        let encoded = encode_trace_jsonl(&events)
            .unwrap_or_else(|error| panic!("trace fixture {name} must re-encode: {error}"));
        assert_eq!(
            encoded, raw,
            "trace fixture {name} is not canonical encoder output"
        );
    }
}

#[test]
fn valid_model_fixtures_reencode_byte_identically() {
    for name in VALID_MODEL_FIXTURES {
        let raw = read_fixture(&format!("models/{name}"));
        let manifest = parse_manifest_json(&raw)
            .unwrap_or_else(|error| panic!("valid model fixture {name} must parse: {error}"));
        let encoded = encode_manifest_json(&manifest)
            .unwrap_or_else(|error| panic!("model fixture {name} must re-encode: {error}"));
        assert_eq!(
            encoded, raw,
            "model fixture {name} is not canonical encoder output"
        );
    }
}

#[test]
fn empty_trace_fixture_is_zero_bytes_and_yields_no_events() {
    let raw = read_fixture("synthetic/empty.jsonl");
    assert!(raw.is_empty(), "empty.jsonl must be a zero-byte file");
    assert_eq!(trace_fixture("empty.jsonl"), Vec::new());
}

#[test]
fn empty_model_fixture_is_an_empty_manifest() {
    assert!(model_fixture("empty.json").is_empty());
}

// Rejection layer: invalid trace fixtures.

#[test]
fn blank_line_trace_fixture_is_rejected_at_the_wire_layer() {
    let raw = read_fixture("synthetic/invalid/blank-line.jsonl");
    let Err(error) = parse_trace_jsonl(&raw) else {
        panic!("blank-line.jsonl must be rejected");
    };
    assert!(
        matches!(error, TraceParseError::BlankLine { line: 2 }),
        "expected BlankLine at line 2, got: {error:?}"
    );
}

#[test]
fn duplicate_expert_id_trace_fixture_is_rejected_at_the_domain_layer() {
    let raw = read_fixture("synthetic/invalid/duplicate-expert-id.jsonl");
    let Err(error) = parse_trace_jsonl(&raw) else {
        panic!("duplicate-expert-id.jsonl must be rejected");
    };
    assert!(
        matches!(
            error,
            TraceParseError::Event {
                line: 1,
                source: EventError::DuplicateExpert { expert_id: 3 },
            }
        ),
        "expected DuplicateExpert 3 as domain source at line 1, got: {error:?}"
    );
}

#[test]
fn unknown_field_trace_fixture_is_rejected_at_the_wire_layer() {
    let raw = read_fixture("synthetic/invalid/unknown-field.jsonl");
    let Err(error) = parse_trace_jsonl(&raw) else {
        panic!("unknown-field.jsonl must be rejected");
    };
    assert!(
        matches!(error, TraceParseError::Json { line: 1, .. }),
        "expected a wire JSON error at line 1, got: {error:?}"
    );
    assert!(
        error.to_string().contains("unknown field"),
        "error must name the unknown field, got: {error}"
    );
}

// Rejection layer: invalid manifest fixtures.

#[test]
fn duplicate_expert_key_model_fixture_is_rejected_at_the_domain_layer() {
    let raw = read_fixture("models/invalid/duplicate-expert-key.json");
    let Err(error) = parse_manifest_json(&raw) else {
        panic!("duplicate-expert-key.json must be rejected");
    };
    assert!(
        matches!(
            error,
            ManifestParseError::Manifest {
                source: ManifestError::DuplicateKey {
                    layer_id: 0,
                    expert_id: 1,
                },
            }
        ),
        "expected DuplicateKey (0, 1) as domain source, got: {error:?}"
    );
}

#[test]
fn empty_document_model_fixture_is_rejected_at_the_wire_layer() {
    let raw = read_fixture("models/invalid/empty-document.json");
    assert!(
        raw.is_empty(),
        "empty-document.json must be a zero-byte file"
    );
    let Err(error) = parse_manifest_json(&raw) else {
        panic!(
            "empty-document.json must be rejected: an empty manifest document is not a manifest"
        );
    };
    assert!(
        matches!(error, ManifestParseError::Json { .. }),
        "expected a wire JSON error, got: {error:?}"
    );
}

#[test]
fn unknown_field_model_fixture_is_rejected_at_the_wire_layer() {
    let raw = read_fixture("models/invalid/unknown-field.json");
    let Err(error) = parse_manifest_json(&raw) else {
        panic!("unknown-field.json must be rejected");
    };
    assert!(
        matches!(error, ManifestParseError::Json { .. }),
        "expected a wire JSON error, got: {error:?}"
    );
    assert!(
        error.to_string().contains("unknown field"),
        "error must name the unknown field, got: {error}"
    );
}

#[test]
fn zero_size_model_fixture_is_rejected_at_the_domain_layer() {
    let raw = read_fixture("models/invalid/zero-size.json");
    let Err(error) = parse_manifest_json(&raw) else {
        panic!("zero-size.json must be rejected");
    };
    assert!(
        matches!(
            error,
            ManifestParseError::Manifest {
                source: ManifestError::ZeroSize {
                    layer_id: 0,
                    expert_id: 2,
                },
            }
        ),
        "expected ZeroSize (0, 2) as domain source, got: {error:?}"
    );
}

// Capacity matrix: frozen (trace, manifest, budget) triples.

#[test]
fn active_set_0_1_with_two_experts_4_6_fits_budget_10() {
    let events = trace_fixture("active-set-0-1.jsonl");
    let manifest = model_fixture("two-experts-4-6.json");
    assert_eq!(manifest.validate_global_capacity(10, events.iter()), Ok(()));
}

#[test]
fn active_set_0_1_with_two_experts_4_6_exceeds_budget_9() {
    let events = trace_fixture("active-set-0-1.jsonl");
    let manifest = model_fixture("two-experts-4-6.json");
    // Both experts (4 B, 6 B) individually fit budget 9; the atomic active
    // set {0, 1} of the first event does not.
    assert_eq!(
        manifest.validate_global_capacity(9, events.iter()),
        Err(CapacityError::ActiveSetExceedsGlobalCapacity {
            event_index: 0,
            request_id: 1,
            layer_id: 0,
            active_set_bytes: 10,
            global_budget_bytes: 9,
        })
    );
}

#[test]
fn two_experts_4_6_manifest_alone_exceeds_budget_5() {
    let events = trace_fixture("empty.jsonl");
    let manifest = model_fixture("two-experts-4-6.json");
    // The manifest pass rejects expert 1 (6 B) even though no event
    // references it: feasibility is a property of (manifest, budget) alone.
    assert_eq!(
        manifest.validate_global_capacity(5, events.iter()),
        Err(CapacityError::ExpertExceedsGlobalCapacity {
            layer_id: 0,
            expert_id: 1,
            size_bytes: 6,
            global_budget_bytes: 5,
        })
    );
}

#[test]
fn expert_2_trace_with_two_experts_4_6_reports_unknown_expert() {
    let events = trace_fixture("expert-2.jsonl");
    let manifest = model_fixture("two-experts-4-6.json");
    assert_eq!(
        manifest.validate_global_capacity(10, events.iter()),
        Err(CapacityError::ActiveSetBytes {
            event_index: 0,
            request_id: 1,
            layer_id: 0,
            source: ManifestError::UnknownExpert {
                layer_id: 0,
                expert_id: 2,
            },
        })
    );
}

#[test]
fn empty_trace_with_empty_manifest_fits_budget_0() {
    let events = trace_fixture("empty.jsonl");
    let manifest = model_fixture("empty.json");
    assert_eq!(manifest.validate_global_capacity(0, events.iter()), Ok(()));
}
