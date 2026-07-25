//! Deterministic replay of canonical events and byte-accurate accounting.
//!
//! Events are replayed strictly in the order the caller supplies them, which
//! is file order. Metadata such as `step_id` or `token_position` never
//! reorders anything; it validates source structure only.
//!
//! Each event's unique `expert_ids` form one atomic active set: every member
//! is made resident together, stays pinned for the whole event, and is
//! released before the next event begins. No event is ever partially admitted,
//! and no member of the current active set can be evicted to make room for
//! another member of the same set.
//!
//! Resident bytes never exceed the supplied budget. When an atomic active set
//! cannot fit even after evicting everything unpinned, replay fails instead of
//! bypassing the cache or admitting the event in part.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::manifest::{ExpertKey, ManifestError, ModelManifest};
use crate::trace::Event;

/// Cache policy applied during replay.
///
/// A policy decides only which resident object is evicted next. It is
/// independent of the budget it operates under, so the same policy can later
/// be applied within a different cache scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Policy {
    /// Retain nothing between events.
    ///
    /// The baseline every caching policy is measured against: every activation
    /// is a load, and no object is ever reused across events.
    NoCache,
    /// Evict the least recently used unpinned object.
    Lru,
    /// Evict the least frequently used unpinned object.
    ///
    /// Ties are broken by least recent use, which keeps the choice
    /// deterministic without biasing eviction toward low expert identifiers.
    /// A frequency count belongs to a resident entry and restarts when an
    /// object is admitted again, so a once-hot object does not stay immortal
    /// after eviction.
    Lfu,
}

impl fmt::Display for Policy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::NoCache => "no-cache",
            Self::Lru => "lru",
            Self::Lfu => "lfu",
        };
        f.write_str(name)
    }
}

/// Names the cumulative counter that overflowed during replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReplayCounter {
    /// Count of replayed events.
    Events,
    /// Count of expert objects loaded.
    ObjectLoads,
    /// Cumulative bytes loaded.
    ByteLoads,
    /// Count of activations served from residency.
    ObjectHits,
    /// Cumulative bytes served from residency.
    ByteHits,
    /// Count of objects loaded again after ceasing to be resident.
    ObjectReloads,
    /// Cumulative bytes loaded again after ceasing to be resident.
    ByteReloads,
    /// Count of objects evicted to reclaim capacity.
    Evictions,
    /// Cumulative bytes evicted to reclaim capacity.
    EvictedBytes,
}

impl fmt::Display for ReplayCounter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Events => "events",
            Self::ObjectLoads => "object_loads",
            Self::ByteLoads => "byte_loads",
            Self::ObjectHits => "object_hits",
            Self::ByteHits => "byte_hits",
            Self::ObjectReloads => "object_reloads",
            Self::ByteReloads => "byte_reloads",
            Self::Evictions => "evictions",
            Self::EvictedBytes => "evicted_bytes",
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
    /// An atomic active set does not fit the budget even with every unpinned
    /// object evicted.
    ///
    /// Callers are expected to run
    /// [`ModelManifest::validate_global_capacity`] first, which rejects this
    /// configuration before replay starts. Replay repeats the check rather
    /// than trusting it, because silently bypassing the cache or admitting
    /// part of an active set would produce a report describing a run that
    /// cannot happen.
    #[error(
        "active set does not fit the budget: event {event_index} request {request_id} layer {layer_id} needs {active_set_bytes} bytes, budget is {budget_bytes} bytes"
    )]
    ActiveSetExceedsCapacity {
        /// Zero-based position of the failing event in the supplied order.
        event_index: usize,
        /// Request of the failing event.
        request_id: u64,
        /// Layer of the failing event.
        layer_id: u32,
        /// Total stored size of the atomic active set in bytes.
        active_set_bytes: u64,
        /// Capacity budget in bytes.
        budget_bytes: u64,
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
    object_reloads: u64,
    byte_reloads: u64,
    evictions: u64,
    evicted_bytes: u64,
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

    /// Activations served by an already-resident object.
    #[must_use]
    pub fn object_hits(&self) -> u64 {
        self.object_hits
    }

    /// Bytes served by an already-resident object.
    #[must_use]
    pub fn byte_hits(&self) -> u64 {
        self.byte_hits
    }

    /// Objects loaded again after ceasing to be resident: the churn metric.
    ///
    /// This is the share of [`Self::object_loads`] that is rework rather than
    /// unavoidable cold misses, so `object_loads` equals cold loads plus
    /// reloads. Neither number is derivable from the hit and eviction
    /// counters alone.
    ///
    /// Churn counts rework, not its cause. A retaining policy loses residency
    /// by eviction, but [`Policy::NoCache`] loses it by releasing the active
    /// set, so the baseline reports reloads while [`Self::evictions`] stays
    /// zero. That pairing is intended, not a broken report.
    #[must_use]
    pub fn object_reloads(&self) -> u64 {
        self.object_reloads
    }

    /// Bytes loaded again after ceasing to be resident: the churn metric in
    /// bytes.
    #[must_use]
    pub fn byte_reloads(&self) -> u64 {
        self.byte_reloads
    }

    /// Resident objects removed to reclaim capacity.
    ///
    /// Releasing a pinned active set after its event completes is **not** an
    /// eviction: eviction is the capacity-driven removal of an object the
    /// policy chose to retain. A policy that retains nothing therefore evicts
    /// nothing, which keeps the no-cache baseline comparable with the caching
    /// policies measured against it.
    #[must_use]
    pub fn evictions(&self) -> u64 {
        self.evictions
    }

    /// Bytes removed to reclaim capacity.
    #[must_use]
    pub fn evicted_bytes(&self) -> u64 {
        self.evicted_bytes
    }

    /// Largest number of bytes resident at any instant during replay.
    ///
    /// Sampled while each atomic active set is pinned, which is when residency
    /// is at its highest for that event.
    #[must_use]
    pub fn peak_resident_bytes(&self) -> u64 {
        self.peak_resident_bytes
    }
}

/// Replays `events` against `manifest` under `policy` and one global budget.
///
/// Resident bytes never exceed `global_budget_bytes`. Each event's atomic
/// active set is made fully resident before the event is accounted, and no
/// member of that set can be evicted while it is pinned.
///
/// [`Policy::NoCache`] retains nothing between events, so it never hits and
/// never evicts; the budget still bounds each individual active set.
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
/// Returns [`ReplayError::ActiveSetExceedsCapacity`] when an atomic active set
/// cannot fit the budget even with every unpinned object evicted.
/// Returns [`ReplayError::CounterOverflow`] when a cumulative counter exceeds
/// `u64`.
pub fn replay<'a>(
    manifest: &ModelManifest,
    events: impl IntoIterator<Item = &'a Event>,
    policy: Policy,
    global_budget_bytes: u64,
) -> Result<ReplayMetrics, ReplayError> {
    let mut metrics = ReplayMetrics::default();
    let mut cache = ResidentCache::new(global_budget_bytes);
    let mut ever_loaded: BTreeSet<ExpertKey> = BTreeSet::new();

    for (event_index, event) in events.into_iter().enumerate() {
        let layer_id = event.layer_id();
        let active_set_bytes =
            manifest
                .active_set_bytes(event)
                .map_err(|source| ReplayError::ActiveSetBytes {
                    event_index,
                    request_id: event.request_id(),
                    layer_id,
                    source,
                })?;

        metrics.events = bump(metrics.events, 1, ReplayCounter::Events, event_index)?;

        // The whole active set is pinned for this event, so nothing in it can
        // be chosen as an eviction victim while the set is being assembled.
        let mut pinned: BTreeSet<ExpertKey> = BTreeSet::new();
        let mut missing: Vec<(ExpertKey, u64)> = Vec::new();
        let mut required_bytes: u64 = 0;

        for &expert_id in event.expert_ids() {
            let key = ExpertKey::new(layer_id, expert_id);
            let size_bytes =
                manifest
                    .size_bytes(key)
                    .map_err(|source| ReplayError::ActiveSetBytes {
                        event_index,
                        request_id: event.request_id(),
                        layer_id,
                        source,
                    })?;
            pinned.insert(key);

            if cache.contains(key) {
                metrics.object_hits = bump(
                    metrics.object_hits,
                    1,
                    ReplayCounter::ObjectHits,
                    event_index,
                )?;
                metrics.byte_hits = bump(
                    metrics.byte_hits,
                    size_bytes,
                    ReplayCounter::ByteHits,
                    event_index,
                )?;
            } else {
                required_bytes = required_bytes.saturating_add(size_bytes);
                missing.push((key, size_bytes));
            }
        }

        if !evict_until_fits(
            &mut cache,
            &mut metrics,
            policy,
            &pinned,
            required_bytes,
            event_index,
        )? {
            return Err(ReplayError::ActiveSetExceedsCapacity {
                event_index,
                request_id: event.request_id(),
                layer_id,
                active_set_bytes,
                budget_bytes: global_budget_bytes,
            });
        }

        admit_missing(
            &mut cache,
            &mut metrics,
            &mut ever_loaded,
            &missing,
            event_index,
        )?;

        cache.record_access(&pinned);

        if policy == Policy::NoCache {
            // Release, not eviction: nothing was retained by choice.
            cache.clear();
        }
    }

    metrics.peak_resident_bytes = cache.peak_resident_bytes();

    Ok(metrics)
}

/// Evicts unpinned objects until `required_bytes` fit, counting each eviction.
///
/// Returns `false` when every resident object is pinned and the requirement is
/// still unmet, which means the atomic active set itself cannot fit.
///
/// The loop always terminates: only the current active set is pinned, so
/// eviction candidates run out only once the cache holds nothing else.
fn evict_until_fits(
    cache: &mut ResidentCache,
    metrics: &mut ReplayMetrics,
    policy: Policy,
    pinned: &BTreeSet<ExpertKey>,
    required_bytes: u64,
    event_index: usize,
) -> Result<bool, ReplayError> {
    while cache.free_bytes() < required_bytes {
        let Some(evicted_bytes) = cache.evict_one(policy, pinned) else {
            return Ok(false);
        };
        metrics.evictions = bump(metrics.evictions, 1, ReplayCounter::Evictions, event_index)?;
        metrics.evicted_bytes = bump(
            metrics.evicted_bytes,
            evicted_bytes,
            ReplayCounter::EvictedBytes,
            event_index,
        )?;
    }
    Ok(true)
}

/// Admits every missing member of an active set, counting loads and splitting
/// rework from cold misses.
fn admit_missing(
    cache: &mut ResidentCache,
    metrics: &mut ReplayMetrics,
    ever_loaded: &mut BTreeSet<ExpertKey>,
    missing: &[(ExpertKey, u64)],
    event_index: usize,
) -> Result<(), ReplayError> {
    for &(key, size_bytes) in missing {
        cache.admit(key, size_bytes);
        metrics.object_loads = bump(
            metrics.object_loads,
            1,
            ReplayCounter::ObjectLoads,
            event_index,
        )?;
        metrics.byte_loads = bump(
            metrics.byte_loads,
            size_bytes,
            ReplayCounter::ByteLoads,
            event_index,
        )?;

        // `insert` reports whether the key is new, so a repeat load is rework
        // rather than a cold miss. The cause may be eviction or, under
        // no-cache, an ordinary release.
        if !ever_loaded.insert(key) {
            metrics.object_reloads = bump(
                metrics.object_reloads,
                1,
                ReplayCounter::ObjectReloads,
                event_index,
            )?;
            metrics.byte_reloads = bump(
                metrics.byte_reloads,
                size_bytes,
                ReplayCounter::ByteReloads,
                event_index,
            )?;
        }
    }
    Ok(())
}

/// One resident expert object and the ordering metadata its policy needs.
#[derive(Debug, Clone, Copy)]
struct ResidentEntry {
    size_bytes: u64,
    frequency: u64,
    last_used_tick: u64,
}

/// Byte-bounded set of resident expert objects.
///
/// The cache owns residency accounting and victim selection. It does not know
/// about events or metrics; the replay loop decides when to consult it.
#[derive(Debug)]
struct ResidentCache {
    budget_bytes: u64,
    resident_bytes: u64,
    entries: BTreeMap<ExpertKey, ResidentEntry>,
    peak_resident_bytes: u64,
    tick: u64,
}

impl ResidentCache {
    fn new(budget_bytes: u64) -> Self {
        Self {
            budget_bytes,
            resident_bytes: 0,
            entries: BTreeMap::new(),
            peak_resident_bytes: 0,
            tick: 0,
        }
    }

    fn contains(&self, key: ExpertKey) -> bool {
        self.entries.contains_key(&key)
    }

    fn free_bytes(&self) -> u64 {
        self.budget_bytes.saturating_sub(self.resident_bytes)
    }

    fn admit(&mut self, key: ExpertKey, size_bytes: u64) {
        // A frequency count belongs to the resident entry, so re-admission
        // starts from zero instead of resurrecting an old hot count.
        self.entries.insert(
            key,
            ResidentEntry {
                size_bytes,
                frequency: 0,
                last_used_tick: self.tick,
            },
        );
        self.resident_bytes = self.resident_bytes.saturating_add(size_bytes);
        // Sampled here rather than once per event: the high-water mark must
        // reflect residency at the instant it changes, so an implementation
        // that admitted before making room could not hide a transient excess.
        self.peak_resident_bytes = self.peak_resident_bytes.max(self.resident_bytes);
    }

    fn peak_resident_bytes(&self) -> u64 {
        self.peak_resident_bytes
    }

    /// Records one access to every key in `accessed`, in deterministic key
    /// order, so recency and frequency reflect this event.
    fn record_access(&mut self, accessed: &BTreeSet<ExpertKey>) {
        for key in accessed {
            self.tick = self.tick.saturating_add(1);
            if let Some(entry) = self.entries.get_mut(key) {
                entry.frequency = entry.frequency.saturating_add(1);
                entry.last_used_tick = self.tick;
            }
        }
    }

    /// Evicts one unpinned object chosen by `policy`, returning its size.
    ///
    /// Returns `None` when every resident object is pinned, which means the
    /// active set itself cannot fit.
    fn evict_one(&mut self, policy: Policy, pinned: &BTreeSet<ExpertKey>) -> Option<u64> {
        let victim = self
            .entries
            .iter()
            .filter(|(key, _)| !pinned.contains(key))
            .min_by(|(_, left), (_, right)| match policy {
                Policy::Lfu => left
                    .frequency
                    .cmp(&right.frequency)
                    .then(left.last_used_tick.cmp(&right.last_used_tick)),
                Policy::Lru | Policy::NoCache => left.last_used_tick.cmp(&right.last_used_tick),
            })
            .map(|(key, entry)| (*key, entry.size_bytes))?;

        self.entries.remove(&victim.0);
        // The entry was accounted on admission, so this cannot underflow.
        self.resident_bytes = self.resident_bytes.saturating_sub(victim.1);
        Some(victim.1)
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.resident_bytes = 0;
    }
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
    use crate::manifest::ExpertSizeEntry;
    use crate::trace::{EventParts, Phase};

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

        let metrics = replay(&manifest, &events, Policy::NoCache, 10).unwrap();

        assert_eq!(metrics.events(), 2);
        assert_eq!(metrics.object_loads(), 3);
        assert_eq!(metrics.byte_loads(), 16);
        assert_eq!(metrics.peak_resident_bytes(), 10);
    }

    #[test]
    fn no_cache_never_hits_and_never_evicts() {
        let manifest = two_experts_4_6();
        let events = [ev(vec![0, 1]), ev(vec![0, 1])];

        let metrics = replay(&manifest, &events, Policy::NoCache, 10).unwrap();

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

        let metrics = replay(&manifest, &events, Policy::NoCache, 10).unwrap();

        // Second activation of each expert is rework, not a cold miss.
        assert_eq!(metrics.object_reloads(), 2);
        assert_eq!(metrics.byte_reloads(), 10);
    }

    #[test]
    fn empty_trace_yields_zeroed_metrics() {
        let no_events: [Event; 0] = [];
        let metrics = replay(&two_experts_4_6(), &no_events, Policy::Lru, 10).unwrap();
        assert_eq!(metrics, ReplayMetrics::default());
    }

    #[test]
    fn empty_active_set_is_an_event_that_loads_nothing() {
        let metrics = replay(&two_experts_4_6(), &[ev(vec![])], Policy::Lru, 10).unwrap();

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
            let metrics = replay(&manifest, &events, policy, 10).unwrap();

            assert_eq!(metrics.object_loads(), 2, "{policy}");
            assert_eq!(metrics.byte_loads(), 10, "{policy}");
            assert_eq!(metrics.object_hits(), 2, "{policy}");
            assert_eq!(metrics.byte_hits(), 10, "{policy}");
            assert_eq!(metrics.object_reloads(), 0, "{policy}");
        }
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

        let metrics = replay(&manifest, &events, Policy::Lfu, 10).unwrap();

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

        let metrics = replay(&manifest, &events, Policy::Lru, 10).unwrap();

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

        let metrics = replay(&manifest, &events, Policy::Lfu, 10).unwrap();

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
            let metrics = replay(&manifest, &events, policy, 10).unwrap();

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
            let metrics = replay(&manifest, &events, policy, 10).unwrap();

            assert_eq!(metrics.object_loads(), 6, "{policy}");
            assert_eq!(metrics.evictions(), 4, "{policy}");
            assert_eq!(metrics.object_reloads(), 2, "{policy}");
            assert_eq!(metrics.peak_resident_bytes(), 10, "{policy}");
        }
    }

    #[test]
    fn an_active_set_larger_than_the_budget_is_rejected() {
        let manifest = manifest_of(&[(0, 6), (1, 6)]);
        let events = [ev(vec![0, 1])];

        for policy in [Policy::NoCache, Policy::Lru, Policy::Lfu] {
            let err = replay(&manifest, &events, policy, 10).unwrap_err();
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
                let metrics = replay(&manifest, &events, policy, budget).unwrap();
                assert!(
                    metrics.peak_resident_bytes() <= budget,
                    "{policy} at budget {budget}: peak {} exceeded it",
                    metrics.peak_resident_bytes()
                );
            }
        }
    }

    #[test]
    fn cyclic_access_defeats_lru_but_stays_within_capacity() {
        // Classic LRU adversary: cycle through one more object than fits.
        let manifest = manifest_of(&[(0, 5), (1, 5), (2, 5)]);
        let events: Vec<Event> = (0..9).map(|i| ev(vec![i % 3])).collect();

        let metrics = replay(&manifest, &events, Policy::Lru, 10).unwrap();

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
            let metrics = replay(&manifest, &events, policy, 10).unwrap();
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
            let metrics = replay(&manifest, &events, policy, 10).unwrap();
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
                let metrics = replay(&manifest, &events, policy, budget).unwrap();
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
            let first = replay(&manifest, &events, policy, 10).unwrap();
            let second = replay(&manifest, &events, policy, 10).unwrap();
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

        let metrics = replay(&manifest, &events, Policy::NoCache, 10).unwrap();

        assert_eq!(metrics.byte_loads(), 16);
    }

    // --- errors ---

    #[test]
    fn unknown_expert_reports_the_failing_event_position() {
        let manifest = two_experts_4_6();
        let events = [ev(vec![0]), ev(vec![7])];

        let err = replay(&manifest, &events, Policy::Lru, 10).unwrap_err();

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

        let err = replay(&manifest, &events, Policy::NoCache, u64::MAX).unwrap_err();

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
        let events = [ev(vec![0, 1]), ev(vec![1])];

        let streamed = replay(&manifest, events.iter(), Policy::NoCache, 10).unwrap();

        assert_eq!(streamed.byte_loads(), 16);
    }
}
