# moe-sim roadmap

## Project stance (read this first)

`moe-sim` is an **exploratory** project. It exists to learn, poke at ideas, and
keep a small, honest test bench for MoE cache questions — not to become a
multi-year research program or a production inference stack.

This document is a **menu of useful stopping points and optional directions**,
not a tunnel you are expected to walk end-to-end.

| Principle | Meaning |
| --- | --- |
| **Default horizon is short** | Aim at Milestone 0 → Milestone 1 (`v0.1`). That alone is a complete, useful artifact. |
| **Later work is optional** | Anything after `v0.1` is curiosity-driven. Skip it freely. |
| **Stopping is success** | Pausing, archiving, or switching to another project after a clean slice is a valid outcome. |
| **No sunk-cost ladder** | Finishing a slice does **not** create an obligation to start the next one. |
| **No calendar commitment** | There are no deadlines, release trains, or “we must reach v1.0” goals. |
| **Narrow over complete** | Prefer one reproducible comparison to a broad unfinished simulator. |

If energy or interest drops, **ship or stop at the last closed milestone** and
leave the rest in this backlog. Do not expand scope to “finish the roadmap.”

---

## Intended path (default)

```text
M0  contracts & workspace     ← in progress / near
        ↓
M1  logical cache sim (v0.1)  ← primary useful stop
        ↓
    [optional] explore further, park the repo, or move on
```

**Primary goal:** a small deterministic logical simulator on synthetic traces
with fair policy comparison under byte budgets.

**Not a goal of the default path:** storage timing, physical replay, multi-device
calibration, public research release, or becoming an inference engine.

---

## Working rules (lightweight)

Keep these even in exploratory mode — they protect honesty, not process theater:

- One vertical slice at a time; prefer small merges over epic branches.
- Keep the logical simulator independent of storage timing.
- Treat every approximation as **named** input data, not a silent default.
- Reject unsupported comparisons instead of inventing missing metadata.
- Keep large traces, generated results, and machine-specific profiles out of
  git unless they are tiny intentional fixtures.
- Do not create a new crate until a real dependency or ownership boundary needs it.
- Do not publish latency claims without a measured, documented basis.

Heavy preregistration protocols, multi-device calibration gates, and
publication-grade statistical procedures are **only** relevant if you later
choose a research-shaped exploration — they are not required for exploratory
engineering toward `v0.1`.

---

## Milestone 0 — Repository and contracts

**Status:** exploratory foundation.  
**Outcome:** a small Rust workspace with stable initial contracts and **no**
simulation claims yet.

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
  document. Deterministic seed handling is **deferred** to slice 1D, whose
  random synthetic pattern is the first stochastic consumer: no command is
  stochastic today, and a seed field with no consumer would be untestable
  scaffolding.
- [x] Add tiny valid and invalid fixtures.
- [x] Document error behavior and compatibility rules.
- [x] Add impact-scoped format, Clippy, test, and rustdoc checks.
- [ ] Optional: pin a reference CI runner if/when resource gates matter.

### Exit criteria

- Canonical events round-trip without information loss.
- Unknown phase remains explicit.
- Event order is file order and is never reconstructed silently from metadata.
- Each event's expert IDs form one atomic pinned set.
- Configurations where one expert or active set exceeds capacity fail before
  simulation results are emitted.
- Duplicate or out-of-range expert identifiers follow a documented rule.
- Malformed traces and manifests return typed, actionable errors.
- Public items are documented and the narrow Rust quality gates pass.

### Explicitly deferred

Cache policies, real datasets, async I/O, plugins, storage profiles, and
benchmarks.

### Stop / continue

Closing M0 is enough to keep playing with types and fixtures. Continue to M1
only if you still want a runnable policy comparison.

---

## Milestone 1 — Deterministic logical cache simulator (`v0.1`)

**Status:** **default exploratory target** — the intended “ship and maybe stop.”  
**Outcome:** reproducible methodology infrastructure on **synthetic** traces:
canonical events, deterministic replay, byte budgets, and fair policy
comparison.

`v0.1` is infrastructure for honest comparisons, not a claim of novelty or a
decision product. LRU is a reference baseline, not a presumed winner. Recent
related work includes
[Cache Management for Mixture-of-Experts LLMs](https://arxiv.org/abs/2509.02408)
and
[In-depth Analysis on Caching and Pre-fetching in Mixture of Experts Offloading](https://arxiv.org/abs/2511.05814).
Any stronger claim than “reproducible comparison tool” needs a fresh prior-art
pass — optional if you are not publishing.

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
- [ ] Explicit fixed per-layer quotas (sum ≤ total budget). Split out as the
  remaining half of this slice: it adds a configuration surface rather than
  eviction logic, and the gate below is demonstrable without it.
- [x] Object hits, byte hits, loads, evictions, resident bytes, churn. Churn is
  **rework**: loads of an expert loaded earlier in the run and evicted since,
  so loads split into cold misses plus reloads. Turnover alone would duplicate
  the eviction counter.

**Gate:** every policy respects atomic pinning and byte capacity on adversarial
fixtures.

### Slice 1C — Offline correctness references

- [ ] Classic Belady MIN for uniform-size traces.
- [ ] Bounded exhaustive oracle for tiny variable-size cases only.
- [ ] Label every offline result with objective and applicability.

**Gate:** Belady matches exhaustive uniform-size cases; the bounded solver
checks tiny variable-size optima. Do **not** brand a scalable greedy
size-aware policy as globally optimal (general variable-size caching is
NP-hard; see
[Practical Bounds on Optimal Caching with Variable Object Sizes](https://arxiv.org/abs/1711.03709)
and
[General Caching Is Hard: Even with Small Pages](https://arxiv.org/abs/1506.07905)).

### Slice 1D — Comparison and outputs

- [ ] `compare` command.
- [ ] Text, JSON, and CSV output.
- [ ] Deterministic synthetic patterns (repetition, random, cyclic, hotset
  shifts, variable sizes, adversarial LRU).
- [ ] Deterministic seed handling, deferred here from M0: the random pattern is
  the first stochastic consumer. Record the seed in report provenance beside
  the existing tool, contract, and digest fields.
- [ ] Optional stress fixture (e.g. ~100k events) for local regression — keep
  it regenerable; do not bloat the repo.

**Gate (exploratory, lightweight):** repeated runs on the same inputs produce
identical machine-readable reports; capacity and pinning invariants hold;
object vs byte metrics stay separate.

Formal preregistered evaluation matrices, bootstrap CIs, and multi-seed policy
tournaments are **optional research hygiene**, not required to call `v0.1`
done for personal exploration.

### Milestone exit criteria (enough for `v0.1`)

- Deterministic logical replay under byte budgets.
- No-cache, LRU, LFU (+ offline refs on appropriate fixtures).
- Clear rejection of impossible configs (oversize expert / active set).
- Human + machine-readable reports with input provenance.
- README explains how to reproduce one synthetic comparison from a clean
  checkout.

### Stop / continue after `v0.1`

**This is the recommended default stopping point.**

You may:

- park the repo and move on;
- add one more policy or metric as a side quest;
- open an optional exploration below if curiosity is still high.

You should **not** treat M2–M7 as “what’s left to finish.”

---

## Optional explorations (not a sequence)

Everything below is a **backlog of directions**, not a pipeline. Pick zero or
one when interest is genuine. Reorder freely. Drop freely.

Each item should earn its keep with a one-line question, e.g.  
*“Does this teach me something I cannot learn cheaper another way?”*

### O1 — One real trace adapter (former M2 / `v0.2`)

**Question:** do synthetic conclusions transfer to one real activation trace?

- Audit one dataset (schema, ordering, access terms, pin-able revision).
- Stream into the canonical format; checksums + provenance.
- Same replay engine and policies as synthetic runs.
- Tiny redistributable or schema-equivalent fixture when redistribution is
  blocked.

**Skip if:** synthetic fixtures already answered your curiosity, or data access
is painful relative to learning value.

### O2 — Layout-aware read schedules (former M3)

**Question:** how much do file layout and alignment amplify logical misses?

- Offsets, alignment, packing → physical read schedules (bytes, not latency).
- Attribute amplification categories explicitly.
- No timing claims.

**Skip if:** you only care about logical hit/miss under a byte budget.

### O3 — Storage timing sketches (former M4–M5)

**Question:** can a simple device model change policy rankings?

- Only after O2 schedules exist and you still care.
- Mark profiles as measured / estimated / synthetic.
- Prefer a **small** experiment on one machine over multi-device science.

**Skip if:** latency is out of scope (default for exploratory mode).

### O4 — Prefetch / overlap toys (former M6)

**Question:** when does prefetch help vs pollute under explicit assumptions?

- Demand vs prefetch priorities; late/unused prefetch accounting.
- Compute profiles must be labeled; perfect overlap is never a silent default.

**Skip if:** you have not validated that timing models are worth the complexity.

### O5 — Hardening / shareable release (former M7)

**Question:** is this worth packaging for someone else to run?

- Stabilize only APIs you actually use.
- Document “add a policy / adapter” only if a second consumer exists.
- Re-evaluate prior art before any public research claim.

**Skip if:** the project remains a personal lab bench — that is fine.

---

## Explicit non-goals (default)

Unless an optional exploration deliberately reopens them:

- becoming an inference engine (weights, CUDA, tokenizers, HTTP servers);
- distributed / multi-node simulation;
- learned predictors as a core feature;
- dynamic plugin ABI;
- hosting large research datasets in-repo;
- cycle-accurate storage simulation;
- a commitment to publish papers or cut a long-lived `v1.0` product.

---

## Idea parking lot

Capture ideas here instead of expanding the active milestone:

- dynamic layer quotas;
- segmented LRU and larger policy catalogs;
- scalable offline bounds for variable-size caching;
- learned expert predictors;
- mixed-precision residency;
- compression-aware service models;
- production runtime integration;
- dashboards and hosted services.

Move an idea into an optional exploration only when you are ready to open a
**bounded** slice with a clear stop condition.

---

## How to work in slices

Keep development in **bounded slices**. This is process hygiene for an
exploratory project, not a mandate to industrialize the backlog.

Suggested rhythm:

1. Pick **one** slice (e.g. M1 / 1A) with a clear stop condition.
2. Keep the plan narrow enough to review in one sitting.
3. Implement + review.
4. Prove the gate with tests or hand-calculated fixtures; **stop** there.
5. Consciously choose: next slice, optional exploration, or pause the project.

Do not chain work only to “complete the roadmap.” Exploration dies when every
session is forced to advance a milestone counter.

---

## Next planning target

**Current focus:** finish remaining **Milestone 0** contracts (fixtures, CLI
stub, oversize rejection), then open **Milestone 1 slice 1A** only if still
interesting.

After any closed slice, the default next action is:

```text
decide: continue | side quest | pause
```

not

```text
automatically start the next milestone
```
