//! Canonical activation event and phase types with atomic-set validation.

use std::collections::HashSet;

/// Longest active set validated by linear scan before the hash-set fallback.
///
/// The measured crossover on `aarch64` lies between 128 and 256 elements.
/// Routed top-k active sets are far smaller, so the scan is the normal path;
/// the fallback keeps malformed oversized sets from degrading quadratically.
const LINEAR_SCAN_MAX_LEN: usize = 128;

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
    parts: EventParts,
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

        Ok(Self { parts })
    }

    /// Originating request identifier.
    #[must_use]
    pub fn request_id(&self) -> u64 {
        self.parts.request_id
    }

    /// Execution phase.
    #[must_use]
    pub fn phase(&self) -> Phase {
        self.parts.phase
    }

    /// Step within the request.
    #[must_use]
    pub fn step_id(&self) -> u64 {
        self.parts.step_id
    }

    /// Token position in the sequence.
    #[must_use]
    pub fn token_position(&self) -> u64 {
        self.parts.token_position
    }

    /// Layer index.
    #[must_use]
    pub fn layer_id(&self) -> u32 {
        self.parts.layer_id
    }

    /// Experts forming one atomic active set (guaranteed no duplicates).
    #[must_use]
    pub fn expert_ids(&self) -> &[u32] {
        &self.parts.expert_ids
    }

    fn validate_expert_ids(expert_ids: &[u32]) -> Result<(), EventError> {
        if expert_ids.len() <= LINEAR_SCAN_MAX_LEN {
            for (index, &id) in expert_ids.iter().enumerate() {
                if expert_ids[..index].contains(&id) {
                    return Err(EventError::DuplicateExpert { expert_id: id });
                }
            }
            return Ok(());
        }

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

    fn parts_with_experts(expert_ids: Vec<u32>) -> EventParts {
        EventParts {
            request_id: 1,
            phase: Phase::Decode,
            step_id: 0,
            token_position: 0,
            layer_id: 0,
            expert_ids,
        }
    }

    fn unique_experts(len: usize) -> Vec<u32> {
        (0..len).map(|id| u32::try_from(id).unwrap()).collect()
    }

    #[test]
    fn event_accepts_unique_experts_above_scan_threshold() {
        let expert_ids = unique_experts(LINEAR_SCAN_MAX_LEN + 1);
        let event = Event::new(parts_with_experts(expert_ids.clone())).unwrap();

        assert_eq!(event.expert_ids(), expert_ids.as_slice());
    }

    #[test]
    fn event_rejects_duplicate_experts_above_scan_threshold() {
        let mut expert_ids = unique_experts(LINEAR_SCAN_MAX_LEN + 1);
        expert_ids.push(0);

        let err = Event::new(parts_with_experts(expert_ids)).unwrap_err();

        assert!(matches!(err, EventError::DuplicateExpert { expert_id: 0 }));
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
