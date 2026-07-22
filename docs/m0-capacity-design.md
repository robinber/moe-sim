# M0 global-capacity validation design

Status: approved. The core API is implemented in M0; the CLI sketch remains
deferred.

## Decision and boundary

Add one synchronous, deterministic validation pass to `moe-sim-core`. It
accepts a `ModelManifest`, canonical `Event`s, and one global `u64` byte budget.
It performs no I/O, parsing, rendering, replay, or residency mutation.

Use a new `CapacityError`, not new `ManifestError` variants. A manifest can be
intrinsically valid yet incompatible with a run budget. `ManifestError` keeps
ownership of construction, lookup, and byte-sum failures; `CapacityError`
provides run-level context and preserves an underlying `ManifestError`.

M0 checks unreferenced manifest experts deliberately: expert feasibility is a
deterministic property of `(manifest, budget)` before trace inspection. This
strictness must be revisited when real-data adapters may pair a full-model
manifest with a trace that activates only a subset.

## API sketch

Keep the method next to `ModelManifest` and reuse `active_set_bytes`:

```rust
impl ModelManifest {
    /// Validates every manifest entry and the events against a global budget.
    ///
    /// Unreferenced entries are included so M0 expert feasibility depends only
    /// on the manifest and budget.
    ///
    /// # Errors
    ///
    /// Returns a typed capacity or active-set byte calculation error.
    pub fn validate_global_capacity<'a>(
        &self,
        global_budget_bytes: u64,
        events: impl IntoIterator<Item = &'a Event>,
    ) -> Result<(), CapacityError>;
}
```

The named `u64` matches manifest sizes without introducing a configuration
type before another capacity scope exists. All new public items and error
fields require rustdoc.

## Error kinds

```rust
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CapacityError {
    #[error(
        "expert exceeds global capacity: layer {layer_id} expert {expert_id} has size {size_bytes} bytes, global budget is {global_budget_bytes} bytes"
    )]
    ExpertExceedsGlobalCapacity {
        layer_id: u32,
        expert_id: u32,
        size_bytes: u64,
        global_budget_bytes: u64,
    },
    #[error(
        "failed to calculate active-set bytes for event {event_index} (request {request_id}, layer {layer_id}): {source}"
    )]
    ActiveSetBytes {
        event_index: usize,
        request_id: u64,
        layer_id: u32,
        source: ManifestError,
    },
    #[error(
        "active set exceeds global capacity: event {event_index} request {request_id} layer {layer_id} totals {active_set_bytes} bytes, global budget is {global_budget_bytes} bytes"
    )]
    ActiveSetExceedsGlobalCapacity {
        event_index: usize,
        request_id: u64,
        layer_id: u32,
        active_set_bytes: u64,
        global_budget_bytes: u64,
    },
}
```

`ActiveSetBytes` wraps only `UnknownExpert` or `ActiveSetBytesOverflow` with
the zero-based event position. Its `layer_id` deliberately duplicates source
context for uniform event diagnostics. The other variants carry the identities
and both compared byte values. Implementation uses propagation, with no
production `unwrap`, `expect`, or panic path.

## Validation rules

1. Check every manifest entry, including unreferenced entries, in its existing
   deterministic `(layer_id, expert_id)` order. Fail on the first expert whose
   size is greater than the global budget.
2. Visit events once in caller-supplied order. Never sort by `step_id`,
   `token_position`, or `layer_id`.
3. Treat each event's unique `expert_ids` as one atomic set. Use
   `active_set_bytes`; never partially admit the set or bypass capacity.
4. Equality with the budget is valid; only values greater than it fail.
5. Unknown experts and `u64` sum overflow are errors, never estimates.
6. A zero budget accepts only an empty manifest and zero-byte active sets. The
   current empty active set remains valid and totals zero bytes.
7. Return only `()` on success, so callers validate before emitting results.

M0 relies on callers to invoke validation. M1 may make this structural with a
validation witness or replay-internal validation if a replay path is added.

The manifest sweep precedes the event sweep. Thus an oversize expert has stable
priority; an active-set capacity error means each expert fits individually but
the combined atomic set does not.

## Test plan and fixtures

Core unit tests extend the existing private `entry` and `sample_event` helpers;
no test-only builder framework is added. Cover:

- empty input at zero budget and exact-fit boundaries;
- an unreferenced oversize expert and an event referencing an oversize expert;
- individually valid experts whose atomic sum is oversize;
- unknown-expert and active-set-overflow context;
- deterministic selection of the lowest oversize manifest key;
- manifest-pass priority over an independent event failure;
- layer-scoped active-set lookup and non-empty manifest / empty trace success;
- all error context fields and first failure in supplied order, using
  non-monotonic metadata to prove no reordering.

Tiny fixture cases needed later by adapters and the CLI:

- `global-exact-fit`: 40 B + 60 B active set, 100 B budget;
- `global-oversize-expert`: 101 B expert, 100 B budget;
- `global-oversize-active-set`: 60 B + 50 B active set, 100 B budget;
- `unknown-expert`: event key absent from the manifest;
- `active-set-overflow`: `u64::MAX` + 1 B, `u64::MAX` budget;
- `file-order-first-failure`: valid then invalid events whose metadata would
  sort in another order.

These remain semantic unit-test cases. No files under `fixtures/` are needed
until a CLI adapter lands; that adapter should prefer JSONL events and a simple
TOML or JSON manifest.

## Planned CLI sketch

- `trace inspect`: validate parsing and canonical `Event` construction while
  preserving source order; do not validate capacity.
- `capacity check`: validate the manifest, references, byte totals, and global
  capacity through the pure core API; emit no simulation result.

No CLI crate, arguments, serialization contract, output schema, or exit-code
mapping is implemented in this step.

## Out of scope

- per-layer scopes or quotas;
- LRU, LFU, or other policies;
- residency, pin/release, eviction, or replay accounting;
- provenance and seeds, which this deterministic validation does not need;
- storage, timing, async work, prefetch, physical I/O, and production CLI code.

## Resolved decisions

- Unit tests and existing helpers cover this slice; file fixtures wait for a
  CLI adapter.
- Empty active sets remain valid and contribute zero bytes.
