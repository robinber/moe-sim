//! Explicit expert sizes for capacity accounting.
//!
//! Policies and simulators never invent expert sizes. Every `(layer, expert)`
//! pair used by a run must appear in a [`ModelManifest`] with a positive size
//! in bytes.

use std::collections::BTreeMap;

use crate::trace::Event;

/// Identifies one expert instance in the model: layer and expert index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExpertKey {
    layer_id: u32,
    expert_id: u32,
}

impl ExpertKey {
    /// Builds a key for `expert_id` on `layer_id`.
    #[must_use]
    pub const fn new(layer_id: u32, expert_id: u32) -> Self {
        Self {
            layer_id,
            expert_id,
        }
    }

    /// Layer index.
    #[must_use]
    pub const fn layer_id(self) -> u32 {
        self.layer_id
    }

    /// Expert index within the layer.
    #[must_use]
    pub const fn expert_id(self) -> u32 {
        self.expert_id
    }
}

/// One stored size entry used to build a [`ModelManifest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpertSizeEntry {
    /// Expert identity.
    pub key: ExpertKey,
    /// Stored size of the expert in bytes. Must be strictly positive.
    pub size_bytes: u64,
}

/// Map of expert sizes used for byte-accurate capacity accounting.
///
/// Construction is fallible: zero sizes and duplicate keys are rejected so
/// invalid model data cannot enter the simulator.
///
/// Sizes are stored in a [`BTreeMap`] so iteration and `Debug` order are
/// deterministic (layer, then expert), which keeps future provenance dumps and
/// fixtures stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelManifest {
    sizes: BTreeMap<ExpertKey, u64>,
}

/// Errors returned when validating a manifest and events against a capacity
/// budget.
///
/// Capacity feasibility is a property of the `(manifest, budget, trace)`
/// triple. It is distinct from intrinsic manifest validity, which stays owned
/// by [`ManifestError`]; underlying lookup or overflow failures are preserved
/// via a `source` field instead of being flattened.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CapacityError {
    /// A manifest expert is, on its own, larger than the global budget.
    #[error(
        "expert exceeds global capacity: layer {layer_id} expert {expert_id} has size {size_bytes} bytes, global budget is {global_budget_bytes} bytes"
    )]
    ExpertExceedsGlobalCapacity {
        /// Layer of the oversize expert.
        layer_id: u32,
        /// Expert whose stored size exceeds the budget.
        expert_id: u32,
        /// Stored size of the oversize expert in bytes.
        size_bytes: u64,
        /// Global capacity budget in bytes.
        global_budget_bytes: u64,
    },
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
    /// An event's atomic active set is larger than the global budget.
    ///
    /// Because the manifest pass runs first, every member expert individually
    /// fits the budget; the combined atomic set does not.
    #[error(
        "active set exceeds global capacity: event {event_index} request {request_id} layer {layer_id} totals {active_set_bytes} bytes, global budget is {global_budget_bytes} bytes"
    )]
    ActiveSetExceedsGlobalCapacity {
        /// Zero-based position of the failing event in the supplied order.
        event_index: usize,
        /// Request of the failing event.
        request_id: u64,
        /// Layer of the failing event.
        layer_id: u32,
        /// Total stored size of the atomic active set in bytes.
        active_set_bytes: u64,
        /// Global capacity budget in bytes.
        global_budget_bytes: u64,
    },
}

/// Errors returned when building a [`ModelManifest`] or looking up sizes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// An expert was declared with a zero byte size.
    #[error("expert size must be positive: layer {layer_id} expert {expert_id} has size 0")]
    ZeroSize {
        /// Layer of the invalid entry.
        layer_id: u32,
        /// Expert of the invalid entry.
        expert_id: u32,
    },
    /// The same expert key appeared more than once while building the manifest.
    #[error("duplicate expert size entry for layer {layer_id} expert {expert_id}")]
    DuplicateKey {
        /// Layer of the duplicated key.
        layer_id: u32,
        /// Expert of the duplicated key.
        expert_id: u32,
    },
    /// A requested expert key is not present in the manifest.
    #[error("unknown expert in model manifest: layer {layer_id} expert {expert_id}")]
    UnknownExpert {
        /// Layer of the missing expert.
        layer_id: u32,
        /// Expert missing from the manifest.
        expert_id: u32,
    },
    /// Summing the active-set sizes would overflow `u64`.
    #[error("active-set byte total overflowed u64 for layer {layer_id}")]
    ActiveSetBytesOverflow {
        /// Layer of the event whose active set overflowed.
        layer_id: u32,
    },
}

impl ModelManifest {
    /// Builds a manifest from explicit size entries.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::ZeroSize`] when any entry has `size_bytes ==
    /// 0`. Returns [`ManifestError::DuplicateKey`] when the same
    /// [`ExpertKey`] appears more than once.
    pub fn try_from_entries(
        entries: impl IntoIterator<Item = ExpertSizeEntry>,
    ) -> Result<Self, ManifestError> {
        let mut sizes = BTreeMap::new();
        for entry in entries {
            if entry.size_bytes == 0 {
                return Err(ManifestError::ZeroSize {
                    layer_id: entry.key.layer_id(),
                    expert_id: entry.key.expert_id(),
                });
            }
            if sizes.insert(entry.key, entry.size_bytes).is_some() {
                return Err(ManifestError::DuplicateKey {
                    layer_id: entry.key.layer_id(),
                    expert_id: entry.key.expert_id(),
                });
            }
        }
        Ok(Self { sizes })
    }

    /// Number of expert size entries in the manifest.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sizes.len()
    }

    /// Returns `true` when the manifest contains no experts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sizes.is_empty()
    }

    /// Returns `true` when `key` has an entry in the manifest.
    #[must_use]
    pub fn contains(&self, key: ExpertKey) -> bool {
        self.sizes.contains_key(&key)
    }

    /// Stored size in bytes for `key`.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::UnknownExpert`] when `key` is absent.
    pub fn size_bytes(&self, key: ExpertKey) -> Result<u64, ManifestError> {
        self.sizes
            .get(&key)
            .copied()
            .ok_or(ManifestError::UnknownExpert {
                layer_id: key.layer_id(),
                expert_id: key.expert_id(),
            })
    }

    /// Total stored size in bytes of the atomic active set of `event`.
    ///
    /// Uses `event.layer_id()` for every expert in the set. An empty active
    /// set contributes `0` bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::UnknownExpert`] when any expert in the set is
    /// missing from the manifest.
    /// Returns [`ManifestError::ActiveSetBytesOverflow`] when the sum overflows
    /// `u64`.
    pub fn active_set_bytes(&self, event: &Event) -> Result<u64, ManifestError> {
        let layer_id = event.layer_id();
        let mut total: u64 = 0;
        for &expert_id in event.expert_ids() {
            let key = ExpertKey::new(layer_id, expert_id);
            let size = self.size_bytes(key)?;
            total = total
                .checked_add(size)
                .ok_or(ManifestError::ActiveSetBytesOverflow { layer_id })?;
        }
        Ok(total)
    }

    /// Validates this manifest and the supplied events against one global
    /// budget.
    ///
    /// This is a pure feasibility check: no residency mutation, no I/O, no
    /// reordering of events. Callers must complete this pass before emitting
    /// simulation results (convention in M0; M1 may enforce structurally).
    ///
    /// ## Validation order
    ///
    /// 1. **Manifest pass** — every entry in the manifest is checked against
    ///    `global_budget_bytes`, including experts never referenced by the
    ///    supplied events. Iteration order is the deterministic `(layer_id,
    ///    expert_id)` key order of the internal map. The first oversize expert
    ///    fails. Unreferenced experts are intentional for M0: feasibility is a
    ///    property of `(manifest, budget)` alone and does not depend on which
    ///    subset a particular trace activates. Real-data / adapter slices may
    ///    revisit this strictness.
    /// 2. **Event pass** — events are visited once in caller-supplied order
    ///    (file order). Metadata fields such as `step_id` are never used to
    ///    reorder. Each event's unique `expert_ids` form one atomic set via
    ///    [`Self::active_set_bytes`].
    ///
    /// Exact fit (`size == budget`) is valid. Greater than the budget is
    /// invalid. A zero budget is valid only when the manifest is empty (the
    /// expert pass runs first); events may then only use empty active sets.
    /// Any positive-size expert exceeds a zero budget.
    ///
    /// # Errors
    ///
    /// Returns [`CapacityError::ExpertExceedsGlobalCapacity`] for the first
    /// oversize manifest entry in key order.
    /// Returns [`CapacityError::ActiveSetBytes`] when an event's active-set
    /// byte calculation fails.
    /// Returns [`CapacityError::ActiveSetExceedsGlobalCapacity`] when an
    /// atomic active set is larger than the budget.
    pub fn validate_global_capacity<'a>(
        &self,
        global_budget_bytes: u64,
        events: impl IntoIterator<Item = &'a Event>,
    ) -> Result<(), CapacityError> {
        for (key, &size_bytes) in &self.sizes {
            if size_bytes > global_budget_bytes {
                return Err(CapacityError::ExpertExceedsGlobalCapacity {
                    layer_id: key.layer_id(),
                    expert_id: key.expert_id(),
                    size_bytes,
                    global_budget_bytes,
                });
            }
        }

        for (event_index, event) in events.into_iter().enumerate() {
            let active_set_bytes =
                self.active_set_bytes(event)
                    .map_err(|source| CapacityError::ActiveSetBytes {
                        event_index,
                        request_id: event.request_id(),
                        layer_id: event.layer_id(),
                        source,
                    })?;
            if active_set_bytes > global_budget_bytes {
                return Err(CapacityError::ActiveSetExceedsGlobalCapacity {
                    event_index,
                    request_id: event.request_id(),
                    layer_id: event.layer_id(),
                    active_set_bytes,
                    global_budget_bytes,
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "tests exercise fallible constructors and lookups directly"
)]
mod tests {
    use super::*;
    use crate::trace::{Event, EventParts, Phase};

    fn entry(layer_id: u32, expert_id: u32, size_bytes: u64) -> ExpertSizeEntry {
        ExpertSizeEntry {
            key: ExpertKey::new(layer_id, expert_id),
            size_bytes,
        }
    }

    fn sample_event(layer_id: u32, expert_ids: Vec<u32>) -> Event {
        event_with_ids(1, 0, 0, layer_id, expert_ids)
    }

    fn event_with_ids(
        request_id: u64,
        step_id: u64,
        token_position: u64,
        layer_id: u32,
        expert_ids: Vec<u32>,
    ) -> Event {
        Event::new(EventParts {
            request_id,
            phase: Phase::Decode,
            step_id,
            token_position,
            layer_id,
            expert_ids,
        })
        .unwrap()
    }

    #[test]
    fn manifest_accepts_unique_positive_sizes() {
        let manifest = ModelManifest::try_from_entries([
            entry(0, 0, 1_024),
            entry(0, 1, 2_048),
            entry(1, 0, 512),
        ])
        .unwrap();

        assert_eq!(manifest.len(), 3);
        assert!(!manifest.is_empty());
        assert!(manifest.contains(ExpertKey::new(0, 1)));
        assert_eq!(manifest.size_bytes(ExpertKey::new(0, 0)).unwrap(), 1_024);
        assert_eq!(manifest.size_bytes(ExpertKey::new(1, 0)).unwrap(), 512);
    }

    #[test]
    fn manifest_rejects_zero_size() {
        let err = ModelManifest::try_from_entries([entry(2, 5, 0)]).unwrap_err();
        assert_eq!(
            err,
            ManifestError::ZeroSize {
                layer_id: 2,
                expert_id: 5,
            }
        );
    }

    #[test]
    fn manifest_rejects_duplicate_keys() {
        let err =
            ModelManifest::try_from_entries([entry(0, 1, 100), entry(0, 1, 200)]).unwrap_err();
        assert_eq!(
            err,
            ManifestError::DuplicateKey {
                layer_id: 0,
                expert_id: 1,
            }
        );
    }

    #[test]
    fn size_bytes_rejects_unknown_expert() {
        let manifest = ModelManifest::try_from_entries([entry(0, 0, 64)]).unwrap();
        let err = manifest.size_bytes(ExpertKey::new(0, 9)).unwrap_err();
        assert_eq!(
            err,
            ManifestError::UnknownExpert {
                layer_id: 0,
                expert_id: 9,
            }
        );
    }

    #[test]
    fn active_set_bytes_matches_hand_calculated_fixture() {
        // Hand fixture:
        // layer 1: expert 0 = 100 B, expert 2 = 250 B, expert 7 = 50 B
        // event active set {0, 2, 7} => 100 + 250 + 50 = 400 B
        let manifest = ModelManifest::try_from_entries([
            entry(1, 0, 100),
            entry(1, 2, 250),
            entry(1, 7, 50),
            entry(0, 0, 9_999), // other layer must not contribute
        ])
        .unwrap();
        let event = sample_event(1, vec![0, 2, 7]);

        assert_eq!(manifest.active_set_bytes(&event).unwrap(), 400);
    }

    #[test]
    fn active_set_bytes_empty_set_is_zero() {
        let manifest = ModelManifest::try_from_entries([entry(0, 0, 64)]).unwrap();
        let event = sample_event(0, vec![]);
        assert_eq!(manifest.active_set_bytes(&event).unwrap(), 0);
    }

    #[test]
    fn active_set_bytes_rejects_unknown_expert_in_event() {
        let manifest = ModelManifest::try_from_entries([entry(0, 0, 64)]).unwrap();
        let event = sample_event(0, vec![0, 3]);
        let err = manifest.active_set_bytes(&event).unwrap_err();
        assert_eq!(
            err,
            ManifestError::UnknownExpert {
                layer_id: 0,
                expert_id: 3,
            }
        );
    }

    #[test]
    fn active_set_bytes_rejects_expert_id_only_present_on_other_layer() {
        // Same expert index on a different layer must not satisfy the lookup.
        let manifest = ModelManifest::try_from_entries([entry(0, 0, 64)]).unwrap();
        let event = sample_event(1, vec![0]);
        let err = manifest.active_set_bytes(&event).unwrap_err();
        assert_eq!(
            err,
            ManifestError::UnknownExpert {
                layer_id: 1,
                expert_id: 0,
            }
        );
    }

    #[test]
    fn active_set_bytes_detects_overflow() {
        let manifest =
            ModelManifest::try_from_entries([entry(0, 0, u64::MAX), entry(0, 1, 1)]).unwrap();
        let event = sample_event(0, vec![0, 1]);
        let err = manifest.active_set_bytes(&event).unwrap_err();
        assert_eq!(err, ManifestError::ActiveSetBytesOverflow { layer_id: 0 });
    }

    #[test]
    fn global_capacity_accepts_empty_input_at_zero_budget() {
        let manifest = ModelManifest::try_from_entries([]).unwrap();
        let events: [&Event; 0] = [];
        assert_eq!(manifest.validate_global_capacity(0, events), Ok(()));
    }

    #[test]
    fn global_capacity_accepts_empty_active_set_at_zero_budget() {
        let manifest = ModelManifest::try_from_entries([]).unwrap();
        let event = sample_event(0, vec![]);
        assert_eq!(
            manifest.validate_global_capacity(0, std::iter::once(&event)),
            Ok(())
        );
    }

    #[test]
    fn global_capacity_accepts_exact_fit_expert_and_active_set() {
        // global-exact-fit: 40 B + 60 B active set, budget 100 B
        let manifest = ModelManifest::try_from_entries([entry(0, 0, 40), entry(0, 1, 60)]).unwrap();
        let event = sample_event(0, vec![0, 1]);
        assert_eq!(
            manifest.validate_global_capacity(100, std::iter::once(&event)),
            Ok(())
        );
    }

    #[test]
    fn global_capacity_accepts_nonempty_manifest_without_events() {
        let manifest = ModelManifest::try_from_entries([entry(0, 0, 40), entry(0, 1, 60)]).unwrap();
        let events: [&Event; 0] = [];
        assert_eq!(manifest.validate_global_capacity(100, events), Ok(()));
    }

    #[test]
    fn global_capacity_rejects_unreferenced_oversize_expert() {
        // global-oversize-expert: unreferenced 101 B expert, budget 100 B
        let manifest =
            ModelManifest::try_from_entries([entry(0, 0, 50), entry(0, 1, 101)]).unwrap();
        let event = sample_event(0, vec![0]);
        assert_eq!(
            manifest.validate_global_capacity(100, std::iter::once(&event)),
            Err(CapacityError::ExpertExceedsGlobalCapacity {
                layer_id: 0,
                expert_id: 1,
                size_bytes: 101,
                global_budget_bytes: 100,
            })
        );
    }

    #[test]
    fn global_capacity_prioritizes_referenced_oversize_expert() {
        let manifest = ModelManifest::try_from_entries([entry(0, 0, 101)]).unwrap();
        let event = sample_event(0, vec![0]);
        assert_eq!(
            manifest.validate_global_capacity(100, std::iter::once(&event)),
            Err(CapacityError::ExpertExceedsGlobalCapacity {
                layer_id: 0,
                expert_id: 0,
                size_bytes: 101,
                global_budget_bytes: 100,
            })
        );
    }

    #[test]
    fn global_capacity_reports_lowest_oversize_expert_key() {
        // Two oversize entries; BTreeMap order must report the lowest key.
        let manifest =
            ModelManifest::try_from_entries([entry(1, 0, 200), entry(0, 5, 150), entry(0, 1, 120)])
                .unwrap();
        let events: [&Event; 0] = [];
        assert_eq!(
            manifest.validate_global_capacity(100, events),
            Err(CapacityError::ExpertExceedsGlobalCapacity {
                layer_id: 0,
                expert_id: 1,
                size_bytes: 120,
                global_budget_bytes: 100,
            })
        );
    }

    #[test]
    fn global_capacity_runs_manifest_pass_before_independent_event_failure() {
        // Unreferenced oversize expert + earlier event with unknown expert →
        // expert error wins (manifest pass first).
        let manifest =
            ModelManifest::try_from_entries([entry(0, 0, 50), entry(9, 9, 200)]).unwrap();
        let event = sample_event(0, vec![0, 99]);
        assert_eq!(
            manifest.validate_global_capacity(100, std::iter::once(&event)),
            Err(CapacityError::ExpertExceedsGlobalCapacity {
                layer_id: 9,
                expert_id: 9,
                size_bytes: 200,
                global_budget_bytes: 100,
            })
        );
    }

    #[test]
    fn global_capacity_rejects_oversize_active_set() {
        // global-oversize-active-set: 60 + 50 = 110 > 100
        let manifest = ModelManifest::try_from_entries([entry(0, 0, 60), entry(0, 1, 50)]).unwrap();
        let event = event_with_ids(7, 0, 0, 0, vec![0, 1]);
        assert_eq!(
            manifest.validate_global_capacity(100, std::iter::once(&event)),
            Err(CapacityError::ActiveSetExceedsGlobalCapacity {
                event_index: 0,
                request_id: 7,
                layer_id: 0,
                active_set_bytes: 110,
                global_budget_bytes: 100,
            })
        );
    }

    #[test]
    fn global_capacity_reports_unknown_expert_with_event_context() {
        let manifest = ModelManifest::try_from_entries([entry(0, 0, 50)]).unwrap();
        let event = event_with_ids(42, 0, 0, 0, vec![0, 3]);
        assert_eq!(
            manifest.validate_global_capacity(100, std::iter::once(&event)),
            Err(CapacityError::ActiveSetBytes {
                event_index: 0,
                request_id: 42,
                layer_id: 0,
                source: ManifestError::UnknownExpert {
                    layer_id: 0,
                    expert_id: 3,
                },
            })
        );
    }

    #[test]
    fn global_capacity_reports_active_set_overflow_with_event_context() {
        let manifest =
            ModelManifest::try_from_entries([entry(0, 0, u64::MAX), entry(0, 1, 1)]).unwrap();
        let event = event_with_ids(3, 0, 0, 0, vec![0, 1]);
        assert_eq!(
            manifest.validate_global_capacity(u64::MAX, std::iter::once(&event)),
            Err(CapacityError::ActiveSetBytes {
                event_index: 0,
                request_id: 3,
                layer_id: 0,
                source: ManifestError::ActiveSetBytesOverflow { layer_id: 0 },
            })
        );
    }

    #[test]
    fn global_capacity_rejects_oversize_active_set_in_supplied_order() {
        // file-order-first-failure: first event ok, second fails; metadata would
        // sort the failing event first if step_id were used (it must not be).
        let manifest =
            ModelManifest::try_from_entries([entry(0, 0, 40), entry(0, 1, 60), entry(0, 2, 70)])
                .unwrap();
        let ok_event = event_with_ids(1, 99, 99, 0, vec![0]);
        let bad_event = event_with_ids(2, 0, 0, 0, vec![1, 2]); // 60+70=130 > 100
        let events = [&ok_event, &bad_event];
        assert_eq!(
            manifest.validate_global_capacity(100, events),
            Err(CapacityError::ActiveSetExceedsGlobalCapacity {
                event_index: 1,
                request_id: 2,
                layer_id: 0,
                active_set_bytes: 130,
                global_budget_bytes: 100,
            })
        );
    }

    #[test]
    fn global_capacity_uses_event_layer_for_active_set_sizes() {
        // Same expert_id on two layers with different sizes; only event layer
        // busts the budget when both experts are activated together.
        let manifest = ModelManifest::try_from_entries([
            entry(0, 0, 40),
            entry(0, 1, 40),
            entry(1, 0, 60),
            entry(1, 1, 50),
        ])
        .unwrap();
        let event = event_with_ids(1, 0, 0, 1, vec![0, 1]); // 60+50=110 > 100
        assert_eq!(
            manifest.validate_global_capacity(100, std::iter::once(&event)),
            Err(CapacityError::ActiveSetExceedsGlobalCapacity {
                event_index: 0,
                request_id: 1,
                layer_id: 1,
                active_set_bytes: 110,
                global_budget_bytes: 100,
            })
        );
    }
}
