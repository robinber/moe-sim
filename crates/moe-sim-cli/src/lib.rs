//! File-format adapters for moe-sim.
//!
//! This crate owns the wire formats: strict v1 JSONL activation traces and
//! strict v1 JSON model manifests. Adapters decode wire data, then delegate
//! all domain validation to the fallible constructors in `moe-sim-core`;
//! capacity feasibility stays a separate, later check and its errors are
//! never folded into parse errors.

pub mod manifest_json;
pub mod trace_jsonl;

pub use manifest_json::{
    ManifestEncodeError, ManifestParseError, encode_manifest_json, parse_manifest_json,
};
pub use trace_jsonl::{TraceEncodeError, TraceParseError, encode_trace_jsonl, parse_trace_jsonl};
