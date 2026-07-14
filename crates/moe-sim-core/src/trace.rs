//! Canonical activation event and phase types with atomic-set validation.

use std::collections::HashSet;

/// The execution phase of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    /// Prefill (prompt processing) phase.
    Prefill,
    /// Decode (token generation) phase.
    Decode,
    /// Phase is not known or not distinguished in the source trace.
    Unknown,
}

/// A single canonical activation event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Originating request identifier.
    pub request_id: u64,
    /// Execution phase.
    pub phase: Phase,
    /// Step within the request.
    pub step_id: u64,
    /// Token position in the sequence.
    pub token_position: u64,
    /// Layer index.
    pub layer_id: u32,
    /// Experts forming one atomic active set (guaranteed no duplicates).
    pub expert_ids: Vec<u32>,
}

/// Errors returned by [`Event`] construction.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EventError {
    /// Duplicate expert identifier in one event.
    #[error("duplicate expert id {expert_id} in one activation event")]
    DuplicateExpert {
        /// The duplicated expert id.
        expert_id: u32,
    },
}

impl Event {
    /// Creates an [`Event`] after validating the atomic expert set.
    ///
    /// # Errors
    ///
    /// Returns [`EventError::DuplicateExpert`] if duplicates are present.
    pub fn new(
        request_id: u64,
        phase: Phase,
        step_id: u64,
        token_position: u64,
        layer_id: u32,
        expert_ids: Vec<u32>,
    ) -> Result<Self, EventError> {
        Self::validate_expert_ids(&expert_ids)?;

        Ok(Self {
            request_id,
            phase,
            step_id,
            token_position,
            layer_id,
            expert_ids,
        })
    }

    fn validate_expert_ids(expert_ids: &[u32]) -> Result<(), EventError> {
        let mut seen = HashSet::with_capacity(expert_ids.len());
        for &id in expert_ids {
            if !seen.insert(id) {
                return Err(EventError::DuplicateExpert { expert_id: id });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "tests exercise the public fallible constructor directly to assert both success and error paths; diagnostics are improved by the direct calls"
)]
mod tests {
    use super::*;

    #[test]
    fn event_rejects_duplicate_experts() {
        let err = Event::new(1, Phase::Decode, 0, 0, 0, vec![3, 7, 3]).unwrap_err();

        assert!(matches!(err, EventError::DuplicateExpert { expert_id: 3 }));
    }

    #[test]
    fn event_accepts_unique_experts() {
        let event = Event::new(42, Phase::Prefill, 1, 5, 2, vec![0, 11, 4]).unwrap();

        assert_eq!(event.request_id, 42);
        assert_eq!(event.phase, Phase::Prefill);
        assert_eq!(event.expert_ids, vec![0, 11, 4]);
    }

    #[test]
    fn unknown_phase_is_explicit() {
        let event = Event::new(1, Phase::Unknown, 0, 0, 0, vec![5]).unwrap();
        assert_eq!(event.phase, Phase::Unknown);
    }

    #[test]
    fn events_are_equal_by_content() {
        let a = Event::new(1, Phase::Decode, 2, 3, 1, vec![7, 8]).unwrap();
        let b = Event::new(1, Phase::Decode, 2, 3, 1, vec![7, 8]).unwrap();
        assert_eq!(a, b);
    }
}
