//! Strict v1 JSONL codec for canonical activation traces.
//!
//! One compact JSON event object per line, each line terminated by LF. Every
//! field is required, unknown fields are rejected, and blank lines are
//! rejected. An empty input is a valid empty trace. Events keep file order;
//! metadata such as `step_id` never reorders them. The `phase` field must be
//! exactly `"prefill"`, `"decode"`, or `"unknown"` — it is never inferred
//! from `token_position`.

use moe_sim_core::{Event, EventError, EventParts, Phase};
use serde::{Deserialize, Serialize};

/// Wire shape of one activation event line (strict v1).
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EventWire {
    request_id: u64,
    phase: PhaseWire,
    step_id: u64,
    token_position: u64,
    layer_id: u32,
    expert_ids: Vec<u32>,
}

/// Wire spelling of [`Phase`]: exactly `prefill`, `decode`, or `unknown`.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum PhaseWire {
    Prefill,
    Decode,
    Unknown,
}

impl From<PhaseWire> for Phase {
    fn from(wire: PhaseWire) -> Self {
        match wire {
            PhaseWire::Prefill => Self::Prefill,
            PhaseWire::Decode => Self::Decode,
            PhaseWire::Unknown => Self::Unknown,
        }
    }
}

impl From<Phase> for PhaseWire {
    fn from(phase: Phase) -> Self {
        match phase {
            Phase::Prefill => Self::Prefill,
            Phase::Decode => Self::Decode,
            Phase::Unknown => Self::Unknown,
        }
    }
}

impl From<&Event> for EventWire {
    fn from(event: &Event) -> Self {
        Self {
            request_id: event.request_id(),
            phase: event.phase().into(),
            step_id: event.step_id(),
            token_position: event.token_position(),
            layer_id: event.layer_id(),
            expert_ids: event.expert_ids().to_vec(),
        }
    }
}

/// Errors returned when parsing a strict v1 JSONL trace.
///
/// Every variant carries the one-based physical line number of the failing
/// line. Domain violations keep the underlying [`EventError`] as `source`.
#[derive(Debug, thiserror::Error)]
pub enum TraceParseError {
    /// A line was empty or whitespace-only.
    #[error("line {line}: blank lines are not allowed in a JSONL trace")]
    BlankLine {
        /// One-based line number of the blank line.
        line: usize,
    },
    /// A line was not a valid v1 event object.
    #[error("line {line}: invalid event JSON: {source}")]
    Json {
        /// One-based line number of the invalid line.
        line: usize,
        /// Underlying JSON error; its column is relative to the line.
        source: serde_json::Error,
    },
    /// A wire-valid line violated an event invariant.
    #[error("line {line}: invalid activation event: {source}")]
    Event {
        /// One-based line number of the invalid event.
        line: usize,
        /// Underlying domain error from event construction.
        source: EventError,
    },
}

/// Error returned when encoding a trace as strict v1 JSONL.
#[derive(Debug, thiserror::Error)]
#[error("failed to encode activation event as JSON: {source}")]
pub struct TraceEncodeError {
    source: serde_json::Error,
}

/// Parses a strict v1 JSONL trace into canonical events in file order.
///
/// An empty input yields an empty trace. A missing final LF is accepted; the
/// last line is parsed like any other.
///
/// # Errors
///
/// Returns [`TraceParseError::BlankLine`] for an empty or whitespace-only
/// line, [`TraceParseError::Json`] for a line that is not a valid v1 event
/// object (missing fields, unknown fields, wrong types, bad phase spelling,
/// trailing content), and [`TraceParseError::Event`] when a wire-valid line
/// is rejected by [`Event::new`].
pub fn parse_trace_jsonl(input: &str) -> Result<Vec<Event>, TraceParseError> {
    let mut events = Vec::new();
    if input.is_empty() {
        return Ok(events);
    }

    let body = input.strip_suffix('\n').unwrap_or(input);
    for (index, line) in body.split('\n').enumerate() {
        let line_number = index + 1;
        if line.trim().is_empty() {
            return Err(TraceParseError::BlankLine { line: line_number });
        }
        let wire: EventWire =
            serde_json::from_str(line).map_err(|source| TraceParseError::Json {
                line: line_number,
                source,
            })?;
        let event = Event::new(EventParts {
            request_id: wire.request_id,
            phase: wire.phase.into(),
            step_id: wire.step_id,
            token_position: wire.token_position,
            layer_id: wire.layer_id,
            expert_ids: wire.expert_ids,
        })
        .map_err(|source| TraceParseError::Event {
            line: line_number,
            source,
        })?;
        events.push(event);
    }
    Ok(events)
}

/// Encodes events as strict v1 JSONL: one compact object per line, each line
/// terminated by LF, in the given order.
///
/// Equal event slices produce byte-identical output. An empty slice encodes
/// as an empty string.
///
/// # Errors
///
/// Returns [`TraceEncodeError`] when JSON serialization fails; with the v1
/// wire shape this is not expected to occur.
pub fn encode_trace_jsonl(events: &[Event]) -> Result<String, TraceEncodeError> {
    let mut out = String::new();
    for event in events {
        let line = serde_json::to_string(&EventWire::from(event))
            .map_err(|source| TraceEncodeError { source })?;
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "tests exercise the fallible codec directly; direct unwraps keep failure diagnostics close to the fixture line"
)]
mod tests {
    use super::*;

    fn event(
        request_id: u64,
        phase: Phase,
        step_id: u64,
        token_position: u64,
        layer_id: u32,
        expert_ids: Vec<u32>,
    ) -> Event {
        Event::new(EventParts {
            request_id,
            phase,
            step_id,
            token_position,
            layer_id,
            expert_ids,
        })
        .unwrap()
    }

    #[test]
    fn round_trip_covers_every_phase_variant() {
        let events = vec![
            event(1, Phase::Prefill, 0, 0, 0, vec![0, 1]),
            event(1, Phase::Decode, 1, 1, 2, vec![3]),
            event(2, Phase::Unknown, 0, 0, 0, vec![]),
        ];

        let encoded = encode_trace_jsonl(&events).unwrap();
        assert_eq!(
            encoded,
            "{\"request_id\":1,\"phase\":\"prefill\",\"step_id\":0,\"token_position\":0,\
             \"layer_id\":0,\"expert_ids\":[0,1]}\n\
             {\"request_id\":1,\"phase\":\"decode\",\"step_id\":1,\"token_position\":1,\
             \"layer_id\":2,\"expert_ids\":[3]}\n\
             {\"request_id\":2,\"phase\":\"unknown\",\"step_id\":0,\"token_position\":0,\
             \"layer_id\":0,\"expert_ids\":[]}\n"
        );

        assert_eq!(parse_trace_jsonl(&encoded).unwrap(), events);
    }

    #[test]
    fn round_trip_preserves_u64_and_u32_extremes() {
        let events = vec![event(
            u64::MAX,
            Phase::Unknown,
            u64::MAX,
            u64::MAX,
            u32::MAX,
            vec![u32::MAX, 0],
        )];

        let encoded = encode_trace_jsonl(&events).unwrap();
        assert_eq!(parse_trace_jsonl(&encoded).unwrap(), events);
    }

    #[test]
    fn parse_keeps_file_order_despite_decreasing_step_ids() {
        let input = "{\"request_id\":1,\"phase\":\"decode\",\"step_id\":9,\
             \"token_position\":9,\"layer_id\":0,\"expert_ids\":[1]}\n\
             {\"request_id\":1,\"phase\":\"decode\",\"step_id\":0,\
             \"token_position\":0,\"layer_id\":0,\"expert_ids\":[2]}\n";

        let events = parse_trace_jsonl(input).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].step_id(), 9);
        assert_eq!(events[1].step_id(), 0);
    }

    #[test]
    fn empty_input_is_a_valid_empty_trace() {
        assert_eq!(parse_trace_jsonl("").unwrap(), Vec::new());
    }

    #[test]
    fn encode_of_empty_trace_is_empty() {
        assert_eq!(encode_trace_jsonl(&[]).unwrap(), "");
    }

    #[test]
    fn missing_final_newline_is_accepted() {
        let input = "{\"request_id\":1,\"phase\":\"unknown\",\"step_id\":0,\
             \"token_position\":0,\"layer_id\":0,\"expert_ids\":[4]}";
        let events = parse_trace_jsonl(input).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].expert_ids(), &[4]);
    }

    #[test]
    fn blank_interior_line_is_rejected_with_line_number() {
        let input = "{\"request_id\":1,\"phase\":\"unknown\",\"step_id\":0,\
             \"token_position\":0,\"layer_id\":0,\"expert_ids\":[]}\n\
             \n\
             {\"request_id\":2,\"phase\":\"unknown\",\"step_id\":0,\
             \"token_position\":0,\"layer_id\":0,\"expert_ids\":[]}\n";

        let err = parse_trace_jsonl(input).unwrap_err();
        assert!(matches!(err, TraceParseError::BlankLine { line: 2 }));
    }

    #[test]
    fn whitespace_only_line_is_rejected() {
        let err = parse_trace_jsonl("   \n").unwrap_err();
        assert!(matches!(err, TraceParseError::BlankLine { line: 1 }));
    }

    #[test]
    fn lone_newline_is_one_blank_line() {
        let err = parse_trace_jsonl("\n").unwrap_err();
        assert!(matches!(err, TraceParseError::BlankLine { line: 1 }));
    }

    #[test]
    fn unknown_field_is_rejected() {
        let input = "{\"request_id\":1,\"phase\":\"decode\",\"step_id\":0,\
             \"token_position\":0,\"layer_id\":0,\"expert_ids\":[],\"extra\":1}\n";
        let err = parse_trace_jsonl(input).unwrap_err();
        assert!(matches!(err, TraceParseError::Json { line: 1, .. }));
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn missing_request_id_field_is_rejected() {
        let input = "{\"phase\":\"decode\",\"step_id\":0,\"token_position\":0,\
             \"layer_id\":0,\"expert_ids\":[]}\n";
        let err = parse_trace_jsonl(input).unwrap_err();
        assert!(matches!(err, TraceParseError::Json { line: 1, .. }));
        assert!(err.to_string().contains("missing field `request_id`"));
    }

    #[test]
    fn missing_expert_ids_field_is_rejected() {
        let input = "{\"request_id\":1,\"phase\":\"decode\",\"step_id\":0,\
             \"token_position\":0,\"layer_id\":0}\n";
        let err = parse_trace_jsonl(input).unwrap_err();
        assert!(matches!(err, TraceParseError::Json { line: 1, .. }));
        assert!(err.to_string().contains("missing field `expert_ids`"));
    }

    #[test]
    fn missing_phase_field_is_rejected_not_inferred() {
        // token_position is present; phase must still be explicit.
        let input = "{\"request_id\":1,\"step_id\":0,\"token_position\":0,\
             \"layer_id\":0,\"expert_ids\":[]}\n";
        let err = parse_trace_jsonl(input).unwrap_err();
        assert!(matches!(err, TraceParseError::Json { line: 1, .. }));
        assert!(err.to_string().contains("missing field"));
    }

    #[test]
    fn wrongly_cased_phase_is_rejected() {
        let input = "{\"request_id\":1,\"phase\":\"Prefill\",\"step_id\":0,\
             \"token_position\":0,\"layer_id\":0,\"expert_ids\":[]}\n";
        let err = parse_trace_jsonl(input).unwrap_err();
        assert!(matches!(err, TraceParseError::Json { line: 1, .. }));
    }

    #[test]
    fn non_object_line_is_rejected() {
        let err = parse_trace_jsonl("42\n").unwrap_err();
        assert!(matches!(err, TraceParseError::Json { line: 1, .. }));
    }

    #[test]
    fn trailing_content_after_object_is_rejected() {
        let input = "{\"request_id\":1,\"phase\":\"decode\",\"step_id\":0,\
             \"token_position\":0,\"layer_id\":0,\"expert_ids\":[]} trailing\n";
        let err = parse_trace_jsonl(input).unwrap_err();
        assert!(matches!(err, TraceParseError::Json { line: 1, .. }));
    }

    #[test]
    fn negative_expert_id_is_rejected() {
        let input = "{\"request_id\":1,\"phase\":\"decode\",\"step_id\":0,\
             \"token_position\":0,\"layer_id\":0,\"expert_ids\":[-1]}\n";
        let err = parse_trace_jsonl(input).unwrap_err();
        assert!(matches!(err, TraceParseError::Json { line: 1, .. }));
    }

    #[test]
    fn negative_request_id_is_rejected() {
        let input = "{\"request_id\":-1,\"phase\":\"decode\",\"step_id\":0,\
             \"token_position\":0,\"layer_id\":0,\"expert_ids\":[]}\n";
        let err = parse_trace_jsonl(input).unwrap_err();
        assert!(matches!(err, TraceParseError::Json { line: 1, .. }));
    }

    #[test]
    fn fractional_step_id_is_rejected() {
        let input = "{\"request_id\":1,\"phase\":\"decode\",\"step_id\":1.5,\
             \"token_position\":0,\"layer_id\":0,\"expert_ids\":[]}\n";
        let err = parse_trace_jsonl(input).unwrap_err();
        assert!(matches!(err, TraceParseError::Json { line: 1, .. }));
    }

    #[test]
    fn request_id_above_u64_max_is_rejected() {
        // 18446744073709551616 == u64::MAX + 1.
        let input = "{\"request_id\":18446744073709551616,\"phase\":\"decode\",\
             \"step_id\":0,\"token_position\":0,\"layer_id\":0,\"expert_ids\":[]}\n";
        let err = parse_trace_jsonl(input).unwrap_err();
        assert!(matches!(err, TraceParseError::Json { line: 1, .. }));
    }

    #[test]
    fn layer_id_above_u32_max_is_rejected() {
        // 4294967296 == u32::MAX + 1.
        let input = "{\"request_id\":1,\"phase\":\"decode\",\"step_id\":0,\
             \"token_position\":0,\"layer_id\":4294967296,\"expert_ids\":[]}\n";
        let err = parse_trace_jsonl(input).unwrap_err();
        assert!(matches!(err, TraceParseError::Json { line: 1, .. }));
    }

    #[test]
    fn expert_id_above_u32_max_is_rejected() {
        let input = "{\"request_id\":1,\"phase\":\"decode\",\"step_id\":0,\
             \"token_position\":0,\"layer_id\":0,\"expert_ids\":[4294967296]}\n";
        let err = parse_trace_jsonl(input).unwrap_err();
        assert!(matches!(err, TraceParseError::Json { line: 1, .. }));
    }

    #[test]
    fn trailing_blank_line_is_rejected() {
        // The first LF terminates the event line; the second leaves a blank
        // line 2 that must be rejected, not silently ignored.
        let input = "{\"request_id\":1,\"phase\":\"unknown\",\"step_id\":0,\
             \"token_position\":0,\"layer_id\":0,\"expert_ids\":[]}\n\n";
        let err = parse_trace_jsonl(input).unwrap_err();
        assert!(matches!(err, TraceParseError::BlankLine { line: 2 }));
    }

    #[test]
    fn double_blank_line_is_rejected_at_first_blank() {
        let input = "{\"request_id\":1,\"phase\":\"unknown\",\"step_id\":0,\
             \"token_position\":0,\"layer_id\":0,\"expert_ids\":[]}\n\n\n";
        let err = parse_trace_jsonl(input).unwrap_err();
        assert!(matches!(err, TraceParseError::BlankLine { line: 2 }));
    }

    #[test]
    fn malformed_json_on_second_line_reports_line_and_column() {
        let input = "{\"request_id\":1,\"phase\":\"decode\",\"step_id\":0,\
             \"token_position\":0,\"layer_id\":0,\"expert_ids\":[]}\n\
             {\"request_id\":]\n";
        let err = parse_trace_jsonl(input).unwrap_err();
        let TraceParseError::Json { line, source } = err else {
            panic!("expected a JSON parse error, got: {err:?}");
        };
        assert_eq!(line, 2);
        // The serde_json position is relative to the failing line, so its
        // line is 1 and its column points at the stray `]`.
        assert_eq!(source.line(), 1);
        assert_eq!(source.column(), 15);
    }

    #[test]
    fn duplicate_object_member_is_rejected() {
        let input = "{\"request_id\":1,\"request_id\":2,\"phase\":\"decode\",\
             \"step_id\":0,\"token_position\":0,\"layer_id\":0,\"expert_ids\":[]}\n";
        let err = parse_trace_jsonl(input).unwrap_err();
        assert!(matches!(err, TraceParseError::Json { line: 1, .. }));
        assert!(err.to_string().contains("duplicate field `request_id`"));
    }

    #[test]
    fn duplicate_expert_ids_preserve_domain_error_with_line() {
        let input = "{\"request_id\":1,\"phase\":\"decode\",\"step_id\":0,\
             \"token_position\":0,\"layer_id\":0,\"expert_ids\":[0]}\n\
             {\"request_id\":2,\"phase\":\"decode\",\"step_id\":1,\
             \"token_position\":1,\"layer_id\":0,\"expert_ids\":[3,7,3]}\n";

        let err = parse_trace_jsonl(input).unwrap_err();
        assert!(matches!(
            err,
            TraceParseError::Event {
                line: 2,
                source: EventError::DuplicateExpert { expert_id: 3 },
            }
        ));
        // The domain error stays reachable through std::error::Error::source.
        let source = std::error::Error::source(&err);
        assert_eq!(
            source.and_then(|s| s.downcast_ref::<EventError>()),
            Some(&EventError::DuplicateExpert { expert_id: 3 })
        );
    }
}
