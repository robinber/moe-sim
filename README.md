# moe-sim

`moe-sim` is a trace-driven simulator for studying cache behavior in
single-node, out-of-core Mixture-of-Experts (MoE) inference.

The project answers a deliberately narrow question:

> Given an expert-activation trace, explicit expert sizes, and a memory budget,
> how do different caching policies affect misses, bytes loaded, and cache
> churn?

The default path answers this question with a **deterministic logical**
simulator on synthetic traces. Storage timing, prefetching, compute overlap,
and physical replay are **optional later experiments**, not part of the
required journey — see [ROADMAP.md](ROADMAP.md).

## Status

**Exploratory / pre-`v0.1`.** `moe-sim-core` has canonical activation events
(atomic-set validation) and an explicit expert-size `ModelManifest`. The
intended useful stop is Milestone 1 (`v0.1`): logical cache comparison under
byte budgets. Everything beyond that is optional curiosity, not a tunnel to
finish.

## Why this project

MoE models activate only a subset of their routed experts for each token and
layer. That sparsity makes out-of-core execution possible, but usefulness
depends on locality, expert size, memory capacity, storage layout, and the cost
of bringing missing experts into memory.

`moe-sim` is a **small personal lab bench**: same traces, same memory
accounting, same objectives — so policy ideas can be compared honestly without
standing up an inference runtime.

The project is not an inference engine, not a multi-year research program, and
not obligated to reach a “complete” simulator. Stopping after a clean `v0.1`
is a successful outcome.

## Feasible first release

The first useful release (`v0.1`) is limited to:

- a canonical activation-event format;
- deterministic synthetic traces;
- explicit expert sizes;
- byte-constrained global and per-layer caches;
- no-cache, LRU, and LFU policies;
- classic Belady MIN for uniform-size traces;
- a bounded exact oracle for tiny variable-size test cases;
- object-hit, byte-hit, load, eviction, and churn metrics;
- single-run and policy-comparison commands;
- human-readable, JSON, and CSV reports.

A real dataset adapter (`v0.2`-shaped work) is an **optional** exploration after
`v0.1`, only if synthetic results still leave an interesting question. It is
not on the critical path.

## Non-goals (default path)

Unless an optional exploration deliberately reopens them, this project will
not:

- execute model weights or tokens;
- implement attention, KV caches, tokenizers, CUDA kernels, or an HTTP server;
- claim end-to-end inference latency;
- infer missing prefill/decode boundaries;
- model distributed or multi-node systems;
- implement learned predictors;
- expose a dynamic plugin ABI;
- host large research trace datasets in-repo;
- claim cycle-accurate storage simulation;
- treat later roadmap items as mandatory follow-ups.

These exclusions keep exploration cheap on a normal development machine and
prevent the simulator from becoming an inference runtime or a process tunnel.

## Core inputs

### Activation trace

A canonical event records the minimum information needed to replay routed
expert accesses:

```text
request_id
phase: prefill | decode | unknown
step_id
token_position
layer_id
expert_ids
```

Optional source metadata may be preserved, but policies must declare which
fields they require. Missing phase information remains `unknown`; it is never
guessed from token position.

Canonical events are replayed in file order. `step_id`, `token_position`, and
`layer_id` validate source structure but do not silently reorder events.
`expert_ids` is one atomic active set: all unique experts in the event must be
resident together, remain pinned until that event completes, and are then
released before the next event. Duplicate expert identifiers in one event are
invalid.

### Model manifest

The first manifest maps each `(layer_id, expert_id)` pair to its stored size in
bytes. Sizes must be strictly positive; duplicate keys are invalid; lookups for
undeclared experts fail rather than inventing a size. Later versions may add
file offsets, alignment, packing, compression, and alternative precisions.

Separating activation order from expert size prevents object-hit rate from
hiding expensive misses on larger experts.

### Run configuration

A run selects:

- one trace;
- one model manifest;
- one total memory budget;
- one cache scope: global or per-layer;
- one policy;
- one deterministic seed where a policy needs randomness.

Policy and scope are independent. A global cache may use the full budget. A
per-layer cache requires an explicit quota for every simulated layer; quotas
must sum to no more than the total budget and unused quota is not shared in
`v0.1`. The same LRU or LFU policy runs within the selected scope.

If one expert exceeds its applicable capacity, or an atomic active set cannot
fit in its applicable cache, the configuration is rejected. The simulator does
not bypass the cache or partially execute the event.

Every report records its inputs and their versions or checksums.

## Planned CLI

The command names below describe the intended interface; they are not available
yet.

```bash
moe-sim trace inspect \
  --trace fixtures/synthetic/repeating.jsonl

moe-sim run \
  --trace fixtures/synthetic/repeating.jsonl \
  --model-manifest fixtures/models/tiny.toml \
  --memory-budget 64mb \
  --cache-scope global \
  --policy lru

moe-sim compare \
  --trace fixtures/synthetic/repeating.jsonl \
  --model-manifest fixtures/models/tiny.toml \
  --memory-budgets 16mb,32mb,64mb \
  --cache-scope global \
  --policies lru,lfu,belady
```

## Correctness principles

The logical simulator must satisfy these invariants before storage timing is
introduced:

- deterministic inputs produce deterministic outputs;
- resident bytes never exceed the configured budget;
- an atomic active set is either fully resident or rejected as incompatible;
- experts in the current active set cannot be evicted;
- malformed traces and manifests fail with actionable errors;
- prefill groups and decode steps remain distinguishable when provided;
- classic Belady is tested against exhaustive uniform-size cases;
- variable-size policies are tested against bounded exact cases, and no greedy
  size-aware variant is labeled globally optimal;
- byte and object metrics are reported separately;
- unsupported policy/input combinations are rejected rather than approximated
  silently.

## Architecture

The project will start as a small Rust workspace:

```text
moe-sim/
  Cargo.toml
  crates/
    moe-sim-core/    pure replay, cache policies, metrics, domain errors
    moe-sim-cli/     file adapters, commands, report rendering
  fixtures/
    synthetic/
    models/
  docs/
  README.md
  ROADMAP.md
```

This is intentionally smaller than a crate-per-concept architecture. A new
crate is justified only when a boundary has multiple consumers, needs isolated
verification, or requires dependencies that the pure core should not inherit.

The core remains synchronous and independent of storage or async runtimes until
the storage-simulation milestone establishes a real need.

## Offline baselines

Classic Belady MIN is optimal for uniform-size pages. Once expert objects have
different sizes or fetch costs, the general offline caching problem is
NP-hard. `moe-sim` will therefore not describe a scalable greedy
"byte-aware Belady" implementation as an optimum.

`v0.1` will use classic Belady on uniform-size fixtures and a deliberately
bounded exhaustive solver for tiny variable-size correctness tests. Practical
bounds for large variable-size traces are a separate research feature. See
[Practical Bounds on Optimal Caching with Variable Object Sizes](https://arxiv.org/abs/1711.03709)
and [General Caching Is Hard: Even with Small Pages](https://arxiv.org/abs/1506.07905).

## Dataset strategy

Candidate sources include:

- [MoE-Beyond](https://github.com/ngavhane/moe-beyond), after its trace format
  is audited;
- [Patterns behind Chaos / MoE expert-selection traces](https://huggingface.co/datasets/core12345/MoE_expert_selection_trace),
  after gated access, revision pinning, and subset selection are resolved.

Large or gated traces will not be committed. Reproducible experiments will
identify the dataset revision, selected files, checksums, adapter version, and
transformations applied.

## Storage-aware work (optional, not scheduled)

Layout-aware reads, device profiles, discrete-event queues, physical replay,
prefetch, and compute overlap live in the roadmap as **optional explorations**.
They are interesting research toys if curiosity remains after `v0.1` — not a
promised second act.

If latency experiments ever happen, every result must label inputs as measured,
estimated, or synthetic. No device profile may be presented as portable across
machines.

## Development workflow

Prefer small, bounded slices:

1. pick one slice with a clear stop condition;
2. implement and review;
3. close the gate with tests or fixtures that prove it;
4. **consciously** continue, side-quest, or pause — do not auto-advance the
   roadmap.

Chaining work only to “complete the roadmap” is out of scope for an exploratory
project.

## Positioning

The intended artifact is a reusable, honest comparison bench for MoE cache
policies under explicit byte budgets — useful first for the author, optionally
for others.

Early work should claim correctness and reproducibility on synthetic fixtures,
not novelty. Any publication-shaped claim needs a fresh prior-art review; none
of that is required to enjoy or stop the project.

## License

`moe-sim` is licensed under the [MIT License](LICENSE). Dataset licenses and
access terms remain independent from the source-code license.
