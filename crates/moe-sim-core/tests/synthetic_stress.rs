//! ~100k-event stress regression over the synthetic generators.
//!
//! Everything is generated in memory: no large fixture enters the
//! repository, and the same traces are regenerable from the CLI with
//! `trace generate`. Gate coverage for slice 1D at scale: repeated
//! generation and replay are identical, capacity and pinning invariants
//! hold, and object and byte metrics stay separate.
//!
//! Each pattern replays under one deliberately chosen policy instead of the
//! full matrix: a full 100k matrix costs ~16 s in debug builds, and the
//! policy × pattern combinations are already pinned at small scale by the
//! unit tests and the exhaustive oracle.

#![expect(
    unused_crate_dependencies,
    reason = "integration-test targets receive every package dependency; this test only drives the moe_sim_core library"
)]
#![expect(
    clippy::unwrap_used,
    reason = "tests build valid patterns directly; direct unwraps keep failure diagnostics next to the pattern parameters"
)]

use moe_sim_core::{
    CacheScope, ModelManifest, Policy, ReplayMetrics, SyntheticPattern, replay, synthetic,
};

const EVENTS: u64 = 100_000;

/// Generates `pattern`, validates capacity, and replays once.
fn replay_once(pattern: &SyntheticPattern, policy: Policy, budget_bytes: u64) -> ReplayMetrics {
    let case = synthetic::generate(pattern).unwrap();
    assert_eq!(case.events.len(), 100_000, "{pattern:?}");
    let manifest = ModelManifest::try_from_entries(case.manifest_entries.iter().copied()).unwrap();
    manifest
        .validate_global_capacity(budget_bytes, &case.events)
        .unwrap();
    replay(
        &manifest,
        &case.events,
        policy,
        &CacheScope::Global { budget_bytes },
    )
    .unwrap()
}

#[test]
fn every_pattern_holds_the_capacity_invariant_at_scale() {
    // One adversarially chosen policy per pattern: recency pressure for the
    // scans that defeat it, frequency pressure for the shifting hot set,
    // and the offline reference for the seeded random trace.
    let cases: [(SyntheticPattern, Policy, u64); 6] = [
        (
            SyntheticPattern::Repetition {
                experts: 8,
                active_per_event: 4,
                events: EVENTS,
            },
            Policy::Lfu,
            8,
        ),
        (
            SyntheticPattern::Cyclic {
                experts: 64,
                events: EVENTS,
            },
            Policy::Lru,
            32,
        ),
        (
            SyntheticPattern::Random {
                experts: 64,
                active_per_event: 4,
                events: EVENTS,
                seed: 42,
            },
            Policy::Belady,
            32,
        ),
        (
            SyntheticPattern::HotsetShift {
                experts: 64,
                hot: 8,
                period: 500,
                events: EVENTS,
            },
            Policy::Lfu,
            16,
        ),
        (
            SyntheticPattern::VariableSizes {
                experts: 64,
                events: EVENTS,
            },
            Policy::Lru,
            256,
        ),
        (
            SyntheticPattern::AdversarialLru {
                experts: 64,
                events: EVENTS,
            },
            Policy::Lru,
            8,
        ),
    ];

    for (pattern, policy, budget_bytes) in cases {
        let metrics = replay_once(&pattern, policy, budget_bytes);
        assert_eq!(metrics.events(), EVENTS, "{pattern:?} under {policy}");
        assert!(
            metrics.peak_resident_bytes() <= budget_bytes,
            "{pattern:?} under {policy}: peak {} exceeds budget {budget_bytes}",
            metrics.peak_resident_bytes()
        );
        if matches!(pattern, SyntheticPattern::VariableSizes { .. }) {
            // Linearly growing sizes: byte counters must not be a disguised
            // copy of the object counters.
            assert!(metrics.byte_loads() > metrics.object_loads(), "{pattern:?}");
        }
    }
}

#[test]
fn the_seeded_random_pattern_is_deterministic_at_scale() {
    let pattern = SyntheticPattern::Random {
        experts: 64,
        active_per_event: 4,
        events: EVENTS,
        seed: 42,
    };
    assert_eq!(
        synthetic::generate(&pattern).unwrap(),
        synthetic::generate(&pattern).unwrap()
    );
    assert_eq!(
        replay_once(&pattern, Policy::Lru, 32),
        replay_once(&pattern, Policy::Lru, 32)
    );
}

#[test]
fn the_offline_reference_refuses_the_variable_size_pattern() {
    let case = synthetic::generate(&SyntheticPattern::VariableSizes {
        experts: 8,
        events: 16,
    })
    .unwrap();
    let manifest = ModelManifest::try_from_entries(case.manifest_entries.iter().copied()).unwrap();
    let result = replay(
        &manifest,
        &case.events,
        Policy::Belady,
        &CacheScope::Global { budget_bytes: 16 },
    );
    assert!(result.is_err());
}
