# Cross-review: M0 global-capacity validation

## 1. Verdict: Request-changes

The capacity behavior matches the approved design after one build-blocking P0
was restored during this review. The requested workspace tests, Clippy, and
rustdoc pass, but the canonical nightly rustfmt check still fails. The branch
therefore needs formatting before it is PR-ready.

## 2. Correctness vs design

- `validate_global_capacity` is pure and performs the manifest pass before the
  event pass.
- The manifest pass uses the `BTreeMap` key order, checks unreferenced experts,
  and returns the first oversize `(layer_id, expert_id)` deterministically.
- The event pass consumes caller-supplied order without consulting event
  metadata, delegates layer-scoped sums to `active_set_bytes`, and preserves
  event index, request, and layer context.
- Exact fit is accepted because both checks use `>`, not `>=`.
- Empty active sets remain valid at zero bytes. A non-empty positive-size
  manifest fails a zero budget during the manifest pass.
- Unknown experts and `u64` active-set overflow remain typed failures. There is
  no partial admission, cache bypass, residency behavior, or policy logic.
- The strict unreferenced-expert rule and its real-data-adapter revisit marker
  are documented in both the design and API rustdoc.

No remaining correctness deviation from the approved M0 design was found.

## 3. Test gaps

The 26 unit tests cover exact fit, zero-budget boundaries, unreferenced and
referenced oversize experts, deterministic first-key selection, independent
manifest-pass priority, unknown experts, overflow, event order, complete error
context, and layer-scoped active-set sizes.

P2 gap: the tests compare the nested `source` field but do not directly assert
the `std::error::Error::source()` chain exposed by `thiserror`. A small test
would pin the documented source-preservation contract, but this does not block
M0 correctness.

## 4. API/docs issues

- `CapacityError` has the approved `Debug`, `Clone`, `PartialEq`, `Eq`, and
  `thiserror::Error` derives and is re-exported from the crate root.
- Error variants carry actionable byte values and event/model identity. The
  `ActiveSetBytes` rustdoc correctly limits runtime sources to unknown-expert
  and overflow failures.
- P2: the zero-budget rustdoc at `manifest.rs:269` says an empty manifest “(or
  empty active sets)” is valid. Because the manifest pass runs first, the
  precise wording is: the manifest must be empty, and supplied events may then
  contain only empty active sets.
- Deferring the CLI is consistent with the approved stop rule and
  `docs/m0-impl-notes.md`; no unsupported CLI or fixture format is claimed as
  implemented.

Verification evidence:

- `cargo test --workspace`: initially failed because `CapacityError` was
  absent; after the authorized P0 restoration, passed 26 unit tests and 0
  doctests.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps`:
  passed.
- `cargo +nightly fmt --all --check`: failed with formatting diffs in
  `lib.rs` and `manifest.rs`.

## 5. P0 / P1 / P2 list

### P0

- **Fixed during review:** `CapacityError` was absent from `manifest.rs` while
  `lib.rs`, `validate_global_capacity`, and tests referenced it, so the crate
  did not compile. The approved enum definition, derives, messages, and
  rustdoc were restored at `manifest.rs:63-124`. No other production behavior
  was edited.

### P1

- Run `cargo +nightly fmt --all` and confirm
  `cargo +nightly fmt --all --check` passes. The current formatting gate
  reports diffs in the crate-root re-export, validation rustdoc wrapping, and
  several test expressions.

### P2

- Clarify the zero-budget rustdoc wording described in section 4.
- Optionally assert the `std::error::Error::source()` chain for
  `CapacityError::ActiveSetBytes`.

## 6. PR-ready? no

Correctness, tests, Clippy, and rustdoc are green after the P0 restoration,
but the repository's canonical formatting gate must pass before review-ready
delivery.
