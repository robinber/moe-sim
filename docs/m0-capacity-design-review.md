# Review: M0 global-capacity validation design

Reviewed: `docs/m0-capacity-design.md` against `crates/moe-sim-core/src/{lib,trace,manifest}.rs`,
`AGENTS.md` invariants, the M0 exit criteria in `ROADMAP.md`, and the
rust-strict skill. Branch: `m0-capacity-fixtures-cli`. Design-only review; no
production code was written or verified by compilation.

## 1. Decision

**Approve.** No P0 blockers. The P1 items below are amendments to the design
doc and test plan that fit inside this slice without redesign; fold them in
during implementation.

## 2. Strengths

- The `CapacityError` / `ManifestError` split cleanly separates intrinsic
  manifest validity from run-relative feasibility, and preserves the source
  chain instead of flattening it.
- The API is pure, synchronous, and single-pass over a borrowed iterator —
  it matches the core boundary rules ("explicit values or streams") and the
  existing `active_set_bytes` contract exactly.
- Boundary semantics are nailed down explicitly: exact fit is valid (rule 4),
  overflow is an error not an estimate (rule 5), and the zero-budget rule 6 is
  a stated corollary rather than a special case.
- Fixtures are hand-computable and adversarial: `active-set-overflow` sits
  exactly at the `u64` edge (expert `u64::MAX` passes the expert check, the
  set overflows), and `file-order-first-failure` proves metadata is never used
  to reorder — directly exercising the file-order invariant.
- Scope discipline is strong: the out-of-scope list matches the deferred
  surfaces in `AGENTS.md`, the CLI is named-only, and genuinely open decisions
  are surfaced as open questions instead of silently resolved.

## 3. Issues

### P0 — must fix before implement

None.

### P1 — should fix

1. **Unreferenced-expert strictness is stated but neither justified nor in the
   API rustdoc.** Rule 1 checks every manifest entry, including experts the
   trace never activates — the load-bearing semantic choice of this design. It
   is defensible for M0 (the expert pass becomes a deterministic property of
   `(manifest, budget)` alone), but it will reject a full real-model manifest
   paired with a small budget even when the trace only touches experts that
   fit. Add the rationale to the doc, put the behavior in the
   `validate_global_capacity` rustdoc (the current sketch does not mention
   it), and record a revisit marker for the real-data/adapter slices.
2. **No test for deterministic first-expert selection.** The design claims
   "the first oversize expert fails validation" in `(layer_id, expert_id)`
   order, but no planned test has two or more oversize manifest entries. Add
   one asserting the error names the lowest key.
3. **The pass-ordering proof is conflated with same-expert priority.** The
   planned "oversize expert referenced by an event" test proves priority only
   when both passes flag the same expert. Add a case where an *unreferenced*
   oversize expert coexists with an earlier event carrying its own violation
   (e.g., unknown expert); expect `ExpertExceedsGlobalCapacity`, proving the
   manifest pass runs first regardless of event content.

### P2 — nit

1. Rule 7's "returns `()` … so callers must complete this pass" is a
   convention, not enforcement — nothing stops a future replay path from
   skipping it. Fine for M0; note the M1 option of a validation witness type
   or replay-internal validation so the contract becomes structural.
2. `ActiveSetBytes { source: ManifestError }` admits variants that cannot
   occur there (`ZeroSize`, `DuplicateKey`). Acceptable; constrain via rustdoc
   ("only `UnknownExpert` or `ActiveSetBytesOverflow`"). Likewise the
   wrapper's `layer_id` always duplicates the source's — document that it is a
   deliberate convenience.
3. The test-only `ManifestBuilder` / `EventBuilder` duplicate the existing
   `entry()` / `sample_event()` helpers in `manifest.rs` tests. Tests may use
   `unwrap` under the existing `#[expect]` pattern, so the builders' stated
   motivation is weak. Prefer extending the existing helpers (drift control:
   avoid over-architected test-only code), or justify the builders in the doc.
4. The `CapacityError` sketch omits derives. The test plan matches errors
   directly, which requires `PartialEq`; specify
   `Debug, Clone, PartialEq, Eq` plus `thiserror::Error`, matching
   `ManifestError`, and specify `#[error(...)]` display strings in the
   existing message style.

## 4. Verdict: `CapacityError` vs `ManifestError` split

**Correct.** A manifest is valid or invalid on its own; capacity feasibility
is a property of the `(manifest, budget, trace)` triple with a different
lifecycle and a different remediation (fix model data vs. raise the budget or
change the trace). Extending `ManifestError` would force manifest-construction
call sites to pattern over run-level variants that cannot occur there, eroding
the "typed, actionable" contract. Wrapping via a `source` field preserves the
underlying error without flattening. See P2.2 for the one caveat.

## 5. Verdict: validation order (all experts first, then events)

**Sound.** The manifest pass first yields two good properties: expert errors
are deterministic and trace-independent (BTreeMap key order makes "first
oversize expert" well-defined), and any `ActiveSetExceedsGlobalCapacity`
therefore guarantees every member expert individually fits — a clean,
testable invariant the doc states explicitly. The event pass in
caller-supplied order honors the file-order invariant, and the single-pass
iterator shape matches "visit events exactly once." The one load-bearing
choice inside this order — checking unreferenced entries — is endorsed for M0
but must be documented and marked for revisit (P1.1).

## 6. Missing test cases for invariants

- Deterministic first-oversize-expert selection with ≥2 oversize entries
  (P1.2).
- Pass-ordering proof with an unreferenced oversize expert plus an earlier
  independently-failing event (P1.3).
- Layer-scoped lookup at the capacity level: the same `expert_id` on two
  layers with different sizes, where only the event's layer busts the budget.
  Covered today by `active_set_bytes` tests, but a capacity-level case guards
  the delegation against future reimplementation.
- Non-empty valid manifest with an empty event iterator and adequate budget →
  `Ok(())` (the expert-pass-only success path; the plan only covers empty
  input with a zero budget).
- When asserting `ActiveSetBytes` / `ActiveSetExceedsGlobalCapacity`, match
  all context fields (`event_index`, `request_id`, `layer_id`), not just the
  variant, so the error-context contract is pinned.

## 7. Change requests

Not applicable — the decision is Approve. The three P1 items are the required
amendments to fold into this slice; none require changing the proposed API
shape, error taxonomy, or validation order.
