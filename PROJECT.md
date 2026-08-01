# Project Card — `moe-sim`

Last updated: 2026-08-01

## In one sentence

`moe-sim` is a deterministic logical simulator that replays Mixture-of-Experts
(MoE) activations to compare cache policies under a byte-denominated memory
budget.

## Why this project exists

The project studies a bounded question without building an inference engine:
given the same activation trace and expert sizes, how do cache policies affect
loads, hits, evictions, and churn?

It is a reproducible test bench that separates cache-policy logic from the many
other costs involved in real inference.

## Current state

- Version: `v0.1` complete.
- Status: deliberately parked at its useful stopping point.
- What works: synthetic trace generation and reading, capacity validation,
  `no-cache`, LRU, LFU, and Belady-reference replay, global budgets or fixed
  per-layer quotas, policy comparison, text/JSON/CSV output, and input
  provenance.
- Not planned by default: model execution, physical storage, latency
  measurement, prefetching, CUDA, multi-node systems, or an HTTP service.
- Last stopping point: every Milestone 1 criterion is complete. There is no
  implicit next task.

The [README](README.md) describes the product and its commands. The
[ROADMAP](ROADMAP.md) preserves possible explorations as an idea archive, not
as unfinished work.

## Mental model

The main flow is:

```text
activation trace + size manifest + budget + scope + policy
                              |
                              v
                 validate before simulation
                              |
                              v
                    replay in file order
                              |
                              v
             object/byte metrics + input provenance
```

A trace says **which experts are requested and in what order**. The manifest
says **how much memory each expert costs**. The policy chooses which experts to
evict, while the scope determines whether the budget is global or divided into
per-layer quotas.

## Architecture

- [`crates/moe-sim-core`](crates/moe-sim-core) contains domain types,
  validation, synthetic generation, policies, and pure replay logic.
- [`crates/moe-sim-cli`](crates/moe-sim-cli) contains file adapters, command-line
  handling, provenance, and report rendering.
- [`fixtures`](fixtures) contains small deterministic valid and invalid cases
  used by tests and examples.

The dependency points from the CLI to the core. The core remains synchronous,
deterministic, and independent of the filesystem, async runtimes, and inference
engines.

## Important contracts

- Events are replayed in file order; their metadata never reorders them
  implicitly.
- The unique experts in one event form an atomic active set. They must fit in
  memory together and remain pinned for the duration of the event.
- An impossible configuration is rejected before any result is produced.
- Resident bytes never exceed the global budget or the applicable layer quota.
- Object and byte metrics remain separate.
- A release is not an eviction. Churn measures reloads, not merely cache exits.
- The same input and configuration must produce the same machine-readable
  report.
- Belady is an offline reference for uniform-size experts. The general
  variable-size case is not presented as optimal.

## Reusable building blocks

| Building block | Location | Useful contract | Possible reuse |
| --- | --- | --- | --- |
| Canonical event | [`trace.rs`](crates/moe-sim-core/src/trace.rs) | Explicit phase, valid active set, duplicate rejection | Adapter from a real MoE routing trace |
| Size manifest | [`manifest.rs`](crates/moe-sim-core/src/manifest.rs) | One positive size per `(layer_id, expert_id)`, typed errors, capacity validation | Any simulator mapping logical objects to a memory cost |
| Cache scope | [`scope.rs`](crates/moe-sim-core/src/scope.rs) | Global budget or fixed per-layer quotas, independent of policy | Comparing fixed partitioning with a shared cache |
| Replay engine | [`replay.rs`](crates/moe-sim-core/src/replay.rs) | Deterministic replay, atomic pinning, precise accounting, `no-cache`/LRU/LFU/Belady | Test bench for new policies or traces |
| Synthetic generator | [`synthetic.rs`](crates/moe-sim-core/src/synthetic.rs) | Bounded reproducible cases, explicit seed for randomness | Producing fixtures and adversarial workloads |
| Provenance | [`provenance.rs`](crates/moe-sim-cli/src/provenance.rs) | Contract versions and input SHA-256 digests in every report | Reproducible reporting in other CLI tools |
| Bounded exhaustive oracle | [`belady_oracle.rs`](crates/moe-sim-core/tests/belady_oracle.rs) | Exact reference only for tiny cases | Checking a heuristic against a test oracle |

These are conceptual reuse boundaries first. Do not automatically extract them
into new crates: start with a local dependency on `moe-sim-core`, then extract
only if a second consumer confirms that the contract is genuinely shared.

## What this project helps recover

- How to separate a pure Rust core from CLI and file adapters.
- How to model invariants with validated constructors and typed errors instead
  of implicit conventions.
- How to make an experiment reproducible with fixtures, seeds, and input
  digests.
- How to distinguish object hits, byte hits, loads, evictions, residency, and
  reloads.
- How to test a heuristic against a deliberately bounded exact oracle.
- How to finish an exploratory project cleanly without turning optional ideas
  into roadmap debt.

## Pitfalls already handled

- Confusing a logical simulator with a real inference engine.
- Using object hit rate to hide the cost of differently sized experts.
- Evicting an expert that belongs to the current active set.
- Producing partial results before discovering an invalid configuration.
- Presenting Belady as optimal outside its objective and assumptions.
- Summing per-layer peaks even though they may occur at different times.
- Adding storage, async code, or new crates before a measured need justifies
  those boundaries.

## How to run it again

From the repository root:

```bash
cargo build
cargo test --workspace --all-features
```

Then run a reproducible comparison using the committed fixtures:

```bash
./target/debug/moe-sim compare \
  --trace fixtures/synthetic/three-experts-cycle.jsonl \
  --model-manifest fixtures/models/three-experts-uniform.json \
  --global-budgets-bytes 2,4,6 \
  --policies no-cache,lru,lfu,belady
```

The complete quality gates and expected tool versions are documented in
[`AGENTS.md`](AGENTS.md).

## If the project is reopened

1. State the single question the exploration should answer.
2. Check whether the existing functionality can answer it more cheaply.
3. Define one bounded slice and its stopping condition.
4. Re-read the README invariants and the parked ROADMAP explorations.
5. Run the reference tests before making changes.
6. At the end, update this card with the outcome, the new stopping point, and
   any building blocks that became genuinely reusable.
