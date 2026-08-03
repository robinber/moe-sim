# Project history

**Authority** for completed Milestone 0 / Milestone 1 deliverables and gates.
This is a closed record, not an active plan.

**Final decision:** Milestone 0 and every Milestone 1 slice (1A–1D) are
complete. The operator parked the project at the recommended `v0.1` boundary.
There is no next planning target. See [ideas.md](ideas.md) for parked
explorations that were never scheduled.

## Path taken

```text
M0  contracts & workspace     ← complete
        ↓
M1  logical cache sim (v0.1)  ← complete
        ↓
    project parked            ← final decision
```

**Primary goal (met):** a small deterministic logical simulator on synthetic
traces with fair policy comparison under byte budgets.

**Never a goal of the default path:** storage timing, physical replay,
multi-device calibration, public research release, or becoming an inference
engine.

## Milestone 0 — Repository and contracts

**Status:** complete.
**Outcome:** a small Rust workspace with stable initial contracts.

### Deliverables

- [x] Initialize Git.
- [x] Use the MIT License for the source code.
- [x] Create `moe-sim-core`.
- [x] Create a thin `moe-sim-cli`.
- [x] Define the canonical activation event and phase semantics.
- [x] Define file-order replay and atomic active-set semantics.
- [x] Define the minimal expert-size manifest.
- [x] Define oversize expert and oversize active-set rejection.
- [x] Define run provenance fields. Every success report names the tool
  version, the input contract version, and a SHA-256 digest of each input
  document. Deterministic seed handling was deferred to slice 1D and closed
  there: `trace generate --pattern random` became the first stochastic
  consumer, and its seed is recorded in the generation report beside the other
  provenance fields.
- [x] Add tiny valid and invalid fixtures.
- [x] Document error behavior and compatibility rules.
- [x] Add impact-scoped format, Clippy, test, and rustdoc checks.
- [ ] Optional (never required): pin a reference CI runner if/when resource
  gates matter. Left open intentionally; not part of `v0.1` exit criteria.

### Exit criteria (met)

- Canonical events round-trip without information loss.
- Unknown phase remains explicit.
- Event order is file order and is never reconstructed silently from metadata.
- Each event's expert IDs form one atomic pinned set.
- Configurations where one expert or active set exceeds capacity fail before
  simulation results are emitted.
- Duplicate or out-of-range expert identifiers follow a documented rule.
- Malformed traces and manifests return typed, actionable errors.
- Public items are documented and the narrow Rust quality gates pass.

### Explicitly deferred from M0

Cache policies, real datasets, async I/O, plugins, storage profiles, and
benchmarks (policies and synthetic comparison closed in M1).

---

## Milestone 1 — Deterministic logical cache simulator (`v0.1`)

**Status:** complete — the intended useful stopping point was taken.
**Outcome:** reproducible methodology infrastructure on **synthetic** traces:
canonical events, deterministic replay, byte budgets, and fair policy
comparison.

`v0.1` is infrastructure for honest comparisons, not a claim of novelty or a
decision product. LRU is a reference baseline, not a presumed winner.

### Slice 1A — Replay and accounting

- [x] Sequential file-order event replay.
- [x] Atomic pin / use / release behavior.
- [x] Byte-accurate residency and eviction accounting. A release after use is
  not an eviction: eviction is capacity-driven removal of a retained object, so
  the no-cache baseline evicts nothing and stays comparable with 1B.
- [x] No-cache baseline.
- [x] `trace inspect` and a text-only `run` command.

**Gate:** hand-calculated no-cache and residency fixtures match exactly.

### Slice 1B — Online policies and cache scopes

- [x] LRU and LFU (policy independent of scope). A policy decides only which
  unpinned object is evicted next, so the same policy applies unchanged when a
  scope other than the global budget is added.
- [x] One global budget.
- [x] Explicit fixed per-layer quotas (sum ≤ total budget). Delivered as the
  split-out second half of this slice: one independent cache per quota'd
  layer, unused quota not shared, and the gate below re-demonstrated per
  layer on adversarial fixtures.
- [x] Object hits, byte hits, loads, evictions, resident bytes, churn. Churn is
  **rework**: loads of an expert loaded earlier in the run and no longer
  resident, so loads split into cold misses plus reloads. Turnover alone would
  duplicate the eviction counter. Churn counts the rework, not its cause: a
  retaining policy loses residency by eviction, no-cache by release, so the
  baseline reports reloads with zero evictions.

**Gate:** every policy respects atomic pinning and byte capacity on adversarial
fixtures.

### Slice 1C — Offline correctness references

- [x] Classic Belady MIN for uniform-size traces.
- [x] Bounded exhaustive oracle for tiny variable-size cases only.
- [x] Label every offline result with objective and applicability.

**Gate:** Belady matches exhaustive uniform-size cases; the bounded solver
checks tiny variable-size optima. Do **not** brand a scalable greedy
size-aware policy as globally optimal (general variable-size caching is
NP-hard; see references in [contracts.md](contracts.md)).

### Slice 1D — Comparison and outputs

- [x] `compare` command.
- [x] Text, JSON, and CSV output.
- [x] Deterministic synthetic patterns (repetition, random, cyclic, hotset
  shifts, variable sizes, adversarial LRU).
- [x] Deterministic seed handling, deferred here from M0: the random pattern is
  the first stochastic consumer. Record the seed in report provenance beside
  the existing tool, contract, and digest fields.
- [x] Optional stress fixture (e.g. ~100k events) for local regression — keep
  it regenerable; do not bloat the repo.

**Gate:** repeated runs on the same inputs produce identical machine-readable
reports; capacity and pinning invariants hold; object vs byte metrics stay
separate.

### Milestone exit criteria (all met for `v0.1`)

- Deterministic logical replay under byte budgets.
- No-cache, LRU, LFU (+ offline refs on appropriate fixtures).
- Clear rejection of impossible configs (oversize expert / active set).
- Human + machine-readable reports with input provenance.
- Documentation explains how to reproduce one synthetic comparison from a
  clean checkout.

## Stance while the project was active

These principles guided the work; they remain useful if the repo is reopened:

| Principle | Meaning |
| --- | --- |
| **Default horizon is short** | Aim at M0 → M1 (`v0.1`). That alone is a complete artifact. |
| **Later work is optional** | Anything after `v0.1` is curiosity-driven. Skip it freely. |
| **Stopping is success** | Pausing or archiving after a clean slice is a valid outcome. |
| **No sunk-cost ladder** | Finishing a slice does not create an obligation to start the next. |
| **No calendar commitment** | No deadlines or “we must reach v1.0” goals. |
| **Narrow over complete** | Prefer one reproducible comparison to a broad unfinished simulator. |
