//! Bounded exhaustive oracle for offline caching optima, and the slice 1C
//! gate: greedy farthest-next-use ([`Policy::Belady`]) must match the
//! oracle's optimum on enumerated uniform-size traces.
//!
//! The oracle's objective and limits are explicit. It minimizes **total
//! bytes loaded** over the whole trace under one byte budget, with each
//! event's atomic active set fully resident and pinned while its event runs,
//! and loads happening only when an activation demands them. It explores
//! every reachable resident set, so it deliberately refuses anything larger
//! than tiny cases: at most 12 events and 8 distinct experts. Under a
//! uniform expert size, minimum bytes loaded and minimum object loads
//! coincide, so the same oracle also checks the uniform-size object-load
//! objective that Belady declares.

#![expect(
    unused_crate_dependencies,
    reason = "integration-test targets receive every package dependency; this test only drives the moe_sim_core library"
)]
#![expect(
    clippy::unwrap_used,
    reason = "tests build valid manifests and events directly; direct unwraps keep failure diagnostics next to the hand-calculated fixture data"
)]

use std::collections::BTreeSet;

use moe_sim_core::{
    CacheScope, Event, EventParts, ExpertKey, ExpertSizeEntry, ModelManifest, Phase, Policy, replay,
};

const MAX_ORACLE_EVENTS: usize = 12;
const MAX_ORACLE_EXPERTS: usize = 8;

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

/// Minimum total bytes loaded to serve `events` in order under one byte
/// budget, over every valid eviction schedule.
///
/// Dynamic program over `(event, resident set)`: serving an event loads its
/// missing members, then any subset of the other resident objects may be
/// kept alongside the pinned active set as long as the result fits the
/// budget. Loading earlier than demanded can never cost fewer bytes, so the
/// demand-driven schedule space contains an optimum.
///
/// # Errors
///
/// Refuses traces beyond the declared bounds, traces whose atomic active set
/// cannot fit the budget at all, and traces whose exact byte total would
/// overflow `u64` — an exact oracle must refuse what it cannot represent
/// instead of relying on build-mode overflow behavior.
fn oracle_min_loaded_bytes(
    manifest: &ModelManifest,
    events: &[Event],
    budget_bytes: u64,
) -> Result<u64, String> {
    if events.len() > MAX_ORACLE_EVENTS {
        return Err(format!(
            "oracle bound exceeded: {} events, limit is {MAX_ORACLE_EVENTS}",
            events.len()
        ));
    }
    let keys: Vec<ExpertKey> = events
        .iter()
        .flat_map(|event| {
            event
                .expert_ids()
                .iter()
                .map(|&expert_id| ExpertKey::new(event.layer_id(), expert_id))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if keys.len() > MAX_ORACLE_EXPERTS {
        return Err(format!(
            "oracle bound exceeded: {} distinct experts, limit is {MAX_ORACLE_EXPERTS}",
            keys.len()
        ));
    }
    let sizes: Vec<u64> = keys
        .iter()
        .map(|&key| manifest.size_bytes(key).unwrap())
        .collect();
    // `None` marks a byte total beyond `u64`: such a set can never fit a
    // `u64` budget, and a cost that reaches it must be refused.
    let mask_bytes = |mask: usize| -> Option<u64> {
        sizes
            .iter()
            .enumerate()
            .filter(|&(expert, _)| mask & (1 << expert) != 0)
            .try_fold(0u64, |total, (_, &size)| total.checked_add(size))
    };

    let states = 1usize << keys.len();
    let mut best: Vec<Option<u64>> = vec![None; states];
    best[0] = Some(0);
    for event in events {
        let active_mask = event.expert_ids().iter().fold(0usize, |mask, &expert_id| {
            let key = ExpertKey::new(event.layer_id(), expert_id);
            mask | (1 << keys.iter().position(|&candidate| candidate == key).unwrap())
        });
        if mask_bytes(active_mask).is_none_or(|active_bytes| active_bytes > budget_bytes) {
            return Err("an atomic active set exceeds the budget".to_owned());
        }
        let mut next_best: Vec<Option<u64>> = vec![None; states];
        for (resident_mask, &reachable) in best.iter().enumerate() {
            let Some(cost) = reachable else {
                continue;
            };
            // The missing members are a subset of the checked active set, so
            // their byte total cannot overflow.
            let loaded = mask_bytes(active_mask & !resident_mask).unwrap();
            let Some(cost) = cost.checked_add(loaded) else {
                return Err("the oracle's byte total overflowed u64".to_owned());
            };
            let optional = resident_mask & !active_mask;
            let mut keep = optional;
            loop {
                let state = keep | active_mask;
                if mask_bytes(state).is_some_and(|state_bytes| state_bytes <= budget_bytes)
                    && next_best[state].is_none_or(|existing| cost < existing)
                {
                    next_best[state] = Some(cost);
                }
                if keep == 0 {
                    break;
                }
                keep = (keep - 1) & optional;
            }
        }
        best = next_best;
    }
    Ok(best.iter().flatten().copied().min().unwrap_or(0))
}

#[test]
fn belady_matches_the_oracle_on_every_single_expert_trace() {
    // Four uniform experts, a two-object budget, and every trace of five
    // single-expert events: 1024 cases. Sizes are 1, so the oracle's
    // minimum bytes equal minimum object loads, the objective Belady
    // declares; LRU bounds the optimum from above as a sanity check on the
    // oracle itself.
    let manifest = manifest_layers(&[(0, 0, 1), (0, 1, 1), (0, 2, 1), (0, 3, 1)]);
    let scope = CacheScope::Global { budget_bytes: 2 };
    for trace_id in 0..4u32.pow(5) {
        let mut remaining = trace_id;
        let events: Vec<Event> = (0..5)
            .map(|_| {
                let expert_id = remaining % 4;
                remaining /= 4;
                ev_on(0, vec![expert_id])
            })
            .collect();
        let optimum = oracle_min_loaded_bytes(&manifest, &events, 2).unwrap();
        let belady = replay(&manifest, &events, Policy::Belady, &scope).unwrap();
        assert_eq!(belady.object_loads(), optimum, "trace {trace_id}");
        let lru = replay(&manifest, &events, Policy::Lru, &scope).unwrap();
        assert!(optimum <= lru.object_loads(), "trace {trace_id}");
    }
}

#[test]
fn belady_matches_the_oracle_on_every_atomic_set_trace() {
    // Three uniform experts, a two-object budget, and every trace of four
    // events drawn from the six nonempty active sets of at most two
    // members: 1296 cases in which same-event ties are common. Whatever the
    // tie resolution, the load count must still land on the optimum.
    let manifest = manifest_layers(&[(0, 0, 1), (0, 1, 1), (0, 2, 1)]);
    let sets: [&[u32]; 6] = [&[0], &[1], &[2], &[0, 1], &[0, 2], &[1, 2]];
    let scope = CacheScope::Global { budget_bytes: 2 };
    for trace_id in 0..6usize.pow(4) {
        let mut remaining = trace_id;
        let events: Vec<Event> = (0..4)
            .map(|_| {
                let set = sets[remaining % 6];
                remaining /= 6;
                ev_on(0, set.to_vec())
            })
            .collect();
        let optimum = oracle_min_loaded_bytes(&manifest, &events, 2).unwrap();
        let belady = replay(&manifest, &events, Policy::Belady, &scope).unwrap();
        assert_eq!(belady.object_loads(), optimum, "trace {trace_id}");
    }
}

#[test]
fn the_oracle_confirms_a_hand_computed_variable_size_optimum() {
    // Experts: 0 = 3 bytes, 1 = 2 bytes, 2 = 2 bytes; budget 5 bytes.
    // Trace: [0], [1], [2], [1], [0]. Serving [2] forces one eviction:
    //   evict 1 -> reload 1, then 0 hits:  3 + 2 + 2 + 2 = 9 bytes
    //   evict 0 -> 1 hits, then reload 0:  3 + 2 + 2 + 3 = 10 bytes
    // The optimum evicts the object with the *nearer* next use because the
    // larger object would cost a bigger reload: exactly the variable-size
    // regime where no greedy schedule-based rule is a proven optimum.
    let manifest = manifest_layers(&[(0, 0, 3), (0, 1, 2), (0, 2, 2)]);
    let events = vec![
        ev_on(0, vec![0]),
        ev_on(0, vec![1]),
        ev_on(0, vec![2]),
        ev_on(0, vec![1]),
        ev_on(0, vec![0]),
    ];
    assert_eq!(oracle_min_loaded_bytes(&manifest, &events, 5).unwrap(), 9);

    // LRU evicts by recency and pays the larger reload on the same trace.
    let scope = CacheScope::Global { budget_bytes: 5 };
    let lru = replay(&manifest, &events, Policy::Lru, &scope).unwrap();
    assert_eq!(lru.byte_loads(), 10);

    // Belady refuses the variable-size manifest instead of approximating.
    assert!(replay(&manifest, &events, Policy::Belady, &scope).is_err());
}

#[test]
fn the_oracle_refuses_cases_beyond_its_declared_bounds() {
    let manifest = manifest_layers(&[(0, 0, 1)]);
    let too_many_events: Vec<Event> = (0..13).map(|_| ev_on(0, vec![0])).collect();
    let error = oracle_min_loaded_bytes(&manifest, &too_many_events, 1).unwrap_err();
    assert_eq!(error, "oracle bound exceeded: 13 events, limit is 12");

    let wide_manifest = manifest_layers(
        &(0..9u32)
            .map(|expert_id| (0, expert_id, 1))
            .collect::<Vec<_>>(),
    );
    let wide_events: Vec<Event> = (0..9u32)
        .map(|expert_id| ev_on(0, vec![expert_id]))
        .collect();
    let error = oracle_min_loaded_bytes(&wide_manifest, &wide_events, 9).unwrap_err();
    assert_eq!(
        error,
        "oracle bound exceeded: 9 distinct experts, limit is 8"
    );

    let pair = manifest_layers(&[(0, 0, 1), (0, 1, 1)]);
    let infeasible = vec![ev_on(0, vec![0, 1])];
    let error = oracle_min_loaded_bytes(&pair, &infeasible, 1).unwrap_err();
    assert_eq!(error, "an atomic active set exceeds the budget");

    // Each demand fits the budget on its own, but the exact total does not
    // fit u64: the oracle refuses instead of wrapping or panicking.
    let huge = manifest_layers(&[(0, 0, u64::MAX - 1), (0, 1, u64::MAX - 1)]);
    let overflowing = vec![ev_on(0, vec![0]), ev_on(0, vec![1])];
    let error = oracle_min_loaded_bytes(&huge, &overflowing, u64::MAX).unwrap_err();
    assert_eq!(error, "the oracle's byte total overflowed u64");
}

#[test]
fn per_layer_belady_matches_the_sum_of_per_layer_optima() {
    // Per-layer caches are independent and every event runs entirely inside
    // its own layer's cache, so the per-layer optimum is the sum of each
    // layer's global optimum over its own subtrace. Belady under per-layer
    // quotas must land exactly there on this uniform-size trace.
    let manifest = manifest_layers(&[
        (0, 0, 1),
        (0, 1, 1),
        (0, 2, 1),
        (1, 0, 1),
        (1, 1, 1),
        (1, 2, 1),
    ]);
    let events = vec![
        ev_on(0, vec![0, 1]),
        ev_on(1, vec![0]),
        ev_on(0, vec![2]),
        ev_on(1, vec![1]),
        ev_on(0, vec![0, 1]),
        ev_on(1, vec![2]),
        ev_on(0, vec![2]),
        ev_on(1, vec![0]),
    ];
    let scope = CacheScope::PerLayer {
        total_budget_bytes: 3,
        layer_quota_bytes: [(0, 2), (1, 1)].into_iter().collect(),
    };
    let belady = replay(&manifest, &events, Policy::Belady, &scope).unwrap();

    let optimum: u64 = [(0u32, 2u64), (1, 1)]
        .into_iter()
        .map(|(layer_id, quota_bytes)| {
            let subtrace: Vec<Event> = events
                .iter()
                .filter(|event| event.layer_id() == layer_id)
                .cloned()
                .collect();
            oracle_min_loaded_bytes(&manifest, &subtrace, quota_bytes).unwrap()
        })
        .sum();
    assert_eq!(belady.object_loads(), optimum);
}
