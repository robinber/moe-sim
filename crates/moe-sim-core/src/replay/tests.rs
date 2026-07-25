#![expect(
    clippy::unwrap_used,
    reason = "tests build valid manifests and events directly; direct unwraps keep failure diagnostics next to the hand-calculated fixture data"
)]

use super::*;
use crate::manifest::ExpertSizeEntry;
use crate::trace::{EventParts, Phase};

/// Global-scope shorthand: every pre-quota test replays under one budget.
fn replay_global<'a>(
    manifest: &ModelManifest,
    events: impl IntoIterator<Item = &'a Event>,
    policy: Policy,
    budget_bytes: u64,
) -> Result<ReplayMetrics, ReplayError> {
    replay(
        manifest,
        events,
        policy,
        &CacheScope::Global { budget_bytes },
    )
}

/// Builds a single-layer manifest from `(expert_id, size_bytes)` pairs.
fn manifest_of(experts: &[(u32, u64)]) -> ModelManifest {
    ModelManifest::try_from_entries(experts.iter().map(|&(expert_id, size_bytes)| {
        ExpertSizeEntry {
            key: ExpertKey::new(0, expert_id),
            size_bytes,
        }
    }))
    .unwrap()
}

/// One layer-0 event activating `expert_ids` as an atomic set.
fn ev(expert_ids: Vec<u32>) -> Event {
    Event::new(EventParts {
        request_id: 1,
        phase: Phase::Decode,
        step_id: 0,
        token_position: 0,
        layer_id: 0,
        expert_ids,
    })
    .unwrap()
}

/// The committed `two-experts-4-6` manifest.
fn two_experts_4_6() -> ModelManifest {
    manifest_of(&[(0, 4), (1, 6)])
}

// --- no-cache baseline (slice 1A contract, unchanged) ---

#[test]
fn no_cache_matches_the_hand_calculated_active_set_fixture() {
    // Mirrors fixtures/synthetic/active-set-0-1.jsonl over
    // fixtures/models/two-experts-4-6.json:
    //   event 0: {0, 1} -> 4 + 6 = 10 bytes, 2 objects
    //   event 1: {1}    ->         6 bytes,  1 object
    let manifest = two_experts_4_6();
    let events = [ev(vec![0, 1]), ev(vec![1])];

    let metrics = replay_global(&manifest, &events, Policy::NoCache, 10).unwrap();

    assert_eq!(metrics.events(), 2);
    assert_eq!(metrics.object_loads(), 3);
    assert_eq!(metrics.byte_loads(), 16);
    assert_eq!(metrics.peak_resident_bytes(), 10);
}

#[test]
fn no_cache_never_hits_and_never_evicts() {
    let manifest = two_experts_4_6();
    let events = [ev(vec![0, 1]), ev(vec![0, 1])];

    let metrics = replay_global(&manifest, &events, Policy::NoCache, 10).unwrap();

    assert_eq!(metrics.object_hits(), 0);
    assert_eq!(metrics.byte_hits(), 0);
    assert_eq!(metrics.evictions(), 0, "a release is not an eviction");
    assert_eq!(metrics.evicted_bytes(), 0);
    assert_eq!(metrics.byte_loads(), 20, "every activation reloads");
}

#[test]
fn no_cache_churn_counts_every_repeat_activation() {
    let manifest = two_experts_4_6();
    let events = [ev(vec![0, 1]), ev(vec![0, 1])];

    let metrics = replay_global(&manifest, &events, Policy::NoCache, 10).unwrap();

    // Second activation of each expert is rework, not a cold miss.
    assert_eq!(metrics.object_reloads(), 2);
    assert_eq!(metrics.byte_reloads(), 10);
}

#[test]
fn empty_trace_yields_zeroed_metrics() {
    let no_events: [Event; 0] = [];
    let metrics = replay_global(&two_experts_4_6(), &no_events, Policy::Lru, 10).unwrap();
    assert_eq!(metrics, ReplayMetrics::default());
}

#[test]
fn empty_active_set_is_an_event_that_loads_nothing() {
    let metrics = replay_global(&two_experts_4_6(), &[ev(vec![])], Policy::Lru, 10).unwrap();

    assert_eq!(metrics.events(), 1);
    assert_eq!(metrics.object_loads(), 0);
    assert_eq!(metrics.peak_resident_bytes(), 0);
}

// --- retention ---

#[test]
fn caching_policies_serve_a_repeated_active_set_from_residency() {
    let manifest = two_experts_4_6();
    let events = [ev(vec![0, 1]), ev(vec![0, 1])];

    for policy in [Policy::Lru, Policy::Lfu] {
        let metrics = replay_global(&manifest, &events, policy, 10).unwrap();

        assert_eq!(metrics.object_loads(), 2, "{policy}");
        assert_eq!(metrics.byte_loads(), 10, "{policy}");
        assert_eq!(metrics.object_hits(), 2, "{policy}");
        assert_eq!(metrics.byte_hits(), 10, "{policy}");
        assert_eq!(metrics.object_reloads(), 0, "{policy}");
    }
}

#[test]
fn the_global_cache_distinguishes_equal_expert_ids_across_layers() {
    // (layer 0, expert 0) and (layer 1, expert 0) are distinct objects
    // that merely share an expert id. With room for exactly one, the
    // second activation must be a cold load that evicts the first, not a
    // hit on an aliased key.
    let manifest = ModelManifest::try_from_entries([
        ExpertSizeEntry {
            key: ExpertKey::new(0, 0),
            size_bytes: 5,
        },
        ExpertSizeEntry {
            key: ExpertKey::new(1, 0),
            size_bytes: 5,
        },
    ])
    .unwrap();
    let on_layer = |layer_id: u32| {
        Event::new(EventParts {
            request_id: 1,
            phase: Phase::Decode,
            step_id: 0,
            token_position: 0,
            layer_id,
            expert_ids: vec![0],
        })
        .unwrap()
    };
    let events = [on_layer(0), on_layer(1)];

    let metrics = replay_global(&manifest, &events, Policy::Lru, 5).unwrap();

    assert_eq!(metrics.object_loads(), 2, "layer 1 expert 0 is cold");
    assert_eq!(metrics.object_hits(), 0);
    assert_eq!(metrics.evictions(), 1);
    assert_eq!(metrics.peak_resident_bytes(), 5);
}

// --- policies choose different victims ---

/// `A A A B C A` with room for two 5-byte experts.
///
/// When `C` arrives the cache holds `A` (frequency 3, older) and `B`
/// (frequency 1, newer). LFU evicts `B` and keeps `A`; LRU evicts `A`.
/// The final `A` therefore hits under LFU and reloads under LRU.
fn frequency_versus_recency_trace() -> (ModelManifest, Vec<Event>) {
    let manifest = manifest_of(&[(0, 5), (1, 5), (2, 5)]);
    let events = vec![
        ev(vec![0]),
        ev(vec![0]),
        ev(vec![0]),
        ev(vec![1]),
        ev(vec![2]),
        ev(vec![0]),
    ];
    (manifest, events)
}

#[test]
fn lfu_keeps_the_frequently_used_object() {
    let (manifest, events) = frequency_versus_recency_trace();

    let metrics = replay_global(&manifest, &events, Policy::Lfu, 10).unwrap();

    assert_eq!(metrics.events(), 6);
    assert_eq!(metrics.object_loads(), 3);
    assert_eq!(metrics.byte_loads(), 15);
    assert_eq!(metrics.object_hits(), 3);
    assert_eq!(metrics.byte_hits(), 15);
    assert_eq!(
        metrics.object_reloads(),
        0,
        "LFU never drops the hot object"
    );
    assert_eq!(metrics.evictions(), 1);
    assert_eq!(metrics.evicted_bytes(), 5);
}

#[test]
fn lru_drops_the_frequently_used_object_when_it_ages() {
    let (manifest, events) = frequency_versus_recency_trace();

    let metrics = replay_global(&manifest, &events, Policy::Lru, 10).unwrap();

    assert_eq!(metrics.events(), 6);
    assert_eq!(metrics.object_loads(), 4);
    assert_eq!(metrics.byte_loads(), 20);
    assert_eq!(metrics.object_hits(), 2);
    assert_eq!(
        metrics.object_reloads(),
        1,
        "the aged hot object comes back"
    );
    assert_eq!(metrics.byte_reloads(), 5);
    assert_eq!(metrics.evictions(), 2);
}

#[test]
fn lfu_breaks_frequency_ties_by_recency_before_key_order() {
    // After [0, 1, 1, 0] both experts sit at frequency 2, but 1's last
    // use is older than 0's. When 2 arrives, LFU must evict 1 (least
    // recent), not 0 (lowest key): the final event then hits.
    let manifest = manifest_of(&[(0, 5), (1, 5), (2, 5)]);
    let events = [
        ev(vec![0]),
        ev(vec![1]),
        ev(vec![1]),
        ev(vec![0]),
        ev(vec![2]),
        ev(vec![0]),
    ];

    let metrics = replay_global(&manifest, &events, Policy::Lfu, 10).unwrap();

    assert_eq!(
        metrics.object_loads(),
        3,
        "a key-order tie-break would reload 0"
    );
    assert_eq!(metrics.object_hits(), 3);
    assert_eq!(metrics.object_reloads(), 0);
    assert_eq!(metrics.evictions(), 1);
}

#[test]
fn lfu_frequency_restarts_after_readmission() {
    // Expert 0 is made hot (frequency 3), forced out by an atomic set that
    // needs the whole budget, then re-admitted. On re-admission its count
    // must restart at 1, which loses the next eviction race against expert
    // 1 at frequency 2 — so 0 is dropped again and the final event reloads
    // it.
    //
    // An LFU that carried the old lifetime frequency instead would give 0
    // a count of 4, keep it resident, and turn that final event into a
    // hit. The exact totals below are what separates the two semantics; a
    // `>= 1` assertion would not, because expert 1 supplies a reload
    // either way.
    //
    //   e0..e2 {0}     0 resident, frequency 3
    //   e3     {1, 2}  needs the full budget, so 0 is evicted
    //   e4     {1}     1 reaches frequency 2
    //   e5     {0}     evicts 2 (lowest frequency), re-admits 0
    //   e6     {3}     evicts 0 under a restarted count, keeps 1
    //   e7     {0}     therefore a reload, not a hit
    let manifest = manifest_of(&[(0, 5), (1, 5), (2, 5), (3, 5)]);
    let events = [
        ev(vec![0]),
        ev(vec![0]),
        ev(vec![0]),
        ev(vec![1, 2]),
        ev(vec![1]),
        ev(vec![0]),
        ev(vec![3]),
        ev(vec![0]),
    ];

    let metrics = replay_global(&manifest, &events, Policy::Lfu, 10).unwrap();

    assert_eq!(metrics.events(), 8);
    assert_eq!(
        metrics.object_hits(),
        3,
        "a carried count would hit 4 times"
    );
    assert_eq!(metrics.object_loads(), 6, "a carried count would load 5");
    assert_eq!(
        metrics.object_reloads(),
        2,
        "a carried count would reload only once"
    );
    assert_eq!(metrics.byte_reloads(), 10);
    assert_eq!(metrics.evictions(), 4);
}

// --- adversarial: atomic pinning ---

#[test]
fn a_pinned_member_survives_even_when_the_policy_would_evict_it() {
    // Budget holds exactly two 5-byte experts.
    //   e0 {0, 1}   -> both resident, cache full
    //   e1 {0, 2}   -> 0 is pinned and is also LRU's natural victim,
    //                  so 1 must be evicted instead
    //   e2 {0}      -> hits only if pinning protected 0
    let manifest = manifest_of(&[(0, 5), (1, 5), (2, 5)]);
    let events = [ev(vec![0, 1]), ev(vec![0, 2]), ev(vec![0])];

    for policy in [Policy::Lru, Policy::Lfu] {
        let metrics = replay_global(&manifest, &events, policy, 10).unwrap();

        assert_eq!(
            metrics.object_reloads(),
            0,
            "{policy} evicted a pinned member of the active set"
        );
        assert_eq!(metrics.evictions(), 1, "{policy}");
        assert_eq!(metrics.object_loads(), 3, "{policy}");
    }
}

#[test]
fn an_atomic_set_is_never_partially_admitted() {
    // Both members must be resident together; the budget fits exactly one
    // pair, so the alternating sets evict each other wholesale.
    let manifest = manifest_of(&[(0, 5), (1, 5), (2, 5), (3, 5)]);
    let events = [ev(vec![0, 1]), ev(vec![2, 3]), ev(vec![0, 1])];

    for policy in [Policy::Lru, Policy::Lfu] {
        let metrics = replay_global(&manifest, &events, policy, 10).unwrap();

        assert_eq!(metrics.object_loads(), 6, "{policy}");
        assert_eq!(metrics.evictions(), 4, "{policy}");
        assert_eq!(metrics.object_reloads(), 2, "{policy}");
        assert_eq!(metrics.peak_resident_bytes(), 10, "{policy}");
    }
}

// --- adversarial: one atomic set is one access ---

#[test]
fn a_genuine_tie_inside_one_atomic_set_evicts_the_lowest_key() {
    // {0, 1} are admitted and accessed by the same event, so they tie on
    // recency and on frequency. The documented rule evicts the lowest
    // expert key first: after {2} forces one eviction, a final access to
    // 1 hits while a final access to 0 reloads, under both policies.
    let manifest = manifest_of(&[(0, 5), (1, 5), (2, 5)]);

    for policy in [Policy::Lru, Policy::Lfu] {
        let survivor = replay_global(
            &manifest,
            &[ev(vec![0, 1]), ev(vec![2]), ev(vec![1])],
            policy,
            10,
        )
        .unwrap();
        let victim = replay_global(
            &manifest,
            &[ev(vec![0, 1]), ev(vec![2]), ev(vec![0])],
            policy,
            10,
        )
        .unwrap();

        assert_eq!(survivor.object_hits(), 1, "{policy}: 1 must survive");
        assert_eq!(survivor.object_reloads(), 0, "{policy}");
        assert_eq!(victim.object_hits(), 0, "{policy}: 0 loses the tie");
        assert_eq!(victim.object_reloads(), 1, "{policy}");
    }
}

#[test]
fn activation_order_inside_an_event_never_orders_recency() {
    // An atomic set is one access, so the order of `expert_ids` in the
    // event must not decide which member a later eviction removes.
    let manifest = manifest_of(&[(0, 5), (1, 5), (2, 5)]);
    let ascending = [ev(vec![0, 1]), ev(vec![2]), ev(vec![1])];
    let descending = [ev(vec![1, 0]), ev(vec![2]), ev(vec![1])];

    for policy in [Policy::Lru, Policy::Lfu] {
        let from_ascending = replay_global(&manifest, &ascending, policy, 10).unwrap();
        let from_descending = replay_global(&manifest, &descending, policy, 10).unwrap();
        assert_eq!(from_ascending, from_descending, "{policy}");
    }
}

#[test]
fn an_active_set_larger_than_the_budget_is_rejected() {
    let manifest = manifest_of(&[(0, 6), (1, 6)]);
    let events = [ev(vec![0, 1])];

    for policy in [Policy::NoCache, Policy::Lru, Policy::Lfu] {
        let err = replay_global(&manifest, &events, policy, 10).unwrap_err();
        assert!(
            matches!(
                err,
                ReplayError::ActiveSetExceedsCapacity {
                    event_index: 0,
                    active_set_bytes: 12,
                    budget_bytes: 10,
                    ..
                }
            ),
            "{policy}: unexpected error: {err}"
        );
    }
}

// --- adversarial: byte capacity ---

#[test]
fn resident_bytes_never_exceed_the_budget() {
    // Variable sizes, a shifting hot set, and a cyclic tail: residency
    // must stay within budget for every policy at every budget that can
    // hold the largest atomic set.
    let manifest = manifest_of(&[(0, 1), (1, 2), (2, 3), (3, 5), (4, 8)]);
    let events: Vec<Event> = [
        vec![0, 1],
        vec![4],
        vec![2, 3],
        vec![0],
        vec![4],
        vec![1, 2],
        vec![3],
        vec![0, 1],
        vec![4],
        vec![2],
    ]
    .into_iter()
    .map(ev)
    .collect();

    for policy in [Policy::NoCache, Policy::Lru, Policy::Lfu] {
        for budget in [8u64, 9, 12, 19, 100] {
            let metrics = replay_global(&manifest, &events, policy, budget).unwrap();
            assert!(
                metrics.peak_resident_bytes() <= budget,
                "{policy} at budget {budget}: peak {} exceeded it",
                metrics.peak_resident_bytes()
            );
        }
    }
}

#[test]
fn evicted_bytes_report_the_victims_actual_size() {
    // The 3-byte expert 0 is the LRU victim when 2 arrives; the counter
    // must report those 3 bytes, not a size correlated with the uniform
    // 5-byte objects the other fixtures use.
    let manifest = manifest_of(&[(0, 3), (1, 7), (2, 3)]);
    let events = [ev(vec![0, 1]), ev(vec![1]), ev(vec![2])];

    let metrics = replay_global(&manifest, &events, Policy::Lru, 10).unwrap();

    assert_eq!(metrics.evictions(), 1);
    assert_eq!(metrics.evicted_bytes(), 3);
}

#[test]
fn cyclic_access_defeats_lru_but_stays_within_capacity() {
    // Classic LRU adversary: cycle through one more object than fits.
    let manifest = manifest_of(&[(0, 5), (1, 5), (2, 5)]);
    let events: Vec<Event> = (0..9).map(|i| ev(vec![i % 3])).collect();

    let metrics = replay_global(&manifest, &events, Policy::Lru, 10).unwrap();

    assert_eq!(metrics.object_hits(), 0, "every access misses");
    assert_eq!(metrics.object_loads(), 9);
    assert_eq!(metrics.object_reloads(), 6, "all rework after the cold set");
    assert!(metrics.peak_resident_bytes() <= 10);
}

// --- metric identities and determinism ---

#[test]
fn loads_split_into_cold_misses_and_reloads() {
    let manifest = manifest_of(&[(0, 5), (1, 5), (2, 5)]);
    let events: Vec<Event> = (0..12).map(|i| ev(vec![i % 3])).collect();

    for policy in [Policy::NoCache, Policy::Lru, Policy::Lfu] {
        let metrics = replay_global(&manifest, &events, policy, 10).unwrap();
        let cold = metrics.object_loads() - metrics.object_reloads();
        assert_eq!(cold, 3, "{policy}: one cold miss per distinct expert");
    }
}

#[test]
fn every_activation_is_either_a_hit_or_a_load() {
    let manifest = manifest_of(&[(0, 5), (1, 5), (2, 5)]);
    let events: Vec<Event> = (0..12).map(|i| ev(vec![i % 3])).collect();
    let activations = 12;

    for policy in [Policy::NoCache, Policy::Lru, Policy::Lfu] {
        let metrics = replay_global(&manifest, &events, policy, 10).unwrap();
        assert_eq!(
            metrics.object_hits() + metrics.object_loads(),
            activations,
            "{policy}"
        );
    }
}

#[test]
fn the_identities_hold_for_multi_expert_and_empty_active_sets() {
    // The singleton cycles above cannot catch a version that counted
    // unique pinned keys instead of activation multiplicity, so this
    // fixture mixes set sizes, an empty set, and variable expert sizes.
    let manifest = manifest_of(&[(0, 1), (1, 2), (2, 3), (3, 5)]);
    let sets = [
        vec![0, 1],
        vec![],
        vec![2, 3],
        vec![0],
        vec![1, 2, 3],
        vec![],
        vec![0, 1, 2],
        vec![3],
    ];
    let sizes = [1u64, 2, 3, 5];
    let activations: u64 = sets
        .iter()
        .map(|set| u64::try_from(set.len()).unwrap())
        .sum();
    let activated_bytes: u64 = sets
        .iter()
        .flat_map(|set| set.iter().map(|&id| sizes[usize::try_from(id).unwrap()]))
        .sum();
    let events: Vec<Event> = sets.into_iter().map(ev).collect();

    for policy in [Policy::NoCache, Policy::Lru, Policy::Lfu] {
        for budget in [11u64, 20] {
            let metrics = replay_global(&manifest, &events, policy, budget).unwrap();
            assert_eq!(
                metrics.object_hits() + metrics.object_loads(),
                activations,
                "{policy} at budget {budget}"
            );
            assert_eq!(
                metrics.byte_hits() + metrics.byte_loads(),
                activated_bytes,
                "{policy} at budget {budget}: byte dual of the partition"
            );
            assert!(
                metrics.object_reloads() <= metrics.object_loads(),
                "{policy}: reloads are a subset of loads"
            );
            assert!(metrics.peak_resident_bytes() <= budget, "{policy}");
        }
    }
}

#[test]
fn equal_inputs_produce_equal_metrics() {
    let manifest = manifest_of(&[(0, 3), (1, 5), (2, 7)]);
    let events: Vec<Event> = (0..20).map(|i| ev(vec![i % 3])).collect();

    for policy in [Policy::NoCache, Policy::Lru, Policy::Lfu] {
        let first = replay_global(&manifest, &events, policy, 10).unwrap();
        let second = replay_global(&manifest, &events, policy, 10).unwrap();
        assert_eq!(first, second, "{policy}");
    }
}

#[test]
fn events_replay_in_supplied_order_regardless_of_metadata() {
    let manifest = two_experts_4_6();
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
    let events = [descending(9, vec![0, 1]), descending(0, vec![1])];

    let metrics = replay_global(&manifest, &events, Policy::NoCache, 10).unwrap();

    assert_eq!(metrics.byte_loads(), 16);
}

// --- errors ---

#[test]
fn unknown_expert_reports_the_failing_event_position() {
    let manifest = two_experts_4_6();
    let events = [ev(vec![0]), ev(vec![7])];

    let err = replay_global(&manifest, &events, Policy::Lru, 10).unwrap_err();

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
    let manifest = manifest_of(&[(0, u64::MAX), (1, 1)]);
    let events = [ev(vec![0]), ev(vec![1])];

    let err = replay_global(&manifest, &events, Policy::NoCache, u64::MAX).unwrap_err();

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
fn byte_hit_overflow_is_reported_not_wrapped() {
    // byte_hits can overflow independently of byte_loads: one load of a
    // u64::MAX expert, then two hits on it.
    let manifest = manifest_of(&[(0, u64::MAX)]);
    let events = [ev(vec![0]), ev(vec![0]), ev(vec![0])];

    let err = replay_global(&manifest, &events, Policy::Lru, u64::MAX).unwrap_err();

    assert!(
        matches!(
            err,
            ReplayError::CounterOverflow {
                counter: ReplayCounter::ByteHits,
                event_index: 2
            }
        ),
        "unexpected error: {err}"
    );
}

#[test]
fn replay_accepts_a_streaming_iterator() {
    let manifest = two_experts_4_6();
    let events = [ev(vec![0, 1]), ev(vec![1])];

    let streamed = replay_global(&manifest, events.iter(), Policy::NoCache, 10).unwrap();

    assert_eq!(streamed.byte_loads(), 16);
}

// --- per-layer quotas ---

/// Builds a manifest from `(layer_id, expert_id, size_bytes)` triples.
fn manifest_layers(experts: &[(u32, u32, u64)]) -> ModelManifest {
    ModelManifest::try_from_entries(experts.iter().map(|&(layer_id, expert_id, size_bytes)| {
        ExpertSizeEntry {
            key: ExpertKey::new(layer_id, expert_id),
            size_bytes,
        }
    }))
    .unwrap()
}

/// One event on `layer_id` activating `expert_ids` as an atomic set.
fn ev_on(layer_id: u32, expert_ids: Vec<u32>) -> Event {
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

fn per_layer(total_budget_bytes: u64, quotas: &[(u32, u64)]) -> CacheScope {
    CacheScope::PerLayer {
        total_budget_bytes,
        layer_quota_bytes: quotas.iter().copied().collect(),
    }
}

#[test]
fn unused_quota_is_not_shared_between_layers() {
    // Layer 0 is at its quota while layer 1 has 5 free bytes. The third
    // layer-0 set must evict inside layer 0 anyway, and the final event
    // pays a reload for it. A cache that pooled the two quotas would fit
    // everything: 4 loads, 1 hit, 0 evictions.
    let manifest = manifest_layers(&[(0, 0, 5), (0, 1, 5), (0, 2, 5), (1, 0, 5)]);
    let events = [
        ev_on(0, vec![0, 1]),
        ev_on(1, vec![0]),
        ev_on(0, vec![2]),
        ev_on(0, vec![0]),
    ];
    let scope = per_layer(20, &[(0, 10), (1, 10)]);

    for policy in [Policy::Lru, Policy::Lfu] {
        let metrics = replay(&manifest, &events, policy, &scope).unwrap();

        assert_eq!(metrics.object_loads(), 5, "{policy}: no pooled fit");
        assert_eq!(metrics.object_hits(), 0, "{policy}");
        assert_eq!(metrics.object_reloads(), 1, "{policy}");
        assert_eq!(metrics.evictions(), 2, "{policy}");
    }
}

#[test]
fn per_layer_peaks_respect_each_quota_on_adversarial_traces() {
    // Variable sizes, shifting hot sets, and interleaved layers: every
    // layer cache must stay inside its own quota, and total residency
    // inside the total budget, for every policy at every feasible quota
    // pair.
    let manifest = manifest_layers(&[
        (0, 0, 1),
        (0, 1, 2),
        (0, 2, 3),
        (0, 3, 5),
        (0, 4, 8),
        (1, 0, 2),
        (1, 1, 4),
        (1, 2, 6),
    ]);
    let events = [
        ev_on(0, vec![0, 1]),
        ev_on(1, vec![0, 1]),
        ev_on(0, vec![4]),
        ev_on(1, vec![2]),
        ev_on(0, vec![2, 3]),
        ev_on(0, vec![0]),
        ev_on(1, vec![1]),
        ev_on(0, vec![4]),
        ev_on(1, vec![0, 1]),
        ev_on(0, vec![2]),
    ];

    for policy in [Policy::NoCache, Policy::Lru, Policy::Lfu] {
        for (quota_0, quota_1) in [(8u64, 6u64), (9, 7), (12, 10), (100, 100)] {
            let total = quota_0 + quota_1;
            let scope = per_layer(total, &[(0, quota_0), (1, quota_1)]);
            let metrics = replay(&manifest, &events, policy, &scope).unwrap();

            let peaks = metrics.layer_peak_resident_bytes();
            assert!(
                peaks[&0] <= quota_0,
                "{policy} at quotas ({quota_0}, {quota_1}): layer 0 peak {} exceeded its quota",
                peaks[&0]
            );
            assert!(
                peaks[&1] <= quota_1,
                "{policy} at quotas ({quota_0}, {quota_1}): layer 1 peak {} exceeded its quota",
                peaks[&1]
            );
            assert!(
                metrics.peak_resident_bytes() <= total,
                "{policy} at quotas ({quota_0}, {quota_1}): total peak {} exceeded the budget",
                metrics.peak_resident_bytes()
            );
            assert!(
                metrics.peak_resident_bytes() <= peaks[&0] + peaks[&1],
                "{policy}: summed residency can never top the summed peaks"
            );
        }
    }
}

#[test]
fn a_pinned_member_survives_per_layer_pressure_with_interleaved_layers() {
    // Within layer 0 this is the classic pinning adversary: the resident
    // member of the active set is also the policy's natural victim. The
    // interleaved layer-1 events must not disturb layer 0's recency or
    // pinning in either direction.
    let manifest = manifest_layers(&[(0, 0, 5), (0, 1, 5), (0, 2, 5), (1, 0, 5)]);
    let events = [
        ev_on(0, vec![0, 1]),
        ev_on(1, vec![0]),
        ev_on(0, vec![0, 2]),
        ev_on(1, vec![0]),
        ev_on(0, vec![0]),
    ];
    let scope = per_layer(15, &[(0, 10), (1, 5)]);

    for policy in [Policy::Lru, Policy::Lfu] {
        let metrics = replay(&manifest, &events, policy, &scope).unwrap();

        assert_eq!(
            metrics.object_reloads(),
            0,
            "{policy} evicted a pinned member of the active set"
        );
        assert_eq!(metrics.object_hits(), 3, "{policy}");
        assert_eq!(metrics.object_loads(), 4, "{policy}");
        assert_eq!(metrics.evictions(), 1, "{policy}");
    }
}

#[test]
fn an_active_set_exceeding_its_layer_quota_is_rejected_within_the_total_budget() {
    // The total budget would hold the set easily; its layer quota does
    // not, and unused budget must not absorb the excess.
    let manifest = manifest_layers(&[(0, 0, 6), (0, 1, 6)]);
    let events = [ev_on(0, vec![0, 1])];
    let scope = per_layer(100, &[(0, 10)]);

    for policy in [Policy::NoCache, Policy::Lru, Policy::Lfu] {
        let err = replay(&manifest, &events, policy, &scope).unwrap_err();
        assert_eq!(
            err,
            ReplayError::ActiveSetExceedsLayerQuota {
                event_index: 0,
                request_id: 1,
                layer_id: 0,
                active_set_bytes: 12,
                quota_bytes: 10,
            },
            "{policy}"
        );
    }
}

#[test]
fn an_event_on_a_layer_without_a_quota_is_rejected() {
    let manifest = manifest_layers(&[(0, 0, 5), (1, 0, 5)]);
    let events = [ev_on(0, vec![0]), ev_on(1, vec![0])];
    let scope = per_layer(10, &[(0, 10)]);

    let err = replay(&manifest, &events, Policy::Lru, &scope).unwrap_err();
    assert_eq!(
        err,
        ReplayError::MissingLayerQuota {
            event_index: 1,
            request_id: 1,
            layer_id: 1,
        }
    );
}

#[test]
fn quotas_breaking_the_total_budget_are_rejected_before_any_event() {
    let manifest = manifest_layers(&[(0, 0, 5), (1, 0, 5)]);
    let events = [ev_on(0, vec![0])];

    let err = replay(
        &manifest,
        &events,
        Policy::Lru,
        &per_layer(17, &[(0, 10), (1, 8)]),
    )
    .unwrap_err();
    assert_eq!(
        err,
        ReplayError::LayerQuotaSumExceedsTotalBudget {
            quota_sum_bytes: 18,
            total_budget_bytes: 17,
        }
    );

    let err = replay(
        &manifest,
        &events,
        Policy::Lru,
        &per_layer(u64::MAX, &[(0, u64::MAX), (1, 1)]),
    )
    .unwrap_err();
    assert_eq!(err, ReplayError::LayerQuotaSumOverflow);
}

#[test]
fn aggregate_peak_is_the_high_water_of_summed_residency_not_the_sum_of_peaks() {
    // Under no-cache the two layers are never resident at the same
    // instant, so the aggregate peak is layer 0's 10 bytes while the
    // per-layer peaks sum to 18. An implementation that added the peaks
    // would overstate residency.
    let manifest = manifest_layers(&[(0, 0, 4), (0, 1, 6), (1, 0, 5), (1, 1, 3)]);
    let events = [ev_on(0, vec![0, 1]), ev_on(1, vec![0, 1])];
    let scope = per_layer(18, &[(0, 10), (1, 8)]);

    let metrics = replay(&manifest, &events, Policy::NoCache, &scope).unwrap();

    assert_eq!(metrics.peak_resident_bytes(), 10);
    assert_eq!(metrics.layer_peak_resident_bytes()[&0], 10);
    assert_eq!(metrics.layer_peak_resident_bytes()[&1], 8);
    assert_eq!(metrics.evictions(), 0, "a release is not an eviction");
}

#[test]
fn aggregate_peak_counts_simultaneous_residency_across_layers() {
    // Retained caches: both layers are resident at once after the second
    // event, so the aggregate peak must be their sum. An implementation
    // that reported the largest individual layer peak would read 10.
    let manifest = manifest_layers(&[(0, 0, 4), (0, 1, 6), (1, 0, 5), (1, 1, 3)]);
    let events = [ev_on(0, vec![0, 1]), ev_on(1, vec![0, 1])];
    let scope = per_layer(18, &[(0, 10), (1, 8)]);

    for policy in [Policy::Lru, Policy::Lfu] {
        let metrics = replay(&manifest, &events, policy, &scope).unwrap();
        assert_eq!(metrics.peak_resident_bytes(), 18, "{policy}");
        assert_eq!(metrics.layer_peak_resident_bytes()[&0], 10, "{policy}");
        assert_eq!(metrics.layer_peak_resident_bytes()[&1], 8, "{policy}");
    }
}

#[test]
fn per_layer_with_one_layer_matches_the_global_metrics() {
    // A single quota covering the whole budget is the same simulation as
    // a global cache of that size; only the layer breakdown differs.
    let (manifest, events) = frequency_versus_recency_trace();

    for policy in [Policy::NoCache, Policy::Lru, Policy::Lfu] {
        let global = replay_global(&manifest, &events, policy, 10).unwrap();
        let scoped = replay(&manifest, &events, policy, &per_layer(10, &[(0, 10)])).unwrap();

        assert_eq!(scoped.object_loads(), global.object_loads(), "{policy}");
        assert_eq!(scoped.byte_loads(), global.byte_loads(), "{policy}");
        assert_eq!(scoped.object_hits(), global.object_hits(), "{policy}");
        assert_eq!(scoped.byte_hits(), global.byte_hits(), "{policy}");
        assert_eq!(scoped.object_reloads(), global.object_reloads(), "{policy}");
        assert_eq!(scoped.evictions(), global.evictions(), "{policy}");
        assert_eq!(
            scoped.peak_resident_bytes(),
            global.peak_resident_bytes(),
            "{policy}"
        );
    }
}

#[test]
fn per_layer_replay_is_deterministic() {
    let manifest = manifest_layers(&[(0, 0, 3), (0, 1, 5), (1, 0, 4), (1, 1, 2)]);
    let events: Vec<Event> = (0..20)
        .map(|i| ev_on(u32::from(i % 2 == 1), vec![i % 2]))
        .collect();
    let scope = per_layer(14, &[(0, 8), (1, 6)]);

    for policy in [Policy::NoCache, Policy::Lru, Policy::Lfu] {
        let first = replay(&manifest, &events, policy, &scope).unwrap();
        let second = replay(&manifest, &events, policy, &scope).unwrap();
        assert_eq!(first, second, "{policy}");
    }
}

#[test]
fn unactivated_quota_layers_report_a_zero_peak() {
    // The quota is part of the configuration, so the report shows the
    // layer even when the trace never touches it.
    let manifest = manifest_layers(&[(0, 0, 5), (1, 0, 5)]);
    let events = [ev_on(0, vec![0])];
    let scope = per_layer(15, &[(0, 10), (1, 5)]);

    let metrics = replay(&manifest, &events, Policy::Lru, &scope).unwrap();

    assert_eq!(
        metrics.layer_peak_resident_bytes(),
        &[(0u32, 5u64), (1u32, 0u64)].into_iter().collect()
    );
}
