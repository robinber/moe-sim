# Cross-review v2 (Claude): M0 global-capacity implementation

Reviewed: commit `5f3519f` on `m0-capacity-fixtures-cli` —
`crates/moe-sim-core/src/{lib,manifest,trace}.rs`, `docs/m0-capacity-design.md`,
`docs/m0-capacity-design-review.md`, `docs/m0-impl-notes.md`, and the M0
section of `ROADMAP.md`.

Axes: (1) API design and public surface, (2) documentation and contract
honesty, (3) scope discipline / M0 boundary, (4) maintainability and
reviewability. Adversarial algorithm edge-case hunting is explicitly out of
lane (owned by the Codex reviewer).

Verification run by this reviewer: `cargo test --workspace` → 26 passed,
0 failed. Not run by this reviewer (smoke-only mandate): `cargo +nightly fmt
--check`, `cargo clippy`, `cargo doc -D warnings`, `cargo deny`. No claim is
made for those gates here; run them before merge if not already evidenced
elsewhere.

## 1. Verdict

**Approve-with-nits.** No P0 or P1 findings on these axes. All findings below
are P2 and none block merge.

## 2. Findings by axis

### Axis 1 — API design & public surface

Severity: **P2 (nits only)**.

The implemented surface matches the approved design verbatim: signature and
parameter order of `validate_global_capacity`, the borrowed
`impl IntoIterator<Item = &'a Event>` (single pass, no forced allocation,
works with slices and `iter::once`), the bare `u64` budget, the three
`CapacityError` variants with their exact field sets, the
`Debug, Clone, PartialEq, Eq, thiserror::Error` derives matching
`ManifestError`, and `#[error(...)]` strings identical to the design doc. The
`lib.rs` re-export keeps the existing alphabetical style. `Result` return
gives `#[must_use]` behavior for free.

- P2: `CapacityError` sits between `ModelManifest` and `ManifestError` in
  `manifest.rs`, so its rustdoc and `source` field reference an enum defined
  later in the file. Cosmetic; consider moving it below `ManifestError` in a
  future touch.
- P2 (recorded choice, no change requested): neither error enum is
  `#[non_exhaustive]`. Consistent with existing `ManifestError`/`EventError`
  and fine while the workspace is unpublished; noting it so it stays a
  decision, not an accident.
- Observation: the `ActiveSetBytes` variant name reads as a data carrier
  rather than a failure. It is the design-approved name, so no change is
  requested; a future error-taxonomy pass could prefer a
  `...Calculation`-style name.

### Axis 2 — Documentation & contract honesty

Severity: **P2 (nits only)**.

The rustdoc matches the behavior, and every amendment the design review asked
to fold in is present:

- P1.1 — unreferenced-expert strictness is in the method rustdoc with the
  rationale and an explicit revisit marker for real-data/adapter slices.
- P1.2 / P1.3 — both required tests exist
  (`global_capacity_reports_lowest_oversize_expert_key`,
  `global_capacity_runs_manifest_pass_before_independent_event_failure`).
- §6 extras — all present: layer-scoped capacity-level case, non-empty
  manifest with empty events → `Ok(())`, and event-context assertions match
  full error structs, not just variants.
- P2.1 — the "convention in M0; M1 may enforce structurally" caveat is in the
  rustdoc. P2.2 — `source` is constrained to `UnknownExpert` /
  `ActiveSetBytesOverflow` and the deliberate `layer_id` duplication is
  documented on the variant. P2.3 — tests extend the existing `entry` /
  `sample_event` helpers (one new `event_with_ids` helper, no builder
  framework). P2.4 — derives and display strings as specified.

All six semantic fixture cases from the design (`global-exact-fit`,
`global-oversize-expert`, `global-oversize-active-set`, `unknown-expert`,
`active-set-overflow`, `file-order-first-failure`) exist as tests with
comments naming them. `docs/m0-impl-notes.md` claims exactly what is
delivered, states the CLI deferral plainly, and its residual section matches
the design's planned-CLI sketch. The design doc's status line ("the core API
is implemented") is now true.

- P2: rule 6's rejection side (non-empty manifest at a zero budget) has no
  dedicated test; it is covered by the same `>` comparison the oversize tests
  exercise. One-line test would pin the documented corollary. Left to the
  Codex lane if it prefers.
- P2: `CapacityError::ActiveSetBytes` context tests only exercise
  `event_index: 0`; index propagation at a later position is pinned only via
  `ActiveSetExceedsGlobalCapacity`. Same lane note as above.

### Axis 3 — Scope discipline / M0 boundary

Severity: **P2 (one nit)**.

The slice stays inside M0-close: no CLI crate, no policy or residency code,
no per-layer scopes, no new dependencies (reuses the existing `thiserror`
workspace dependency), no new crates, and nothing from the deferred-surfaces
list. The design's out-of-scope list is honored in code.

ROADMAP checkbox honesty is good: commit `5f3519f` checks exactly one box —
"Define oversize expert and oversize active-set rejection" — which the same
commit makes true. The CLI, provenance, and fixtures boxes remain honestly
unchecked. The exit criterion "fail before simulation results are emitted" is
met by convention (validation returns `()`, no simulator exists yet to
bypass it), with the M1 witness-type option recorded in both the design doc
and the rustdoc.

- P2: the branch name `m0-capacity-fixtures-cli` promises fixtures and a CLI
  that this slice deliberately defers. The PR title/description should state
  the narrower delivered scope (the impl notes already do) so the merge
  record does not over-claim.

### Axis 4 — Maintainability & reviewability

Severity: **P2 (nits only)**.

Capacity tests are grouped under a consistent `global_capacity_` prefix,
assert full error structs, and use hand-computable byte values with comments
tying them to the named design fixtures — good review ergonomics. Helper
drift was avoided: `sample_event` now delegates to `event_with_ids` instead
of a parallel builder. The implementation itself is a straightforward
two-loop delegation to `active_set_bytes` with no panic paths.

- P2: `event_with_ids` also sets `request_id`, `step_id`, and
  `token_position`; a name like `event_with_metadata` would describe it
  better. Cosmetic.
- P2: `manifest.rs` (~676 lines) now holds both manifest construction and
  capacity validation. Acceptable at this size; if per-layer scope lands in
  M1, splitting a `capacity.rs` module is the natural seam. Not requested
  now.

## 3. Doc/scope gaps

None blocking. The two P2 test-pinning gaps (zero-budget rejection,
`event_index > 0` for `ActiveSetBytes`) and the branch-name/PR-title wording
are the only residuals; the fmt/clippy/rustdoc/deny gates were not run under
this review's mandate and need evidence from another lane before merge.

## 4. PR-ready for these axes?

**Yes** — axes 1–4 are merge-ready as reviewed, subject to the standard
format/lint/doc gates being evidenced elsewhere.
