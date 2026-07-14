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

/// Parts used to construct an [`Event`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventParts {
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
    /// Experts forming one atomic active set.
    pub expert_ids: Vec<u32>,
}

/// A single canonical activation event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    request_id: u64,
    phase: Phase,
    step_id: u64,
    token_position: u64,
    layer_id: u32,
    expert_ids: Vec<u32>,
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
    pub fn new(parts: EventParts) -> Result<Self, EventError> {
        Self::validate_expert_ids(&parts.expert_ids)?;

        Ok(Self {
            request_id: parts.request_id,
            phase: parts.phase,
            step_id: parts.step_id,
            token_position: parts.token_position,
            layer_id: parts.layer_id,
            expert_ids: parts.expert_ids,
        })
    }

    /// Originating request identifier.
    #[must_use]
    pub fn request_id(&self) -> u64 {
        self.request_id
    }

    /// Execution phase.
    #[must_use]
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Step within the request.
    #[must_use]
    pub fn step_id(&self) -> u64 {
        self.step_id
    }

    /// Token position in the sequence.
    #[must_use]
    pub fn token_position(&self) -> u64 {
        self.token_position
    }

    /// Layer index.
    #[must_use]
    pub fn layer_id(&self) -> u32 {
        self.layer_id
    }

    /// Experts forming one atomic active set (guaranteed no duplicates).
    #[must_use]
    pub fn expert_ids(&self) -> &[u32] {
        &self.expert_ids
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
        let err = Event::new(EventParts {
            request_id: 1,
            phase: Phase::Decode,
            step_id: 0,
            token_position: 0,
            layer_id: 0,
            expert_ids: vec![3, 7, 3],
        })
        .unwrap_err();

        assert!(matches!(err, EventError::DuplicateExpert { expert_id: 3 }));
    }

    #[test]
    fn event_accepts_unique_experts() {
        let event = Event::new(EventParts {
            request_id: 42,
            phase: Phase::Prefill,
            step_id: 1,
            token_position: 5,
            layer_id: 2,
            expert_ids: vec![0, 11, 4],
        })
        .unwrap();

        assert_eq!(event.request_id(), 42);
        assert_eq!(event.phase(), Phase::Prefill);
        assert_eq!(event.expert_ids(), &[0, 11, 4]);
    }

    #[test]
    fn unknown_phase_is_explicit() {
        let event = Event::new(EventParts {
            request_id: 1,
            phase: Phase::Unknown,
            step_id: 0,
            token_position: 0,
            layer_id: 0,
            expert_ids: vec![5],
        })
        .unwrap();
        assert_eq!(event.phase(), Phase::Unknown);
    }

    #[test]
    fn events_are_equal_by_content() {
        let parts = EventParts {
            request_id: 1,
            phase: Phase::Decode,
            step_id: 2,
            token_position: 3,
            layer_id: 1,
            expert_ids: vec![7, 8],
        };
        let a = Event::new(parts.clone()).unwrap();
        let b = Event::new(parts).unwrap();
        assert_eq!(a, b);
    }
}
