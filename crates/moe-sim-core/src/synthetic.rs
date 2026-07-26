//! Deterministic synthetic activation patterns.
//!
//! Every generator is a pure function of its declared parameters: equal
//! parameters produce equal traces, byte for byte, on every platform. Only
//! [`SyntheticPattern::Random`] consumes entropy, and that entropy is one
//! explicit seed spread by a small documented generator, so a recorded seed
//! reproduces the trace exactly.
//!
//! Generated data is synthetic by construction and labeled as such: every
//! pattern emits a single-layer trace on layer 0 with `request_id` 1, phase
//! `decode`, and `step_id` and `token_position` equal to the event index.
//! The structure is canonical and validates like a hand-written trace; it
//! does not pretend to model a real serving schedule.

use crate::manifest::{ExpertKey, ExpertSizeEntry};
use crate::trace::{Event, EventError, EventParts, Phase};

/// One synthetic trace family and its parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntheticPattern {
    /// The same atomic set `{0 .. active_per_event}` on every event: an
    /// all-hit baseline once the set is resident.
    Repetition {
        /// Experts declared in the manifest.
        experts: u32,
        /// Members of the repeated atomic set, `1..=experts`.
        active_per_event: u32,
        /// Events to generate.
        events: u64,
    },
    /// Single-expert events cycling round-robin over every expert: the
    /// classic sequential scan that thrashes LRU when the budget is short.
    Cyclic {
        /// Experts declared in the manifest.
        experts: u32,
        /// Events to generate.
        events: u64,
    },
    /// Each event activates `active_per_event` distinct experts drawn
    /// uniformly by the seeded generator.
    Random {
        /// Experts declared in the manifest.
        experts: u32,
        /// Distinct members drawn for each event, `1..=experts`.
        active_per_event: u32,
        /// Events to generate.
        events: u64,
        /// Seed spread by `SplitMix64`; equal seeds reproduce the trace.
        seed: u64,
    },
    /// Single-expert events cycling inside a hot window of `hot` experts;
    /// after every `period` events the window advances by `hot` positions
    /// (wrapping) and restarts at its base.
    HotsetShift {
        /// Experts declared in the manifest.
        experts: u32,
        /// Width of the hot window, `1..=experts`.
        hot: u32,
        /// Events between two window shifts, at least 1.
        period: u64,
        /// Events to generate.
        events: u64,
    },
    /// The cyclic scan over experts of linearly growing size (expert `e`
    /// stores `e + 1` bytes), so object and byte metrics separate.
    VariableSizes {
        /// Experts declared in the manifest.
        experts: u32,
        /// Events to generate.
        events: u64,
    },
    /// One hot expert (id 0) revisited between the steps of a cold scan
    /// over the remaining experts: recency keeps evicting what frequency
    /// would keep.
    AdversarialLru {
        /// Experts declared in the manifest, at least 2.
        experts: u32,
        /// Events to generate.
        events: u64,
    },
}

/// Errors rejecting impossible synthetic parameters.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyntheticError {
    /// Every pattern needs at least one expert.
    #[error("a synthetic pattern needs at least one expert")]
    ZeroExperts,
    /// The atomic set must have between 1 member and the expert count.
    #[error(
        "active_per_event must be between 1 and the expert count: got {active_per_event} of {experts}"
    )]
    ActiveSetSize {
        /// Requested members per event.
        active_per_event: u32,
        /// Declared expert count.
        experts: u32,
    },
    /// The hot window must have between 1 expert and the expert count.
    #[error("the hot window must be between 1 and the expert count: got {hot} of {experts}")]
    HotWindow {
        /// Requested hot-window width.
        hot: u32,
        /// Declared expert count.
        experts: u32,
    },
    /// The hot window shift period must be at least one event.
    #[error("the hot window shift period must be at least 1 event")]
    ZeroPeriod,
    /// The adversarial scan needs a hot expert plus at least one cold one.
    #[error("the adversarial-lru pattern needs at least two experts: got {experts}")]
    AdversarialLruNeedsTwoExperts {
        /// Declared expert count.
        experts: u32,
    },
    /// Defensive: generated parts were rejected by canonical validation.
    ///
    /// Generators only emit unique expert ids, so this indicates a bug in
    /// the generator itself rather than in the parameters.
    #[error("a generated event was rejected by canonical validation: {source}")]
    Event {
        /// Underlying canonical validation error.
        source: EventError,
    },
}

/// A generated manifest and trace pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntheticCase {
    /// One size entry per declared expert, ascending by key.
    pub manifest_entries: Vec<ExpertSizeEntry>,
    /// Generated events in replay order.
    pub events: Vec<Event>,
}

/// Generates the manifest entries and events for `pattern`.
///
/// # Errors
///
/// Returns the [`SyntheticError`] naming the impossible parameter: zero
/// experts, an atomic set outside `1..=experts`, a hot window outside
/// `1..=experts`, a zero shift period, or fewer than two experts for the
/// adversarial scan.
pub fn generate(pattern: &SyntheticPattern) -> Result<SyntheticCase, SyntheticError> {
    match *pattern {
        SyntheticPattern::Repetition {
            experts,
            active_per_event,
            events,
        } => {
            ensure_experts(experts)?;
            ensure_active_set(active_per_event, experts)?;
            let ids: Vec<u32> = (0..active_per_event).collect();
            let events = (0..events)
                .map(|index| event_at(index, ids.clone()))
                .collect::<Result<_, _>>()?;
            Ok(SyntheticCase {
                manifest_entries: uniform_entries(experts),
                events,
            })
        }
        SyntheticPattern::Cyclic { experts, events } => Ok(SyntheticCase {
            manifest_entries: uniform_entries(experts),
            events: cyclic_events(experts, events)?,
        }),
        SyntheticPattern::Random {
            experts,
            active_per_event,
            events,
            seed,
        } => {
            ensure_experts(experts)?;
            ensure_active_set(active_per_event, experts)?;
            Ok(SyntheticCase {
                manifest_entries: uniform_entries(experts),
                events: random_events(experts, active_per_event, events, seed)?,
            })
        }
        SyntheticPattern::HotsetShift {
            experts,
            hot,
            period,
            events,
        } => {
            ensure_experts(experts)?;
            if hot == 0 || hot > experts {
                return Err(SyntheticError::HotWindow { hot, experts });
            }
            if period == 0 {
                return Err(SyntheticError::ZeroPeriod);
            }
            Ok(SyntheticCase {
                manifest_entries: uniform_entries(experts),
                events: hotset_events(experts, hot, period, events)?,
            })
        }
        SyntheticPattern::VariableSizes { experts, events } => Ok(SyntheticCase {
            manifest_entries: linear_entries(experts),
            events: cyclic_events(experts, events)?,
        }),
        SyntheticPattern::AdversarialLru { experts, events } => {
            if experts < 2 {
                return Err(SyntheticError::AdversarialLruNeedsTwoExperts { experts });
            }
            let mut cold = 1u32;
            let mut out = Vec::new();
            for index in 0..events {
                let expert = if index % 2 == 0 {
                    0
                } else {
                    let picked = cold;
                    cold += 1;
                    if cold == experts {
                        cold = 1;
                    }
                    picked
                };
                out.push(event_at(index, vec![expert])?);
            }
            Ok(SyntheticCase {
                manifest_entries: uniform_entries(experts),
                events: out,
            })
        }
    }
}

/// Rejects a pattern over no experts.
fn ensure_experts(experts: u32) -> Result<(), SyntheticError> {
    if experts == 0 {
        return Err(SyntheticError::ZeroExperts);
    }
    Ok(())
}

/// Rejects an atomic-set width outside `1..=experts`.
fn ensure_active_set(active_per_event: u32, experts: u32) -> Result<(), SyntheticError> {
    if active_per_event == 0 || active_per_event > experts {
        return Err(SyntheticError::ActiveSetSize {
            active_per_event,
            experts,
        });
    }
    Ok(())
}

/// One synthetic event at `index` activating `expert_ids` on layer 0.
fn event_at(index: u64, expert_ids: Vec<u32>) -> Result<Event, SyntheticError> {
    Event::new(EventParts {
        request_id: 1,
        phase: Phase::Decode,
        step_id: index,
        token_position: index,
        layer_id: 0,
        expert_ids,
    })
    .map_err(|source| SyntheticError::Event { source })
}

/// Seeded uniform draws of `active_per_event` distinct experts per event,
/// by partial Fisher-Yates over the expert pool.
fn random_events(
    experts: u32,
    active_per_event: u32,
    events: u64,
    seed: u64,
) -> Result<Vec<Event>, SyntheticError> {
    let mut rng = SplitMix64::new(seed);
    let mut pool: Vec<u32> = (0..experts).collect();
    let active = usize_of(active_per_event);
    let mut out = Vec::new();
    for index in 0..events {
        let mut ids = Vec::with_capacity(active);
        for slot in 0..active {
            let pick = slot + pool_slot(rng.next(), pool.len() - slot);
            pool.swap(slot, pick);
            ids.push(pool[slot]);
        }
        out.push(event_at(index, ids)?);
    }
    Ok(out)
}

/// Single-expert events inside a hot window that advances by `hot` and
/// restarts at its base every `period` events.
fn hotset_events(
    experts: u32,
    hot: u32,
    period: u64,
    events: u64,
) -> Result<Vec<Event>, SyntheticError> {
    let mut offset = 0u32;
    let mut within = 0u32;
    let mut into_period = 0u64;
    let mut out = Vec::new();
    for index in 0..events {
        out.push(event_at(index, vec![wrap_add(offset, within, experts)])?);
        within = wrap_add(within, 1, hot);
        into_period += 1;
        if into_period == period {
            into_period = 0;
            offset = wrap_add(offset, hot, experts);
            within = 0;
        }
    }
    Ok(out)
}

/// Round-robin single-expert events over `experts`.
fn cyclic_events(experts: u32, events: u64) -> Result<Vec<Event>, SyntheticError> {
    ensure_experts(experts)?;
    let mut cursor = 0u32;
    let mut out = Vec::new();
    for index in 0..events {
        out.push(event_at(index, vec![cursor])?);
        cursor = wrap_add(cursor, 1, experts);
    }
    Ok(out)
}

/// One 1-byte entry per expert on layer 0.
fn uniform_entries(experts: u32) -> Vec<ExpertSizeEntry> {
    (0..experts)
        .map(|expert_id| ExpertSizeEntry {
            key: ExpertKey::new(0, expert_id),
            size_bytes: 1,
        })
        .collect()
}

/// Linearly growing entries: expert `e` stores `e + 1` bytes.
fn linear_entries(experts: u32) -> Vec<ExpertSizeEntry> {
    (0..experts)
        .map(|expert_id| ExpertSizeEntry {
            key: ExpertKey::new(0, expert_id),
            size_bytes: u64::from(expert_id) + 1,
        })
        .collect()
}

/// `(a + b) % modulus` computed in `u64` space.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the remainder is strictly below a u32 modulus, so it always fits u32"
)]
fn wrap_add(a: u32, b: u32, modulus: u32) -> u32 {
    ((u64::from(a) + u64::from(b)) % u64::from(modulus)) as u32
}

/// Index into a pool of `len` slots by `value` modulo `len`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the remainder is strictly below the pool length, which is a usize"
)]
fn pool_slot(value: u64, len: usize) -> usize {
    (value % len as u64) as usize
}

/// Widens a `u32` count to `usize`; supported platforms have a `usize` of
/// at least 32 bits.
fn usize_of(value: u32) -> usize {
    value as usize
}

/// `SplitMix64`: a tiny, well-known, platform-independent mixer.
///
/// Deliberately implemented in-repo instead of adding a dependency: the only
/// requirement is that one recorded seed reproduces one trace forever, not
/// statistical or cryptographic strength.
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }
}

#[cfg(test)]
mod tests;
