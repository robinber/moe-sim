//! File-format adapters and commands for the `moe-sim` binary.
//!
//! This crate owns the wire formats — strict v1 JSONL activation traces and
//! strict v1 JSON model manifests — plus the command-line surface built on
//! them. Adapters decode wire data, then delegate all domain validation to
//! the fallible constructors in `moe-sim-core`; capacity feasibility stays a
//! separate, later check and its errors are never folded into parse errors.
//! Command orchestration and report rendering live in [`commands`] so the
//! binary entrypoint stays thin glue, and every success report carries the
//! [`provenance`] facts needed to identify the build and the exact inputs.

pub mod cli;
pub mod commands;
pub mod manifest_json;
pub mod provenance;
pub mod trace_jsonl;

pub use manifest_json::{
    ManifestEncodeError, ManifestParseError, encode_manifest_json, parse_manifest_json,
};
pub use trace_jsonl::{TraceEncodeError, TraceParseError, encode_trace_jsonl, parse_trace_jsonl};
