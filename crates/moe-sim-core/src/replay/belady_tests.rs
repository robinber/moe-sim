//! Unit tests for [`Policy::Belady`], the offline farthest-next-use
//! reference.
//!
//! There is deliberately no test that discriminates the lowest-key tie rule
//! through metrics: two entries tie only when their next use is the same
//! event, that event readmits both atomically, and the two branches
//! reconverge there with identical counters. The tie rule exists for
//! determinism; the shared comparator tail is discriminated by the LRU/LFU
//! tie tests, and the oracle enumeration in `tests/belady_oracle.rs` pins
//! the optimum under whichever resolution runs.

#![expect(
    clippy::unwrap_used,
    reason = "tests build valid manifests and events directly; direct unwraps keep failure diagnostics next to the hand-calculated fixture data"
)]

use super::tests::{ev, ev_on, manifest_layers, manifest_of, per_layer, replay_global};
use super::*;

#[test]
fn belady_keeps_the_object_needed_soonest() {
    // Three uniform experts cycling through a two-object budget: the classic
    // trace where LRU thrashes and MIN does not.
    //   e2: {0 next e3, 1 next e4} -> evict 1 (farthest)
    //   e4: {0 never again, 2 next e5} -> evict 0 (never again)
    let manifest = manifest_of(&[(0, 2), (1, 2), (2, 2)]);
    let events = [
        ev(vec![0]),
        ev(vec![1]),
        ev(vec![2]),
        ev(vec![0]),
        ev(vec![1]),
        ev(vec![2]),
    ];

    let belady = replay_global(&manifest, &events, Policy::Belady, 4).unwrap();
    assert_eq!(belady.events(), 6);
    assert_eq!(belady.object_loads(), 4);
    assert_eq!(belady.byte_loads(), 8);
    assert_eq!(belady.object_hits(), 2);
    assert_eq!(belady.byte_hits(), 4);
    assert_eq!(belady.object_reloads(), 1);
    assert_eq!(belady.byte_reloads(), 2);
    assert_eq!(belady.evictions(), 2);
    assert_eq!(belady.evicted_bytes(), 4);
    assert_eq!(belady.peak_resident_bytes(), 4);

    // The same trace under LRU evicts the object needed next every time.
    let lru = replay_global(&manifest, &events, Policy::Lru, 4).unwrap();
    assert_eq!(lru.object_loads(), 6);
    assert_eq!(lru.object_hits(), 0);
}

#[test]
fn belady_evicts_a_never_reused_object_first() {
    // e2: {0 next e3, 1 never again} -> evict 1, so e3 hits. A recency
    // policy would evict 0 and pay a reload instead.
    let manifest = manifest_of(&[(0, 1), (1, 1), (2, 1)]);
    let events = [ev(vec![0]), ev(vec![1]), ev(vec![2]), ev(vec![0])];

    let metrics = replay_global(&manifest, &events, Policy::Belady, 2).unwrap();
    assert_eq!(metrics.object_loads(), 3);
    assert_eq!(metrics.object_hits(), 1);
    assert_eq!(metrics.object_reloads(), 0);
    assert_eq!(metrics.evictions(), 1);
}

#[test]
fn a_pinned_member_survives_even_when_its_next_use_is_nearest() {
    // At e1 the schedule alone would evict 1 (never used again), but 1 is
    // pinned by the running event, so 0 goes despite being needed at e2.
    //   e1 {1, 2}: hit 1, evict 0, load 2
    //   e2 {0}: reload 0, evicting one of the never-reused survivors
    let manifest = manifest_of(&[(0, 2), (1, 2), (2, 2)]);
    let events = [ev(vec![0, 1]), ev(vec![1, 2]), ev(vec![0])];

    let metrics = replay_global(&manifest, &events, Policy::Belady, 4).unwrap();
    assert_eq!(metrics.object_loads(), 4);
    assert_eq!(metrics.byte_loads(), 8);
    assert_eq!(metrics.object_hits(), 1);
    assert_eq!(metrics.byte_hits(), 2);
    assert_eq!(metrics.object_reloads(), 1);
    assert_eq!(metrics.byte_reloads(), 2);
    assert_eq!(metrics.evictions(), 2);
    assert_eq!(metrics.evicted_bytes(), 4);
}

#[test]
fn a_variable_size_manifest_is_rejected_for_belady() {
    // Applicability is a property of the manifest, not the trace: even an
    // empty trace is rejected, and the error names the first size mismatch
    // in ascending key order.
    let manifest = manifest_of(&[(0, 4), (1, 6)]);
    let expected = ReplayError::BeladyRequiresUniformSizes {
        first_layer_id: 0,
        first_expert_id: 0,
        first_size_bytes: 4,
        second_layer_id: 0,
        second_expert_id: 1,
        second_size_bytes: 6,
    };

    let events = [ev(vec![0])];
    let error = replay_global(&manifest, &events, Policy::Belady, 10).unwrap_err();
    assert_eq!(error, expected);

    let error = replay_global(&manifest, &[], Policy::Belady, 10).unwrap_err();
    assert_eq!(error, expected);
}

#[test]
fn uniform_and_empty_manifests_are_accepted_for_belady() {
    let uniform = manifest_of(&[(0, 3), (1, 3)]);
    let metrics = replay_global(&uniform, &[], Policy::Belady, 3).unwrap();
    assert_eq!(metrics.events(), 0);

    let empty = manifest_of(&[]);
    let metrics = replay_global(&empty, &[], Policy::Belady, 0).unwrap();
    assert_eq!(metrics.events(), 0);

    // A single-expert manifest is trivially uniform and replays normally.
    let single = manifest_of(&[(0, 4)]);
    let events = [ev(vec![0]), ev(vec![0])];
    let metrics = replay_global(&single, &events, Policy::Belady, 4).unwrap();
    assert_eq!(metrics.object_loads(), 1);
    assert_eq!(metrics.object_hits(), 1);
}

#[test]
fn belady_under_per_layer_matches_global_on_a_single_layer() {
    let manifest = manifest_of(&[(0, 2), (1, 2), (2, 2)]);
    let events = [
        ev(vec![0]),
        ev(vec![1]),
        ev(vec![2]),
        ev(vec![0]),
        ev(vec![1]),
        ev(vec![2]),
    ];

    let global = replay_global(&manifest, &events, Policy::Belady, 4).unwrap();
    let scoped = replay(&manifest, &events, Policy::Belady, &per_layer(4, &[(0, 4)])).unwrap();

    assert_eq!(scoped.object_loads(), global.object_loads());
    assert_eq!(scoped.byte_loads(), global.byte_loads());
    assert_eq!(scoped.object_hits(), global.object_hits());
    assert_eq!(scoped.byte_hits(), global.byte_hits());
    assert_eq!(scoped.object_reloads(), global.object_reloads());
    assert_eq!(scoped.byte_reloads(), global.byte_reloads());
    assert_eq!(scoped.evictions(), global.evictions());
    assert_eq!(scoped.evicted_bytes(), global.evicted_bytes());
    assert_eq!(scoped.peak_resident_bytes(), global.peak_resident_bytes());
    assert_eq!(
        scoped.layer_peak_resident_bytes(),
        &[(0, 4)].into_iter().collect()
    );
}

#[test]
fn belady_sees_each_layers_own_schedule_under_per_layer_quotas() {
    // Layer 0 (quota 4): [0], [1], [2], [0]
    //   at [2]: {0 next e5, 1 never} -> evict 1, so the final [0] hits.
    // Layer 1 (quota 2): [0], [1], [0]
    //   one slot forces an eviction either way; the final [0] is a reload.
    let manifest = manifest_layers(&[(0, 0, 2), (0, 1, 2), (0, 2, 2), (1, 0, 2), (1, 1, 2)]);
    let events = [
        ev_on(0, vec![0]),
        ev_on(1, vec![0]),
        ev_on(0, vec![1]),
        ev_on(0, vec![2]),
        ev_on(1, vec![1]),
        ev_on(0, vec![0]),
        ev_on(1, vec![0]),
    ];

    let metrics = replay(
        &manifest,
        &events,
        Policy::Belady,
        &per_layer(6, &[(0, 4), (1, 2)]),
    )
    .unwrap();
    assert_eq!(metrics.events(), 7);
    assert_eq!(metrics.object_loads(), 6);
    assert_eq!(metrics.byte_loads(), 12);
    assert_eq!(metrics.object_hits(), 1);
    assert_eq!(metrics.byte_hits(), 2);
    assert_eq!(metrics.object_reloads(), 1);
    assert_eq!(metrics.byte_reloads(), 2);
    assert_eq!(metrics.evictions(), 3);
    assert_eq!(metrics.evicted_bytes(), 6);
    assert_eq!(metrics.peak_resident_bytes(), 6);
    assert_eq!(
        metrics.layer_peak_resident_bytes(),
        &[(0, 4), (1, 2)].into_iter().collect()
    );
}

#[test]
fn belady_replay_is_deterministic() {
    let manifest = manifest_layers(&[(0, 0, 2), (0, 1, 2), (0, 2, 2), (1, 0, 2), (1, 1, 2)]);
    let events = [
        ev_on(0, vec![0, 1]),
        ev_on(1, vec![0]),
        ev_on(0, vec![2]),
        ev_on(1, vec![1]),
        ev_on(0, vec![0, 1]),
    ];
    let scope = per_layer(6, &[(0, 4), (1, 2)]);

    let first = replay(&manifest, &events, Policy::Belady, &scope).unwrap();
    let second = replay(&manifest, &events, Policy::Belady, &scope).unwrap();
    assert_eq!(first, second);
}

#[test]
fn an_expert_id_shared_across_layers_never_leaks_into_a_schedule() {
    // Layer 1 reuses expert id 2. A schedule keyed by expert id alone would
    // import layer 1's activation as layer-0 expert 2's next use, evict
    // expert 0 instead of the never-reused expert 2 at the fourth event,
    // and finish with 5 loads, no hit, and 2 evictions.
    let manifest = manifest_layers(&[(0, 0, 1), (0, 1, 1), (0, 2, 1), (1, 2, 1)]);
    let events = [
        ev_on(0, vec![2]),
        ev_on(1, vec![2]),
        ev_on(0, vec![0]),
        ev_on(0, vec![1]),
        ev_on(0, vec![0]),
    ];

    let metrics = replay(
        &manifest,
        &events,
        Policy::Belady,
        &per_layer(4, &[(0, 2), (1, 2)]),
    )
    .unwrap();
    assert_eq!(metrics.object_loads(), 4);
    assert_eq!(metrics.object_hits(), 1);
    assert_eq!(metrics.evictions(), 1);
}

#[test]
fn a_genuine_belady_tie_evicts_the_lowest_key_itself() {
    // Aggregate metrics reconverge after a tie (the tied pair shares its
    // next event), so the victim's identity is pinned directly on the
    // cache: equal next uses lose by lowest key, and "never activated
    // again" loses to any scheduled use.
    let mut cache = ResidentCache::new(4);
    cache.admit(ExpertKey::new(0, 0), 2);
    cache.admit(ExpertKey::new(0, 1), 2);
    cache.record_next_use(ExpertKey::new(0, 0), Some(7));
    cache.record_next_use(ExpertKey::new(0, 1), Some(7));
    assert_eq!(cache.evict_one(Policy::Belady, &BTreeSet::new()), Some(2));
    assert!(!cache.contains(ExpertKey::new(0, 0)));
    assert!(cache.contains(ExpertKey::new(0, 1)));

    cache.admit(ExpertKey::new(0, 0), 2);
    assert_eq!(cache.evict_one(Policy::Belady, &BTreeSet::new()), Some(2));
    assert!(!cache.contains(ExpertKey::new(0, 0)));
    assert!(cache.contains(ExpertKey::new(0, 1)));
}
