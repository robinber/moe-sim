#![expect(
    clippy::unwrap_used,
    reason = "tests exercise fallible constructors and lookups directly"
)]

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
    let manifest =
        ModelManifest::try_from_entries([entry(0, 0, 1_024), entry(0, 1, 2_048), entry(1, 0, 512)])
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
    let err = ModelManifest::try_from_entries([entry(0, 1, 100), entry(0, 1, 200)]).unwrap_err();
    assert_eq!(
        err,
        ManifestError::DuplicateKey {
            layer_id: 0,
            expert_id: 1,
        }
    );
}

#[test]
fn entries_iterate_in_sorted_key_order() {
    // Insertion order is deliberately unsorted; iteration must be
    // ascending (layer_id, expert_id) regardless.
    let manifest =
        ModelManifest::try_from_entries([entry(1, 0, 300), entry(0, 7, 100), entry(0, 2, 200)])
            .unwrap();

    let entries: Vec<ExpertSizeEntry> = manifest.entries().collect();
    assert_eq!(
        entries,
        vec![entry(0, 2, 200), entry(0, 7, 100), entry(1, 0, 300),]
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
    let manifest = ModelManifest::try_from_entries([entry(0, 0, 50), entry(0, 1, 101)]).unwrap();
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
    let manifest = ModelManifest::try_from_entries([entry(0, 0, 50), entry(9, 9, 200)]).unwrap();
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

#[test]
fn global_capacity_rejects_positive_manifest_at_zero_budget() {
    // Rule 6 corollary: any positive-size expert exceeds a zero budget
    // during the manifest pass (before events are considered).
    let manifest = ModelManifest::try_from_entries([entry(0, 0, 1)]).unwrap();
    let event = sample_event(0, vec![0]);
    assert_eq!(
        manifest.validate_global_capacity(0, std::iter::once(&event)),
        Err(CapacityError::ExpertExceedsGlobalCapacity {
            layer_id: 0,
            expert_id: 0,
            size_bytes: 1,
            global_budget_bytes: 0,
        })
    );
}

#[test]
fn global_capacity_reports_unknown_expert_at_nonzero_event_index() {
    // ActiveSetBytes must carry the file-order index of the failing event,
    // not only index 0.
    let manifest = ModelManifest::try_from_entries([entry(0, 0, 50)]).unwrap();
    let ok_event = event_with_ids(1, 0, 0, 0, vec![0]);
    let bad_event = event_with_ids(9, 0, 0, 0, vec![0, 7]);
    let events = [&ok_event, &bad_event];
    let err = manifest.validate_global_capacity(100, events).unwrap_err();
    assert_eq!(
        err,
        CapacityError::ActiveSetBytes {
            event_index: 1,
            request_id: 9,
            layer_id: 0,
            source: ManifestError::UnknownExpert {
                layer_id: 0,
                expert_id: 7,
            },
        }
    );
    // Pin the thiserror source chain for callers using std::error::Error.
    let source = std::error::Error::source(&err);
    assert!(source.is_some(), "ActiveSetBytes must expose Error::source");
    assert_eq!(
        source.and_then(|s| s.downcast_ref::<ManifestError>()),
        Some(&ManifestError::UnknownExpert {
            layer_id: 0,
            expert_id: 7,
        })
    );
}

// --- per-layer capacity validation ---

/// Layer 0 holds experts 0 (4 B) and 1 (6 B); layer 1 holds experts
/// 0 (5 B) and 1 (3 B). Mirrors `fixtures/models/two-layers.json`.
fn two_layer_manifest() -> ModelManifest {
    ModelManifest::try_from_entries([
        entry(0, 0, 4),
        entry(0, 1, 6),
        entry(1, 0, 5),
        entry(1, 1, 3),
    ])
    .unwrap()
}

fn quotas(pairs: &[(u32, u64)]) -> BTreeMap<u32, u64> {
    pairs.iter().copied().collect()
}

#[test]
fn per_layer_capacity_accepts_a_feasible_configuration() {
    let manifest = two_layer_manifest();
    let events = [sample_event(0, vec![0, 1]), sample_event(1, vec![0, 1])];
    assert_eq!(
        manifest.validate_per_layer_capacity(18, &quotas(&[(0, 10), (1, 8)]), &events),
        Ok(())
    );
}

#[test]
fn per_layer_capacity_rejects_a_quota_for_an_unknown_layer() {
    let manifest = two_layer_manifest();
    let no_events: [Event; 0] = [];
    let err = manifest
        .validate_per_layer_capacity(100, &quotas(&[(0, 10), (7, 10)]), &no_events)
        .unwrap_err();
    assert_eq!(err, CapacityError::QuotaForUnknownLayer { layer_id: 7 });
}

#[test]
fn per_layer_capacity_rejects_a_quota_sum_overflow() {
    let manifest = two_layer_manifest();
    let no_events: [Event; 0] = [];
    let err = manifest
        .validate_per_layer_capacity(u64::MAX, &quotas(&[(0, u64::MAX), (1, 1)]), &no_events)
        .unwrap_err();
    assert_eq!(err, CapacityError::LayerQuotaSumOverflow);
}

#[test]
fn per_layer_capacity_rejects_a_quota_sum_above_the_total_budget() {
    let manifest = two_layer_manifest();
    let no_events: [Event; 0] = [];
    let err = manifest
        .validate_per_layer_capacity(17, &quotas(&[(0, 10), (1, 8)]), &no_events)
        .unwrap_err();
    assert_eq!(
        err,
        CapacityError::LayerQuotaSumExceedsTotalBudget {
            quota_sum_bytes: 18,
            total_budget_bytes: 17,
        }
    );
}

#[test]
fn per_layer_capacity_rejects_an_expert_larger_than_its_layer_quota() {
    // The check covers every manifest expert on a quota'd layer, even
    // when the trace never activates it.
    let manifest = two_layer_manifest();
    let no_events: [Event; 0] = [];
    let err = manifest
        .validate_per_layer_capacity(10, &quotas(&[(0, 5)]), &no_events)
        .unwrap_err();
    assert_eq!(
        err,
        CapacityError::ExpertExceedsLayerQuota {
            layer_id: 0,
            expert_id: 1,
            size_bytes: 6,
            quota_bytes: 5,
        }
    );
}

#[test]
fn per_layer_capacity_rejects_an_activated_layer_without_a_quota() {
    let manifest = two_layer_manifest();
    let events = [sample_event(0, vec![0]), sample_event(1, vec![0])];
    let err = manifest
        .validate_per_layer_capacity(100, &quotas(&[(0, 10)]), &events)
        .unwrap_err();
    assert_eq!(
        err,
        CapacityError::MissingLayerQuota {
            event_index: 1,
            request_id: 1,
            layer_id: 1,
        }
    );
}

#[test]
fn per_layer_capacity_rejects_an_active_set_larger_than_its_layer_quota() {
    // Each member fits the quota alone; the atomic set does not, and the
    // unused quota of layer 1 must not absorb the excess.
    let manifest = two_layer_manifest();
    let events = [sample_event(0, vec![0, 1])];
    let err = manifest
        .validate_per_layer_capacity(100, &quotas(&[(0, 8), (1, 8)]), &events)
        .unwrap_err();
    assert_eq!(
        err,
        CapacityError::ActiveSetExceedsLayerQuota {
            event_index: 0,
            request_id: 1,
            layer_id: 0,
            active_set_bytes: 10,
            quota_bytes: 8,
        }
    );
}

#[test]
fn per_layer_capacity_reports_active_set_lookup_failures() {
    let manifest = two_layer_manifest();
    let events = [sample_event(0, vec![0, 9])];
    let err = manifest
        .validate_per_layer_capacity(100, &quotas(&[(0, 10)]), &events)
        .unwrap_err();
    assert_eq!(
        err,
        CapacityError::ActiveSetBytes {
            event_index: 0,
            request_id: 1,
            layer_id: 0,
            source: ManifestError::UnknownExpert {
                layer_id: 0,
                expert_id: 9,
            },
        }
    );
}

#[test]
fn per_layer_capacity_ignores_manifest_layers_that_are_not_simulated() {
    // Layer 1 has neither a quota nor an activation: it is not simulated,
    // so its experts are not measured against anything.
    let manifest = two_layer_manifest();
    let events = [sample_event(0, vec![0, 1])];
    assert_eq!(
        manifest.validate_per_layer_capacity(10, &quotas(&[(0, 10)]), &events),
        Ok(())
    );
}

#[test]
fn per_layer_capacity_accepts_a_quota_for_an_unactivated_manifest_layer() {
    let manifest = two_layer_manifest();
    let events = [sample_event(0, vec![0])];
    assert_eq!(
        manifest.validate_per_layer_capacity(18, &quotas(&[(0, 10), (1, 8)]), &events),
        Ok(())
    );
}

#[test]
fn per_layer_capacity_accepts_an_empty_active_set_on_an_expertless_layer() {
    // A layer can be simulated through empty active sets without
    // declaring any expert. Its quota is legitimate configuration, and
    // rejecting it would refuse a trace replay accepts.
    let manifest = ModelManifest::try_from_entries([]).unwrap();
    let events = [sample_event(7, vec![])];
    assert_eq!(
        manifest.validate_per_layer_capacity(0, &quotas(&[(7, 0)]), &events),
        Ok(())
    );
}

#[test]
fn per_layer_capacity_accepts_an_empty_quota_map_with_an_empty_trace() {
    // Nothing is simulated, so nothing needs a quota; the surface rule
    // that a per-layer run must name at least one quota belongs to the
    // CLI, not to this validator.
    let manifest = ModelManifest::try_from_entries([entry(0, 0, 1)]).unwrap();
    let no_events: [Event; 0] = [];
    let empty = BTreeMap::new();
    assert_eq!(
        manifest.validate_per_layer_capacity(0, &empty, &no_events),
        Ok(())
    );
}
