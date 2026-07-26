#![expect(
    clippy::unwrap_used,
    reason = "tests exercise the fallible generators directly; direct unwraps keep failure diagnostics next to the expected pattern data"
)]

use super::*;

/// The single-expert id of each generated event, asserting shape on the way.
fn expert_sequence(case: &SyntheticCase) -> Vec<u32> {
    case.events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            assert_eq!(event.request_id(), 1);
            assert_eq!(event.phase(), Phase::Decode);
            assert_eq!(event.step_id(), index as u64);
            assert_eq!(event.token_position(), index as u64);
            assert_eq!(event.layer_id(), 0);
            assert_eq!(event.expert_ids().len(), 1, "event {index}");
            event.expert_ids()[0]
        })
        .collect()
}

#[test]
fn cyclic_cycles_round_robin() {
    let case = generate(&SyntheticPattern::Cyclic {
        experts: 3,
        events: 5,
    })
    .unwrap();
    assert_eq!(expert_sequence(&case), [0, 1, 2, 0, 1]);
    assert_eq!(case.manifest_entries.len(), 3);
    assert!(case.manifest_entries.iter().all(|e| e.size_bytes == 1));
}

#[test]
fn repetition_repeats_one_atomic_set() {
    let case = generate(&SyntheticPattern::Repetition {
        experts: 4,
        active_per_event: 2,
        events: 3,
    })
    .unwrap();
    assert_eq!(case.events.len(), 3);
    for event in &case.events {
        assert_eq!(event.expert_ids(), [0, 1]);
    }
    assert_eq!(case.manifest_entries.len(), 4);
}

#[test]
fn hotset_shift_advances_and_restarts_the_window() {
    // Window width 2 over 6 experts, shifting every 3 events:
    //   offset 0: 0, 1, 0 | offset 2: 2, 3, 2 | offset 4: 4, 5
    let case = generate(&SyntheticPattern::HotsetShift {
        experts: 6,
        hot: 2,
        period: 3,
        events: 8,
    })
    .unwrap();
    assert_eq!(expert_sequence(&case), [0, 1, 0, 2, 3, 2, 4, 5]);
}

#[test]
fn hotset_window_wraps_around_the_expert_space() {
    // Width 2 over 3 experts shifting every 2 events: offsets 0, 2, 1, 0.
    let case = generate(&SyntheticPattern::HotsetShift {
        experts: 3,
        hot: 2,
        period: 2,
        events: 8,
    })
    .unwrap();
    assert_eq!(expert_sequence(&case), [0, 1, 2, 0, 1, 2, 0, 1]);
}

#[test]
fn variable_sizes_grow_linearly_over_a_cyclic_scan() {
    let case = generate(&SyntheticPattern::VariableSizes {
        experts: 3,
        events: 4,
    })
    .unwrap();
    assert_eq!(expert_sequence(&case), [0, 1, 2, 0]);
    let sizes: Vec<u64> = case
        .manifest_entries
        .iter()
        .map(|entry| entry.size_bytes)
        .collect();
    assert_eq!(sizes, [1, 2, 3]);
}

#[test]
fn adversarial_lru_alternates_the_hot_expert_with_a_cold_scan() {
    let case = generate(&SyntheticPattern::AdversarialLru {
        experts: 4,
        events: 9,
    })
    .unwrap();
    assert_eq!(expert_sequence(&case), [0, 1, 0, 2, 0, 3, 0, 1, 0]);
}

#[test]
fn random_draws_distinct_members_and_reproduces_from_its_seed() {
    let pattern = SyntheticPattern::Random {
        experts: 8,
        active_per_event: 3,
        events: 16,
        seed: 42,
    };
    let case = generate(&pattern).unwrap();
    assert_eq!(case.events.len(), 16);
    for event in &case.events {
        assert_eq!(event.expert_ids().len(), 3);
        assert!(event.expert_ids().iter().all(|&id| id < 8));
    }

    let replayed = generate(&pattern).unwrap();
    assert_eq!(case, replayed);

    let other_seed = generate(&SyntheticPattern::Random {
        experts: 8,
        active_per_event: 3,
        events: 16,
        seed: 43,
    })
    .unwrap();
    assert_ne!(case.events, other_seed.events);
}

#[test]
fn every_pattern_is_deterministic_and_zero_events_are_legal() {
    let patterns = [
        SyntheticPattern::Repetition {
            experts: 4,
            active_per_event: 2,
            events: 10,
        },
        SyntheticPattern::Cyclic {
            experts: 4,
            events: 10,
        },
        SyntheticPattern::Random {
            experts: 4,
            active_per_event: 2,
            events: 10,
            seed: 7,
        },
        SyntheticPattern::HotsetShift {
            experts: 4,
            hot: 2,
            period: 3,
            events: 10,
        },
        SyntheticPattern::VariableSizes {
            experts: 4,
            events: 10,
        },
        SyntheticPattern::AdversarialLru {
            experts: 4,
            events: 10,
        },
    ];
    for pattern in patterns {
        assert_eq!(generate(&pattern).unwrap(), generate(&pattern).unwrap());
    }

    let empty = generate(&SyntheticPattern::Cyclic {
        experts: 2,
        events: 0,
    })
    .unwrap();
    assert!(empty.events.is_empty());
    assert_eq!(empty.manifest_entries.len(), 2);
}

#[test]
fn impossible_parameters_are_rejected_with_the_named_error() {
    assert_eq!(
        generate(&SyntheticPattern::Cyclic {
            experts: 0,
            events: 1,
        })
        .unwrap_err(),
        SyntheticError::ZeroExperts
    );
    assert_eq!(
        generate(&SyntheticPattern::Repetition {
            experts: 2,
            active_per_event: 3,
            events: 1,
        })
        .unwrap_err(),
        SyntheticError::ActiveSetSize {
            active_per_event: 3,
            experts: 2,
        }
    );
    assert_eq!(
        generate(&SyntheticPattern::Random {
            experts: 2,
            active_per_event: 0,
            events: 1,
            seed: 0,
        })
        .unwrap_err(),
        SyntheticError::ActiveSetSize {
            active_per_event: 0,
            experts: 2,
        }
    );
    assert_eq!(
        generate(&SyntheticPattern::HotsetShift {
            experts: 2,
            hot: 3,
            period: 1,
            events: 1,
        })
        .unwrap_err(),
        SyntheticError::HotWindow { hot: 3, experts: 2 }
    );
    assert_eq!(
        generate(&SyntheticPattern::HotsetShift {
            experts: 2,
            hot: 1,
            period: 0,
            events: 1,
        })
        .unwrap_err(),
        SyntheticError::ZeroPeriod
    );
    assert_eq!(
        generate(&SyntheticPattern::AdversarialLru {
            experts: 1,
            events: 1,
        })
        .unwrap_err(),
        SyntheticError::AdversarialLruNeedsTwoExperts { experts: 1 }
    );
}

#[test]
fn random_seed_42_produces_the_pinned_sequence() {
    // The drawn sequence is part of the reproducibility contract: a recorded
    // seed must reproduce its trace across releases and platforms, so a
    // silent change to the mixer or the draw order is a provenance break,
    // not an internal detail.
    let case = generate(&SyntheticPattern::Random {
        experts: 8,
        active_per_event: 3,
        events: 6,
        seed: 42,
    })
    .unwrap();
    let drawn: Vec<&[u32]> = case.events.iter().map(Event::expert_ids).collect();
    assert_eq!(
        drawn,
        [
            [5, 6, 2],
            [4, 7, 2],
            [0, 6, 3],
            [1, 0, 6],
            [3, 4, 5],
            [5, 6, 0],
        ]
    );
}
