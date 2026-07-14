# moe-sim

`moe-sim` is a trace-driven simulator for studying cache behavior in
single-node, out-of-core Mixture-of-Experts (MoE) inference.

The project answers a deliberately narrow question:

> Given an expert-activation trace, explicit expert sizes, and a memory budget,
> how do different caching policies affect misses, bytes loaded, and cache
> churn?

The first releases will answer this question with a deterministic logical
simulator. Storage timing, prefetching, compute overlap, and physical replay are
later validation stages, not assumptions built into the initial result.

## Status

**M0.1 slice.** This branch adds `moe-sim-core`, the first crate, with the
canonical activation event types and atomic-set validation (see roadmap).

## Why this project

MoE models activate only a subset of their routed experts for each token and
layer. That sparsity makes out-of-core execution possible, but usefulness
depends on locality, expert size, memory capacity, storage layout, and the cost
of bringing missing experts into memory.

Existing runtimes and research simulators often combine several of these
concerns. `moe-sim` aims to provide a small, standalone test bench where policy
comparisons use the same trace, memory accounting, and objectives.

The project is not an inference engine. It is a tool for deciding which ideas
are worth validating in one.

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

One real dataset adapter is planned for `v0.2`, after its source format, access
conditions, and reproducibility requirements have been audited. Supporting two
real datasets is not a prerequisite for the first releases.

## Non-goals for the first releases

The initial project will not:

- execute model weights or tokens;
- implement attention, KV caches, tokenizers, CUDA kernels, or an HTTP server;
- claim end-to-end inference latency;
- infer missing prefill/decode boundaries;
- model distributed or multi-node systems;
- implement learned predictors;
- expose a dynamic plugin ABI;
- create a large research trace dataset;
- claim cycle-accurate storage simulation.

These exclusions keep the first result testable on a normal development
machine and prevent the simulator from becoming an inference runtime.

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
bytes. Later versions may add file offsets, alignment, packing, compression,
and alternative precisions.

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

## Storage-aware work comes later

Logical misses will eventually be converted into physical read requests using
file offsets, alignment, and packing information. Only after those schedules
are correct will the project add:

1. measured device profiles;
2. a discrete-event storage queue;
3. physical schedule replay;
4. prefetch and demand-read priorities;
5. optional compute overlap.

Latency results must identify which inputs are measured, estimated, or
synthetic. No device profile may be presented as portable across machines.

## Development workflow

Development is intended to be orchestrated with Kira in supervised slices:

1. the orchestrator proposes a bounded plan;
2. the operator approves the scope;
3. one worker implements;
4. independent workers review correctness and reproducibility;
5. the orchestrator records evidence and pauses at the next gate.

A milestone may require several bounded workflow runs. Each slice uses one run
and must satisfy its own gate; the milestone closes only after all exit criteria
in [ROADMAP.md](ROADMAP.md) are demonstrated. Publishing, merging, scope
expansion, and irreversible actions remain operator decisions.

## Research positioning

The intended contribution is a reusable framework for fair, trace-driven
comparison of MoE cache policies under explicit byte budgets, later extended
with calibrated single-node storage behavior.

Any claim of novelty must be re-evaluated against current literature before
publication. Early releases should make narrower engineering claims about
correctness, reproducibility, and measured validation.

## License

`moe-sim` is licensed under the [MIT License](LICENSE). Dataset licenses and
access terms remain independent from the source-code license.
