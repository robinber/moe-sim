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
//! another member of the same set. The set also counts as a single access:
//! recency and frequency never observe an order between members of one event.
//!
//! Resident bytes never exceed the applicable capacity: the total budget
//! under a global scope, each layer's quota under a per-layer scope. When an
//! atomic active set cannot fit even after evicting everything unpinned,
//! replay fails instead of bypassing the cache or admitting the event in
//! part.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::manifest::{ExpertKey, ManifestError, ModelManifest};
use crate::scope::CacheScope;
use crate::trace::Event;

/// Cache policy applied during replay.
///
/// A retaining policy decides only which resident object is evicted next;
/// [`Policy::NoCache`] retains nothing, so it never faces that decision. A
/// policy is independent of the budget it operates under, so the same policy
/// can later be applied within a different cache scope.
///
/// One atomic active set counts as one access, so entries can tie on every
/// policy criterion. A genuine tie evicts the lowest expert key first: an
/// explicit rule, not an accident of iteration order.
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
    /// Ties are broken by least recent use, then by the lowest expert key
    /// when entries were last accessed by the same event. A frequency count
    /// belongs to a resident entry and restarts when an object is admitted
    /// again, so a once-hot object does not stay immortal after eviction.
    Lfu,
    /// Evict the unpinned object whose next use lies farthest in the future.
    ///
    /// An offline reference, not an online policy: victim choice reads the
    /// whole trace beyond the current event. Objects never activated again
    /// are evicted first, and entries tied on next use lose by lowest expert
    /// key.
    ///
    /// Only uniform-size manifests are accepted. Greedy farthest-next-use is
    /// the classic MIN optimum for the uniform-size object-load objective
    /// only, and atomic active sets fall outside the classic single-request
    /// proof even there, so replay results are checked against a bounded
    /// exhaustive oracle instead of being called optimal.
    Belady,
}

impl fmt::Display for Policy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::NoCache => "no-cache",
            Self::Lru => "lru",
            Self::Lfu => "lfu",
            Self::Belady => "belady",
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
    /// An atomic active set does not fit its layer quota even with every
    /// unpinned object of that layer evicted.
    ///
    /// The per-layer twin of [`ReplayError::ActiveSetExceedsCapacity`]:
    /// callers are expected to run
    /// [`ModelManifest::validate_per_layer_capacity`] first, and replay
    /// repeats the check rather than trusting it.
    #[error(
        "active set does not fit its layer quota: event {event_index} request {request_id} layer {layer_id} needs {active_set_bytes} bytes, layer quota is {quota_bytes} bytes"
    )]
    ActiveSetExceedsLayerQuota {
        /// Zero-based position of the failing event in the supplied order.
        event_index: usize,
        /// Request of the failing event.
        request_id: u64,
        /// Layer of the failing event.
        layer_id: u32,
        /// Total stored size of the atomic active set in bytes.
        active_set_bytes: u64,
        /// Quota of the event's layer in bytes.
        quota_bytes: u64,
    },
    /// An event activates a layer that has no explicit quota.
    ///
    /// A per-layer cache requires an explicit quota for every simulated
    /// layer; replay refuses to invent one.
    #[error(
        "missing layer quota: event {event_index} (request {request_id}) activates layer {layer_id}, which has no explicit quota"
    )]
    MissingLayerQuota {
        /// Zero-based position of the failing event in the supplied order.
        event_index: usize,
        /// Request of the failing event.
        request_id: u64,
        /// Activated layer that has no quota.
        layer_id: u32,
    },
    /// Summing the per-layer quotas would overflow `u64`.
    #[error("layer quota sum overflowed u64")]
    LayerQuotaSumOverflow,
    /// The per-layer quotas together exceed the total budget.
    ///
    /// Checked before any event is replayed: a run under quotas that break
    /// the `sum <= total budget` contract must not produce a report.
    #[error(
        "layer quotas exceed the total budget: quotas sum to {quota_sum_bytes} bytes, total budget is {total_budget_bytes} bytes"
    )]
    LayerQuotaSumExceedsTotalBudget {
        /// Sum of every declared layer quota in bytes.
        quota_sum_bytes: u64,
        /// Total capacity budget in bytes.
        total_budget_bytes: u64,
    },
    /// The manifest declares more than one expert size under
    /// [`Policy::Belady`].
    ///
    /// Belady's declared objective is the uniform-size object-load minimum.
    /// General variable-size caching has no greedy farthest-next-use
    /// optimum, so the combination is rejected before any event is replayed
    /// instead of being approximated silently. The two named experts are the
    /// manifest's first entry and the first entry whose size differs from
    /// it, in ascending `(layer_id, expert_id)` order.
    #[error(
        "belady requires a uniform expert size: layer {first_layer_id} expert {first_expert_id} has {first_size_bytes} bytes, layer {second_layer_id} expert {second_expert_id} has {second_size_bytes} bytes"
    )]
    BeladyRequiresUniformSizes {
        /// Layer of the manifest's first declared expert.
        first_layer_id: u32,
        /// Expert index of the manifest's first declared expert.
        first_expert_id: u32,
        /// Stored size of the manifest's first declared expert in bytes.
        first_size_bytes: u64,
        /// Layer of the first expert whose size differs.
        second_layer_id: u32,
        /// Expert index of the first expert whose size differs.
        second_expert_id: u32,
        /// Stored size of the first differing expert in bytes.
        second_size_bytes: u64,
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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
    layer_peak_resident_bytes: BTreeMap<u32, u64>,
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

    /// Largest number of bytes resident at any instant during replay, summed
    /// over every cache in the selected scope.
    ///
    /// Under [`CacheScope::Global`] this is the single cache's high-water
    /// mark, sampled at each admission. Under [`CacheScope::PerLayer`] it is
    /// the high-water mark of total residency across the layer caches; only
    /// one layer's cache changes during an event, and residency within an
    /// event peaks at its last admission, so per-event sampling is exact.
    #[must_use]
    pub fn peak_resident_bytes(&self) -> u64 {
        self.peak_resident_bytes
    }

    /// Largest bytes resident in each per-layer cache, keyed by layer id.
    ///
    /// One entry per declared quota, including layers the trace never
    /// activates (their peak is 0). Empty under [`CacheScope::Global`].
    #[must_use]
    pub fn layer_peak_resident_bytes(&self) -> &BTreeMap<u32, u64> {
        &self.layer_peak_resident_bytes
    }
}

/// Replays `events` against `manifest` under `policy` within `scope`.
///
/// Resident bytes never exceed the applicable capacity: the total budget
/// under [`CacheScope::Global`], each layer's quota under
/// [`CacheScope::PerLayer`]. Each event's atomic active set is made fully
/// resident in its own layer's cache before the event is accounted, and no
/// member of that set can be evicted while it is pinned. Unused quota is not
/// shared between layers.
///
/// [`Policy::NoCache`] retains nothing between events, so it never hits and
/// never evicts; the applicable capacity still bounds each individual active
/// set.
///
/// `events` is collected before the first event is replayed:
/// [`Policy::Belady`] chooses victims from the schedule beyond the current
/// event, and the `&Event` item bound already makes the caller supply the
/// whole trace. This function does not replay a trace larger than memory.
///
/// # Errors
///
/// Returns [`ReplayError::BeladyRequiresUniformSizes`] before any event is
/// replayed when [`Policy::Belady`] is selected and the manifest declares
/// more than one expert size.
/// Returns [`ReplayError::LayerQuotaSumOverflow`] or
/// [`ReplayError::LayerQuotaSumExceedsTotalBudget`] before any event is
/// replayed when per-layer quotas break the `sum <= total budget` contract.
/// Returns [`ReplayError::ActiveSetBytes`] when an event references an expert
/// the manifest does not declare, or when one active set's byte total
/// overflows.
/// Returns [`ReplayError::MissingLayerQuota`] when an event activates a layer
/// without an explicit quota.
/// Returns [`ReplayError::ActiveSetExceedsCapacity`] under a global scope, or
/// [`ReplayError::ActiveSetExceedsLayerQuota`] under a per-layer scope, when
/// an atomic active set cannot fit even with every unpinned object evicted.
/// Returns [`ReplayError::CounterOverflow`] when a cumulative counter exceeds
/// `u64`.
pub fn replay<'a>(
    manifest: &ModelManifest,
    events: impl IntoIterator<Item = &'a Event>,
    policy: Policy,
    scope: &CacheScope,
) -> Result<ReplayMetrics, ReplayError> {
    let events: Vec<&'a Event> = events.into_iter().collect();
    let next_uses = if policy == Policy::Belady {
        ensure_uniform_expert_size(manifest)?;
        Some(next_use_schedule(&events))
    } else {
        None
    };

    let mut metrics = ReplayMetrics::default();
    let mut caches = ScopedCaches::new(scope)?;
    let mut ever_loaded: BTreeSet<ExpertKey> = BTreeSet::new();
    let mut peak_total_bytes: u64 = 0;

    for (event_index, &event) in events.iter().enumerate() {
        let layer_id = event.layer_id();
        let request_id = event.request_id();
        let active_set_bytes =
            manifest
                .active_set_bytes(event)
                .map_err(|source| ReplayError::ActiveSetBytes {
                    event_index,
                    request_id,
                    layer_id,
                    source,
                })?;

        metrics.events = bump(metrics.events, 1, ReplayCounter::Events, event_index)?;

        let cache = caches.cache_mut(layer_id, event_index, request_id)?;

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
                        request_id,
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
            cache,
            &mut metrics,
            policy,
            &pinned,
            required_bytes,
            event_index,
        )? {
            return Err(match scope {
                CacheScope::Global { budget_bytes } => ReplayError::ActiveSetExceedsCapacity {
                    event_index,
                    request_id,
                    layer_id,
                    active_set_bytes,
                    budget_bytes: *budget_bytes,
                },
                CacheScope::PerLayer { .. } => ReplayError::ActiveSetExceedsLayerQuota {
                    event_index,
                    request_id,
                    layer_id,
                    active_set_bytes,
                    quota_bytes: cache.budget_bytes(),
                },
            });
        }

        admit_missing(cache, &mut metrics, &mut ever_loaded, &missing, event_index)?;

        cache.record_access(&pinned);

        if let Some(schedule) = &next_uses {
            record_event_next_uses(cache, event, &schedule[event_index]);
        }

        // Sampled after this event's admissions and before any no-cache
        // release: only this event's layer changed, and its residency peaks
        // at the last admission, so the per-event sample is the true
        // high-water of summed residency.
        peak_total_bytes = peak_total_bytes.max(caches.total_resident_bytes());

        if policy == Policy::NoCache {
            // Release, not eviction: nothing was retained by choice.
            caches.cache_mut(layer_id, event_index, request_id)?.clear();
        }
    }

    metrics.peak_resident_bytes = match &caches {
        // The single cache samples its own high-water at each admission,
        // which additionally exposes any transient excess inside one event.
        ScopedCaches::Global(cache) => cache.peak_resident_bytes(),
        ScopedCaches::PerLayer(_) => peak_total_bytes,
    };
    metrics.layer_peak_resident_bytes = caches.layer_peaks();

    Ok(metrics)
}

/// The resident caches for one replay: one per scope partition.
enum ScopedCaches {
    /// One cache over the total budget.
    Global(ResidentCache),
    /// One independent cache per quota'd layer.
    PerLayer(BTreeMap<u32, ResidentCache>),
}

impl ScopedCaches {
    /// Builds the caches, re-checking the quota-sum contract rather than
    /// trusting the caller's validation pass.
    fn new(scope: &CacheScope) -> Result<Self, ReplayError> {
        match scope {
            CacheScope::Global { budget_bytes } => {
                Ok(Self::Global(ResidentCache::new(*budget_bytes)))
            }
            CacheScope::PerLayer {
                total_budget_bytes,
                layer_quota_bytes,
            } => {
                let mut quota_sum_bytes: u64 = 0;
                for &quota in layer_quota_bytes.values() {
                    quota_sum_bytes = quota_sum_bytes
                        .checked_add(quota)
                        .ok_or(ReplayError::LayerQuotaSumOverflow)?;
                }
                if quota_sum_bytes > *total_budget_bytes {
                    return Err(ReplayError::LayerQuotaSumExceedsTotalBudget {
                        quota_sum_bytes,
                        total_budget_bytes: *total_budget_bytes,
                    });
                }
                Ok(Self::PerLayer(
                    layer_quota_bytes
                        .iter()
                        .map(|(&layer_id, &quota)| (layer_id, ResidentCache::new(quota)))
                        .collect(),
                ))
            }
        }
    }

    /// The cache an event on `layer_id` runs in.
    fn cache_mut(
        &mut self,
        layer_id: u32,
        event_index: usize,
        request_id: u64,
    ) -> Result<&mut ResidentCache, ReplayError> {
        match self {
            Self::Global(cache) => Ok(cache),
            Self::PerLayer(caches) => {
                caches
                    .get_mut(&layer_id)
                    .ok_or(ReplayError::MissingLayerQuota {
                        event_index,
                        request_id,
                        layer_id,
                    })
            }
        }
    }

    /// Bytes currently resident across every cache.
    ///
    /// Each cache stays within its budget and validation bounds the budget
    /// sum, so the saturation can only mask an error that other invariants
    /// already exclude.
    fn total_resident_bytes(&self) -> u64 {
        match self {
            Self::Global(cache) => cache.resident_bytes(),
            Self::PerLayer(caches) => caches.values().fold(0u64, |sum, cache| {
                sum.saturating_add(cache.resident_bytes())
            }),
        }
    }

    /// Per-layer high-water marks; empty under a global scope.
    fn layer_peaks(&self) -> BTreeMap<u32, u64> {
        match self {
            Self::Global(_) => BTreeMap::new(),
            Self::PerLayer(caches) => caches
                .iter()
                .map(|(&layer_id, cache)| (layer_id, cache.peak_resident_bytes()))
                .collect(),
        }
    }
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

/// Rejects manifests that declare more than one expert size.
///
/// [`Policy::Belady`]'s applicability gate: its declared objective is the
/// uniform-size object-load minimum, so a variable-size manifest must fail
/// before any event is replayed.
fn ensure_uniform_expert_size(manifest: &ModelManifest) -> Result<(), ReplayError> {
    let mut entries = manifest.entries();
    let Some(first) = entries.next() else {
        return Ok(());
    };
    for entry in entries {
        if entry.size_bytes != first.size_bytes {
            return Err(ReplayError::BeladyRequiresUniformSizes {
                first_layer_id: first.key.layer_id(),
                first_expert_id: first.key.expert_id(),
                first_size_bytes: first.size_bytes,
                second_layer_id: entry.key.layer_id(),
                second_expert_id: entry.key.expert_id(),
                second_size_bytes: entry.size_bytes,
            });
        }
    }
    Ok(())
}

/// For each event, each active-set member's next activation, aligned with
/// the event's `expert_ids` order. `None` marks a member never activated
/// again.
///
/// Keys carry their layer, so under a per-layer scope each cache still sees
/// exactly the schedule of its own layer.
fn next_use_schedule(events: &[&Event]) -> Vec<Vec<Option<usize>>> {
    let mut next_activation: BTreeMap<ExpertKey, usize> = BTreeMap::new();
    let mut schedule = vec![Vec::new(); events.len()];
    for (event_index, event) in events.iter().enumerate().rev() {
        let member_next_uses = &mut schedule[event_index];
        for &expert_id in event.expert_ids() {
            let key = ExpertKey::new(event.layer_id(), expert_id);
            // Walking backwards, the previously stored index is exactly the
            // next activation after this event.
            member_next_uses.push(next_activation.insert(key, event_index));
        }
    }
    schedule
}

/// Applies one event's slice of the schedule to its resident members.
///
/// Every member is resident when this runs, so each entry learns when its
/// next activation comes; eviction candidates always carry a schedule
/// because members stay pinned until their event has recorded it.
fn record_event_next_uses(
    cache: &mut ResidentCache,
    event: &Event,
    member_next_uses: &[Option<usize>],
) {
    for (&expert_id, &next_use_event) in event.expert_ids().iter().zip(member_next_uses) {
        cache.record_next_use(ExpertKey::new(event.layer_id(), expert_id), next_use_event);
    }
}

/// Ranks a next use so later activations rank higher and "never activated
/// again" ranks highest of all.
fn next_use_rank(next_use_event: Option<usize>) -> (bool, usize) {
    next_use_event.map_or((true, 0), |event_index| (false, event_index))
}

/// One resident expert object and the ordering metadata its policy needs.
#[derive(Debug, Clone, Copy)]
struct ResidentEntry {
    size_bytes: u64,
    frequency: u64,
    last_used_tick: u64,
    next_use_event: Option<usize>,
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

    fn budget_bytes(&self) -> u64 {
        self.budget_bytes
    }

    fn resident_bytes(&self) -> u64 {
        self.resident_bytes
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
                next_use_event: None,
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

    /// Records one event's atomic active set as a single access: the tick
    /// advances once and every member shares it, so recency never invents an
    /// order between members of the same set.
    fn record_access(&mut self, accessed: &BTreeSet<ExpertKey>) {
        self.tick = self.tick.saturating_add(1);
        for key in accessed {
            if let Some(entry) = self.entries.get_mut(key) {
                entry.frequency = entry.frequency.saturating_add(1);
                entry.last_used_tick = self.tick;
            }
        }
    }

    /// Records one member's next activation so Belady's victim choice sees
    /// the declared schedule; `None` marks a member never activated again.
    fn record_next_use(&mut self, key: ExpertKey, next_use_event: Option<usize>) {
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.next_use_event = next_use_event;
        }
    }

    /// Evicts one unpinned object chosen by `policy`, returning its size.
    ///
    /// Entries tied on every policy criterion lose by lowest expert key.
    /// Returns `None` when every resident object is pinned, which means the
    /// active set itself cannot fit.
    fn evict_one(&mut self, policy: Policy, pinned: &BTreeSet<ExpertKey>) -> Option<u64> {
        let victim = self
            .entries
            .iter()
            .filter(|(key, _)| !pinned.contains(key))
            .min_by(|(left_key, left), (right_key, right)| {
                let criteria = match policy {
                    Policy::Lfu => left
                        .frequency
                        .cmp(&right.frequency)
                        .then(left.last_used_tick.cmp(&right.last_used_tick)),
                    Policy::Lru | Policy::NoCache => left.last_used_tick.cmp(&right.last_used_tick),
                    // Reversed on purpose: the entry whose next use is
                    // farthest away (or never comes) must sort first.
                    Policy::Belady => {
                        next_use_rank(right.next_use_event).cmp(&next_use_rank(left.next_use_event))
                    }
                };
                // Members of one atomic set share one access timestamp, so a
                // genuine tie is possible and must be broken explicitly.
                criteria.then(left_key.cmp(right_key))
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
mod belady_tests;
#[cfg(test)]
mod tests;
