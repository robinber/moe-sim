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

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "smoke tests fail loudly on wire-valid fixtures; the messages state which input was expected to parse"
)]
mod tests {
    use moe_sim_core::CapacityError;

    use crate::manifest_json::parse_manifest_json;
    use crate::trace_jsonl::parse_trace_jsonl;

    const SMOKE_TRACE: &str = concat!(
        "{\"request_id\":1,\"phase\":\"prefill\",\"step_id\":0,\
         \"token_position\":0,\"layer_id\":0,\"expert_ids\":[0,1]}\n",
        "{\"request_id\":1,\"phase\":\"decode\",\"step_id\":1,\
         \"token_position\":1,\"layer_id\":0,\"expert_ids\":[1]}\n",
    );

    const SMOKE_MANIFEST: &str = "{\"experts\":[\
         {\"layer_id\":0,\"expert_id\":0,\"size_bytes\":40},\
         {\"layer_id\":0,\"expert_id\":1,\"size_bytes\":60}]}";

    #[test]
    fn parsed_trace_and_manifest_pass_capacity_validation() {
        let events = parse_trace_jsonl(SMOKE_TRACE).expect("trace must parse");
        let manifest = parse_manifest_json(SMOKE_MANIFEST).expect("manifest must parse");

        // Active set {0, 1} totals 100 bytes: exact fit is valid.
        assert_eq!(
            manifest.validate_global_capacity(100, events.iter()),
            Ok(())
        );
    }

    #[test]
    fn parsed_trace_and_manifest_report_capacity_failure_separately() {
        let events = parse_trace_jsonl(SMOKE_TRACE).expect("trace must parse");
        let manifest = parse_manifest_json(SMOKE_MANIFEST).expect("manifest must parse");

        // Both inputs are wire-valid; infeasibility surfaces as a
        // CapacityError from the dedicated validation pass, not as a parse
        // error.
        assert_eq!(
            manifest.validate_global_capacity(99, events.iter()),
            Err(CapacityError::ActiveSetExceedsGlobalCapacity {
                event_index: 0,
                request_id: 1,
                layer_id: 0,
                active_set_bytes: 100,
                global_budget_bytes: 99,
            })
        );
    }
}
