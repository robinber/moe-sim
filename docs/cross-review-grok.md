# Cross-review: M0 global-capacity validation (core)

**Reviewer:** grok  
**Branch:** `m0-capacity-fixtures-cli`  
**Scope:** design docs + `CapacityError` / `ModelManifest::validate_global_capacity` + unit tests  
**Mode:** cross-review only; production code not edited (no P0 found).  
**Date:** 2026-07-22

## 1. Verdict

**Approve-with-nits**

The pure core API matches the approved design and the AGENTS simulation
invariants that apply to this slice. Design-review P1 amendments are present
in code and rustdoc. `cargo test --workspace` passes (26 unit tests + 0
doctests). Residual nits are documentation/tracking and thin edge-case test
gaps, not correctness defects.

## 2. Correctness vs design

### Invariants

| Design / AGENTS rule | Implementation | Assessment |
| --- | --- | --- |
| Reject config before results; no partial admit | Pure `Result<(), CapacityError>`; no residency path | Correct |
| Atomic active set via unique `expert_ids` | Delegates to `active_set_bytes` (layer-scoped sum) | Correct |
| No silent reordering | Single event pass over caller iterator; no sort | Correct |
| Unknown expert / overflow are errors | Wrapped as `CapacityError::ActiveSetBytes` | Correct |
| Object/byte metrics, policies, residency | Out of scope; not introduced | Correct |
| `CapacityError` ≠ `ManifestError` | Separate enum; source chain preserved | Correct |

### Pass order

Manifest sweep first (`BTreeMap` key order), then events in supplied order.
Code:

1. `for (key, &size_bytes) in &self.sizes` — fail on first `size_bytes > budget`
2. `for (event_index, event) in events.into_iter().enumerate()` — fail on
   lookup/overflow or `active_set_bytes > budget`

This matches design rules 1–2 and the stated priority invariant: an
`ActiveSetExceedsGlobalCapacity` implies every member expert individually
fits. Covered by:

- `global_capacity_reports_lowest_oversize_expert_key` (key order among ≥2
  oversize entries)
- `global_capacity_runs_manifest_pass_before_independent_event_failure`
  (unreferenced oversize + earlier unknown-expert event → expert error)
- `global_capacity_rejects_oversize_active_set_in_supplied_order`
  (non-monotonic `step_id` / `token_position`; first failure is file-order)

### Exact-fit

Comparisons use strict greater-than (`>`), not `>=`. Equality is valid for
both individual experts and atomic active-set totals. Covered by
`global_capacity_accepts_exact_fit_expert_and_active_set` (40 + 60 = 100).

### Unreferenced experts

Every manifest entry is checked regardless of event references. Rationale is
documented in design §Decision and in `validate_global_capacity` rustdoc
(M0 property of `(manifest, budget)`; revisit marker for real-data adapters).
Covered by `global_capacity_rejects_unreferenced_oversize_expert` and the
pass-order test above.

### Zero budget

Empty manifest + empty events / empty active set → `Ok(())`. Any positive
stored size fails the expert pass (manifest construction already forbids
zero-size entries). Empty-input and empty-active-set cases are tested;
positive size at budget `0` is implied by the comparison, not dedicated.

### Error taxonomy and context

- Variant set, field names, and `#[error(...)]` strings match the design
  sketch.
- Derives: `Debug, Clone, PartialEq, Eq, thiserror::Error` (design-review
  P2.4 addressed).
- `ActiveSetBytes` documents that `source` is only `UnknownExpert` or
  `ActiveSetBytesOverflow`, and that `layer_id` deliberately duplicates
  source context (design-review P2.2).
- Tests assert full context fields (`event_index`, `request_id`, `layer_id`,
  sizes/budget, nested source).

### Scope discipline

No CLI, fixtures tree, policies, residency, async, or new crates. Matches
design “Planned CLI sketch” / “Out of scope” and `docs/m0-impl-notes.md`.
Branch name still implies fixtures/CLI; residual work is explicitly deferred
in impl notes — acceptable if the PR description says so.

## 3. Test gaps

No gap that blocks this slice. Optional strengthens (P2):

1. **Single-expert exact fit** — expert size == budget with a one-expert
   active set (sum exact-fit is covered; individual equality is the same
   branch but not named).
2. **Zero budget rejects positive size** — e.g. one 1 B expert, budget `0` →
   `ExpertExceedsGlobalCapacity` (corollary of rules 1 and 6; code is
   obvious).
3. **Empty active set on a non-empty, in-budget manifest** — expert pass only
   then event pass with total `0`; success path for mixed empty sets.
4. **Display / `Error::source` chain** — no assertion that
   `std::error::Error::source()` on `ActiveSetBytes` yields the
   `ManifestError` (thiserror field name `source` should wire this; a one-line
   test would lock it).

Design fixture semantics (`global-exact-fit`, oversize expert/set, unknown,
overflow, file-order) are present as unit tests; on-disk `fixtures/` correctly
wait for a CLI adapter.

## 4. API / docs issues

**Solid:**

- Public export of `CapacityError` from `lib.rs`.
- Rustdoc on the method documents order, unreferenced strictness, exact-fit,
  zero-budget, and error variants with `# Errors`.
- Design doc status: approved; core implemented; CLI deferred.
- Design-review P1.1–P1.3 folded into design, rustdoc, and tests.
- Impl notes honestly state CLI residual.

**Nits:**

1. **`ROADMAP.md` M0 checklist** still unchecked for “Define oversize expert
   and oversize active-set rejection” (and fixtures/CLI). Core rejection is
   implemented; leaving the box unchecked is defensible until CLI wiring, but
   a one-line note or partial check would avoid understating progress.
2. **Branch name vs delivered scope** — `m0-capacity-fixtures-cli` vs core-only
   delivery. PR title/body should state the cut.
3. **`m0-impl-notes.md` “Delivered”** is accurate for the core; no conflict
   with design. No production-code doc drift found in `manifest.rs` /
   `lib.rs` / `trace.rs`.

No public API shape change requested. No rustdoc missing on new public items
from a static read (formal `cargo doc -D warnings` not re-run in this review).

## 5. P0 / P1 / P2 list

### P0 — must fix before merge

None.

### P1 — should fix (non-blocking for core correctness)

None required for correctness of this pure validation pass.

Optional process P1 (docs only):

1. In the PR description (or a one-line ROADMAP note), record that global
   capacity **API + unit tests** are done and that CLI / on-disk fixtures
   remain residual so M0 exit criteria are not overstated as fully closed.

### P2 — nits

1. Optional unit tests listed in §3 (zero-budget oversize expert; single
   expert exact-fit; empty active set on non-empty manifest; `Error::source`).
2. Align branch naming / PR scope wording with deferred CLI.
3. Consider updating the M0 deliverable checkbox or annotating partial
   completion for capacity rejection once this PR lands.

## 6. PR-ready?

**Yes** — for the **core global-capacity validation slice** as documented in
`docs/m0-impl-notes.md` (CLI and `fixtures/` deferred).

Not a claim that full M0 exit criteria are closed (CLI crate, on-disk
fixtures, provenance still open per ROADMAP).

### Verification evidence

```text
cargo test --workspace
# 26 passed; 0 failed (moe-sim-core unit tests)
# 0 doctests
```

Not run in this review: `fmt --check`, `clippy -D warnings`,
`cargo doc -D warnings`, `cargo deny`. Impact-scoped test suite was the
requested gate; widen before release if those gates are part of the merge bar.

### Residual (explicit non-goals of this PR)

- `moe-sim-cli` / `capacity check` / exit codes
- JSONL + TOML|JSON adapters and `fixtures/` files
- Validation witness / structural enforcement (M1 option)
- Per-layer capacity scopes
