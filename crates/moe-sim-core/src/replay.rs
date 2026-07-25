//! Deterministic replay of canonical events and byte-accurate accounting.
//!
//! Events are replayed strictly in the order the caller supplies them, which
//! is file order. Metadata such as `step_id` or `token_position` never
//! reorders anything; it validates source structure only.
//!
//! Each event's unique `expert_ids` form one atomic active set: every member
//! is made resident together, stays pinned for the whole event, and is
//! released before the next event begins. No event is ever partially admitted.
//!
//! Replay does not enforce a capacity budget. Feasibility is a property of the
//! `(manifest, budget, trace)` triple and belongs to
//! [`ModelManifest::validate_global_capacity`], which callers run first.
//! Replay reports the residency it observed; it does not police it.

use std::fmt;

use crate::manifest::{ManifestError, ModelManifest};
use crate::trace::Event;

/// Names the cumulative counter that overflowed during replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReplayCounter {
    /// Count of replayed events.
    Events,
    /// Count of expert objects loaded.
    ObjectLoads,
    /// Cumulative bytes loaded.
    ByteLoads,
}

impl fmt::Display for ReplayCounter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Events => "events",
            Self::ObjectLoads => "object_loads",
            Self::ByteLoads => "byte_loads",
        };
        f.write_str(name)
    }
}

/// Errors returned by replay.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplayError {
    /// Calculating an event's active-set byte total failed.
    ///
    /// `source` is only ever [`ManifestError::UnknownExpert`] or
    /// [`ManifestError::ActiveSetBytesOverflow`]. `layer_id` deliberately
    /// duplicates the layer carried by the source so every event-level error
    /// exposes uniform diagnostics.
    #[error(
        "failed to calculate active-set bytes for event {event_index} (request {request_id}, layer {layer_id}): {source}"
    )]
    ActiveSetBytes {
        /// Zero-based position of the failing event in the supplied order.
        event_index: usize,
        /// Request of the failing event.
        request_id: u64,
        /// Layer of the failing event.
        layer_id: u32,
        /// Underlying lookup or overflow error.
        source: ManifestError,
    },
    /// A cumulative counter exceeded `u64`.
    ///
    /// Reported instead of wrapping silently: a wrapped metric is a wrong
    /// result, and wrong results must not reach a report.
    #[error("replay counter {counter} overflowed u64 at event {event_index}")]
    CounterOverflow {
        /// The counter that overflowed.
        counter: ReplayCounter,
        /// Zero-based position of the event that overflowed it.
        event_index: usize,
    },
}

/// Byte and object accounting produced by one replay.
///
/// Object and byte metrics are reported separately on purpose: an object-hit
/// rate can hide expensive misses on larger experts, so neither number is
/// derivable from the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReplayMetrics {
    events: u64,
    object_loads: u64,
    byte_loads: u64,
    object_hits: u64,
    byte_hits: u64,
    evictions: u64,
    peak_resident_bytes: u64,
}

impl ReplayMetrics {
    /// Events replayed, in supplied (file) order.
    #[must_use]
    pub fn events(&self) -> u64 {
        self.events
    }

    /// Expert objects brought into residency.
    #[must_use]
    pub fn object_loads(&self) -> u64 {
        self.object_loads
    }

    /// Bytes brought into residency.
    #[must_use]
    pub fn byte_loads(&self) -> u64 {
        self.byte_loads
    }

    /// Expert objects satisfied by an already-resident copy.
    #[must_use]
    pub fn object_hits(&self) -> u64 {
        self.object_hits
    }

    /// Bytes satisfied by an already-resident copy.
    #[must_use]
    pub fn byte_hits(&self) -> u64 {
        self.byte_hits
    }

    /// Resident objects removed to reclaim capacity.
    ///
    /// Releasing a pinned active set after its event completes is **not** an
    /// eviction: eviction is the capacity-driven removal of an object the
    /// policy chose to retain. A policy that retains nothing therefore evicts
    /// nothing, which keeps the no-cache baseline comparable with the caching
    /// policies that will be measured against it.
    #[must_use]
    pub fn evictions(&self) -> u64 {
        self.evictions
    }

    /// Largest number of bytes resident at any instant during replay.
    ///
    /// Residency is sampled while an active set is pinned, which is the only
    /// moment anything is resident under a no-retention policy.
    #[must_use]
    pub fn peak_resident_bytes(&self) -> u64 {
        self.peak_resident_bytes
    }
}

/// Replays `events` against `manifest` with no retention between events.
///
/// This is the baseline every caching policy is measured against. Each event
/// loads its entire atomic active set, uses it, and releases it, so every
/// activation is a load and nothing is ever reused:
/// [`ReplayMetrics::object_hits`], [`ReplayMetrics::byte_hits`], and
/// [`ReplayMetrics::evictions`] are always zero.
///
/// `events` is iterated once and is never collected internally. The item bound
/// is `&Event`, so every event must outlive the call and the caller therefore
/// supplies the whole trace: this signature does not on its own replay a trace
/// larger than memory.
///
/// # Errors
///
/// Returns [`ReplayError::ActiveSetBytes`] when an event references an expert
/// the manifest does not declare, or when one active set's byte total
/// overflows.
/// Returns [`ReplayError::CounterOverflow`] when a cumulative counter exceeds
/// `u64`.
pub fn replay_no_cache<'a>(
    manifest: &ModelManifest,
    events: impl IntoIterator<Item = &'a Event>,
) -> Result<ReplayMetrics, ReplayError> {
    let mut metrics = ReplayMetrics::default();

    for (event_index, event) in events.into_iter().enumerate() {
        let active_set_bytes =
            manifest
                .active_set_bytes(event)
                .map_err(|source| ReplayError::ActiveSetBytes {
                    event_index,
                    request_id: event.request_id(),
                    layer_id: event.layer_id(),
                    source,
                })?;

        metrics.events = bump(metrics.events, 1, ReplayCounter::Events, event_index)?;

        // One load per member of the atomic set; counting by iteration keeps
        // the object total exact without a `usize` cast.
        for _ in event.expert_ids() {
            metrics.object_loads = bump(
                metrics.object_loads,
                1,
                ReplayCounter::ObjectLoads,
                event_index,
            )?;
        }

        metrics.byte_loads = bump(
            metrics.byte_loads,
            active_set_bytes,
            ReplayCounter::ByteLoads,
            event_index,
        )?;

        metrics.peak_resident_bytes = metrics.peak_resident_bytes.max(active_set_bytes);
    }

    Ok(metrics)
}

/// Adds `delta` to `counter_value`, reporting overflow instead of wrapping.
fn bump(
    counter_value: u64,
    delta: u64,
    counter: ReplayCounter,
    event_index: usize,
) -> Result<u64, ReplayError> {
    counter_value
        .checked_add(delta)
        .ok_or(ReplayError::CounterOverflow {
            counter,
            event_index,
        })
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "tests build valid manifests and events directly; direct unwraps keep failure diagnostics next to the hand-calculated fixture data"
)]
mod tests {
    use super::*;
    use crate::manifest::{ExpertKey, ExpertSizeEntry};
    use crate::trace::{EventParts, Phase};

    fn entry(layer_id: u32, expert_id: u32, size_bytes: u64) -> ExpertSizeEntry {
        ExpertSizeEntry {
            key: ExpertKey::new(layer_id, expert_id),
            size_bytes,
        }
    }

    fn event(layer_id: u32, expert_ids: Vec<u32>) -> Event {
        Event::new(EventParts {
            request_id: 1,
            phase: Phase::Decode,
            step_id: 0,
            token_position: 0,
            layer_id,
            expert_ids,
        })
        .unwrap()
    }

    /// The committed `two-experts-4-6` manifest: layer 0 expert 0 is 4 bytes,
    /// layer 0 expert 1 is 6 bytes.
    fn two_experts_4_6() -> ModelManifest {
        ModelManifest::try_from_entries(vec![entry(0, 0, 4), entry(0, 1, 6)]).unwrap()
    }

    #[test]
    fn no_cache_matches_the_hand_calculated_active_set_fixture() {
        // Mirrors fixtures/synthetic/active-set-0-1.jsonl against
        // fixtures/models/two-experts-4-6.json.
        //
        // event 0: {0, 1} -> 4 + 6 = 10 bytes, 2 objects
        // event 1: {1}    ->     6 = 6 bytes,  1 object
        // totals: 2 events, 3 objects, 16 bytes, peak residency 10
        let manifest = two_experts_4_6();
        let events = vec![event(0, vec![0, 1]), event(0, vec![1])];

        let metrics = replay_no_cache(&manifest, &events).unwrap();

        assert_eq!(metrics.events(), 2);
        assert_eq!(metrics.object_loads(), 3);
        assert_eq!(metrics.byte_loads(), 16);
        assert_eq!(metrics.peak_resident_bytes(), 10);
    }

    #[test]
    fn no_cache_never_hits_and_never_evicts() {
        let manifest = two_experts_4_6();
        // The same active set repeated: a caching policy would hit here.
        let events = vec![event(0, vec![0, 1]), event(0, vec![0, 1])];

        let metrics = replay_no_cache(&manifest, &events).unwrap();

        assert_eq!(metrics.object_hits(), 0);
        assert_eq!(metrics.byte_hits(), 0);
        assert_eq!(metrics.evictions(), 0);
        assert_eq!(metrics.byte_loads(), 20, "every activation reloads");
    }

    #[test]
    fn peak_residency_is_the_largest_active_set_not_the_last() {
        let manifest = two_experts_4_6();
        let events = vec![event(0, vec![0, 1]), event(0, vec![0])];

        let metrics = replay_no_cache(&manifest, &events).unwrap();

        assert_eq!(metrics.peak_resident_bytes(), 10);
    }

    #[test]
    fn empty_trace_yields_zeroed_metrics() {
        let manifest = two_experts_4_6();

        let metrics = replay_no_cache(&manifest, &[]).unwrap();

        assert_eq!(metrics, ReplayMetrics::default());
        assert_eq!(metrics.events(), 0);
        assert_eq!(metrics.peak_resident_bytes(), 0);
    }

    #[test]
    fn empty_active_set_is_an_event_that_loads_nothing() {
        let manifest = two_experts_4_6();
        let events = vec![event(0, vec![])];

        let metrics = replay_no_cache(&manifest, &events).unwrap();

        assert_eq!(metrics.events(), 1);
        assert_eq!(metrics.object_loads(), 0);
        assert_eq!(metrics.byte_loads(), 0);
        assert_eq!(metrics.peak_resident_bytes(), 0);
    }

    #[test]
    fn events_replay_in_supplied_order_regardless_of_metadata() {
        let manifest = two_experts_4_6();
        // step_id descends while file order ascends; replay must not reorder.
        let descending = |step_id: u64, expert_ids: Vec<u32>| {
            Event::new(EventParts {
                request_id: 1,
                phase: Phase::Decode,
                step_id,
                token_position: step_id,
                layer_id: 0,
                expert_ids,
            })
            .unwrap()
        };
        let events = vec![descending(9, vec![0, 1]), descending(0, vec![1])];

        let metrics = replay_no_cache(&manifest, &events).unwrap();

        assert_eq!(metrics.byte_loads(), 16);
        assert_eq!(metrics.peak_resident_bytes(), 10);
    }

    #[test]
    fn unknown_expert_reports_the_failing_event_position() {
        let manifest = two_experts_4_6();
        let events = vec![event(0, vec![0]), event(0, vec![7])];

        let err = replay_no_cache(&manifest, &events).unwrap_err();

        assert!(
            matches!(
                err,
                ReplayError::ActiveSetBytes {
                    event_index: 1,
                    request_id: 1,
                    layer_id: 0,
                    source: ManifestError::UnknownExpert { .. }
                }
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn byte_load_overflow_is_reported_not_wrapped() {
        let manifest =
            ModelManifest::try_from_entries(vec![entry(0, 0, u64::MAX), entry(0, 1, 1)]).unwrap();
        let events = vec![event(0, vec![0]), event(0, vec![1])];

        let err = replay_no_cache(&manifest, &events).unwrap_err();

        assert!(
            matches!(
                err,
                ReplayError::CounterOverflow {
                    counter: ReplayCounter::ByteLoads,
                    event_index: 1
                }
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn replay_accepts_a_streaming_iterator() {
        let manifest = two_experts_4_6();
        let events = [event(0, vec![0, 1]), event(0, vec![1])];

        let streamed = replay_no_cache(&manifest, events.iter()).unwrap();

        assert_eq!(streamed.byte_loads(), 16);
    }
}
