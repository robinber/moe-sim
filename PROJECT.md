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

## Worked exercise: `no-cache` vs LRU vs LFU

This exercise starts with observable data, asks for a prediction, and then
checks that prediction against the simulator.

### Question

Three experts each occupy 2 bytes. The cache budget is 4 bytes, so only two
experts can be resident at once. The trace requests experts in this order:

```text
0, 0, 1, 2, 0
```

Before running the commands, predict:

1. How many loads and hits will each policy report?
2. Which expert will LRU evict when expert `2` arrives?
3. Which expert will LFU evict instead?
4. Which policy will still contain expert `0` at the final event?

### Input trace

From the repository root, ensure the ignored working directory exists:

```bash
mkdir -p target
```

Create `target/learning-trace.jsonl` with the following content. This is JSONL:
each line is one complete JSON event, not an element in a surrounding JSON
array.

```json
{"request_id":1,"phase":"decode","step_id":0,"token_position":0,"layer_id":0,"expert_ids":[0]}
{"request_id":1,"phase":"decode","step_id":1,"token_position":1,"layer_id":0,"expert_ids":[0]}
{"request_id":1,"phase":"decode","step_id":2,"token_position":2,"layer_id":0,"expert_ids":[1]}
{"request_id":1,"phase":"decode","step_id":3,"token_position":3,"layer_id":0,"expert_ids":[2]}
{"request_id":1,"phase":"decode","step_id":4,"token_position":4,"layer_id":0,"expert_ids":[0]}
```

The fields have distinct roles:

- `request_id` groups the events into one request;
- `phase` says these events belong to decoding;
- `step_id` and `token_position` preserve their source positions;
- `layer_id` selects the layer whose experts are accessed;
- `expert_ids` is the atomic active set for that event.

### Size manifest

Create `target/learning-manifest.json` with the following content:

```json
{
  "experts": [
    {"layer_id": 0, "expert_id": 0, "size_bytes": 2},
    {"layer_id": 0, "expert_id": 1, "size_bytes": 2},
    {"layer_id": 0, "expert_id": 2, "size_bytes": 2}
  ]
}
```

The trace provides the access order; the manifest provides the memory cost.
With `--global-budget-bytes 4`, every individual event fits and the cache can
retain two experts.

### Run the three policies

Build the CLI, then run the same inputs and budget under each policy:

```bash
cargo build
```

```bash
./target/debug/moe-sim run \
  --trace target/learning-trace.jsonl \
  --model-manifest target/learning-manifest.json \
  --global-budget-bytes 4 \
  --policy no-cache
```

```bash
./target/debug/moe-sim run \
  --trace target/learning-trace.jsonl \
  --model-manifest target/learning-manifest.json \
  --global-budget-bytes 4 \
  --policy lru
```

```bash
./target/debug/moe-sim run \
  --trace target/learning-trace.jsonl \
  --model-manifest target/learning-manifest.json \
  --global-budget-bytes 4 \
  --policy lfu
```

### Replay it by hand

In the LRU column, brackets list resident experts from least to most recently
used. In the LFU column, `expert:frequency` shows each resident expert's access
count.

| Event | Requested expert | `no-cache` | LRU state after event | LFU state after event |
| --- | ---: | --- | --- | --- |
| 1 | `0` | cold load, then release | cold load → `[0]` | cold load → `{0:1}` |
| 2 | `0` | reload, then release | hit → `[0]` | hit → `{0:2}` |
| 3 | `1` | cold load, then release | cold load → `[0, 1]` | cold load → `{0:2, 1:1}` |
| 4 | `2` | cold load, then release | evict `0`, load `2` → `[1, 2]` | evict `1`, load `2` → `{0:2, 2:1}` |
| 5 | `0` | reload, then release | evict `1`, reload `0` → `[2, 0]` | hit → `{0:3, 2:1}` |

LRU evicts expert `0` at event 4 because it was used less recently than expert
`1`. LFU instead keeps expert `0`, whose resident frequency is 2, and evicts
expert `1`, whose frequency is only 1.

### Expected metrics

The reports also contain versions, input paths, and SHA-256 provenance. The
policy-dependent metrics are:

| Metric | `no-cache` | LRU | LFU |
| --- | ---: | ---: | ---: |
| `events` | 5 | 5 | 5 |
| `object_loads` | 5 | 4 | 3 |
| `byte_loads` | 10 | 8 | 6 |
| `object_hits` | 0 | 1 | 2 |
| `byte_hits` | 0 | 2 | 4 |
| `object_reloads` | 2 | 1 | 0 |
| `byte_reloads` | 4 | 2 | 0 |
| `evictions` | 0 | 2 | 1 |
| `evicted_bytes` | 0 | 4 | 2 |
| `peak_resident_bytes` | 2 | 4 | 4 |

### What the result means

- `no-cache` is the retention-free baseline. It releases expert `0` after each
  event, so both later uses are reloads. Releases are not counted as evictions.
- LRU uses only recency. The short access to expert `1` makes the previously hot
  expert `0` the least recent entry, so loading expert `2` removes it.
- LFU uses resident access frequency before recency. It remembers that expert
  `0` was used twice and keeps it for the final hit.

This trace deliberately favors LFU. It demonstrates how the policies differ;
it does not prove that LFU is generally better than LRU.

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
