//! Canonical activation event and phase types.
//!
//! These types represent the minimal information required to replay
//! expert activations in file order.
//!
//! ## Invariants
//!
//! - `expert_ids` within a single event form one **atomic active set**. All
//!   experts listed must be resident together.
//! - Duplicate expert identifiers within one event are invalid.
//! - The `phase` field preserves explicit prefill/decode boundaries when known.
//!   `Unknown` must remain explicit; it is never inferred.

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
///
/// This event records the experts that must be active together for one
/// atomic step (typically one token at one layer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Identifier of the originating request.
    pub request_id: u64,
    /// Execution phase.
    pub phase: Phase,
    /// Monotonic step identifier within the request.
    pub step_id: u64,
    /// Position of the token within the sequence.
    pub token_position: u64,
    /// Layer index.
    pub layer_id: u32,
    /// The experts that must be simultaneously resident for this event.
    ///
    /// The list contains no duplicates. The order is preserved from input
    /// (after validation).
    pub expert_ids: Vec<u32>,
}

/// Errors that can occur when constructing or validating an [`Event`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EventError {
    /// The same expert identifier appears more than once in a single event.
    #[error("duplicate expert id {expert_id} within one atomic activation event")]
    DuplicateExpert {
        /// The expert identifier that was duplicated.
        expert_id: u32,
    },
}

impl Event {
    /// Constructs a new event after validating the atomic active set.
    ///
    /// # Errors
    ///
    /// Returns [`EventError::DuplicateExpert`] if `expert_ids` contains
    /// duplicates.
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
