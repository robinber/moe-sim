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
(atomic-set validation), an explicit expert-size `ModelManifest`, and
deterministic replay with byte-accurate accounting under no-cache, LRU, LFU,
and an offline Belady reference whose results the test suite checks against a
bounded exhaustive oracle on enumerated tiny cases — within one global budget
or under explicit fixed per-layer quotas. The CLI generates deterministic
synthetic traces (`trace generate`) and compares policies across budget
sweeps (`compare`) in text, JSON, and CSV. Every Milestone 1 slice (1A–1D)
has shipped; declaring `v0.1` — the intended useful stop: logical cache
comparison under byte budgets — is the operator's next decision. Everything
beyond that is optional curiosity, not a tunnel to finish.

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
- one policy.

No shipped policy is stochastic, so runs take no seed. The only seed in the
tool belongs to synthetic input generation (`trace generate --pattern
random`) and is recorded in that command's report.

Policy and scope are independent. A global cache may use the full budget. A
per-layer cache requires an explicit quota for every simulated layer; quotas
must sum to no more than the total budget and unused quota is not shared in
`v0.1`. The same LRU or LFU policy runs within the selected scope.

If one expert exceeds its applicable capacity, or an atomic active set cannot
fit in its applicable cache, the configuration is rejected. The simulator does
not bypass the cache or partially execute the event.

Every report records its inputs and their versions or checksums.

## CLI

Five commands are implemented today: `trace inspect`, `trace generate`,
`capacity check`, `run`, and `compare`. All are flag-driven (no positional
arguments), take budgets as plain byte counts, print a deterministic report
to stdout on success, and report typed errors on stderr with a meaningful
exit code: `0` success, `2` bad arguments, `3` file I/O failure, `4` parse
failure, `5` capacity rejection, `6` replay failure. Failures never emit
partial stdout.

`capacity check` and `run` take `--cache-scope global|per-layer` (default
`global`). A per-layer scope additionally requires one repeated
`--layer-quota-bytes LAYER:BYTES` flag per simulated layer; quotas under a
global scope, a per-layer scope without quotas, or one layer quoted twice are
argument errors (exit code 2). Every report records the selected scope.

Every success report opens with provenance — the tool version, the input
contract version, and a SHA-256 digest of each input beside its path. The
digests are reproducible with `shasum -a 256 <path>`.

Inspect a committed trace fixture:

```bash
moe-sim trace inspect --trace fixtures/synthetic/active-set-0-1.jsonl
```

```text
status: ok
tool_version: 0.1.0
input_format: v1
trace: fixtures/synthetic/active-set-0-1.jsonl
trace_sha256: ba96fdf54901d5f93e090714c539b63aa748b1b845434a92522a77dee3744556
events: 2
requests: 1
layers: 1
expert_activations: 3
phase_prefill: 1
phase_decode: 1
phase_unknown: 0
```

Check capacity feasibility. The first event activates the atomic set `{0, 1}`
(4 B + 6 B = 10 B), so a 10-byte global budget is an exact fit:

```bash
moe-sim capacity check \
  --trace fixtures/synthetic/active-set-0-1.jsonl \
  --model-manifest fixtures/models/two-experts-4-6.json \
  --global-budget-bytes 10
```

```text
status: ok
tool_version: 0.1.0
input_format: v1
trace: fixtures/synthetic/active-set-0-1.jsonl
trace_sha256: ba96fdf54901d5f93e090714c539b63aa748b1b845434a92522a77dee3744556
model_manifest: fixtures/models/two-experts-4-6.json
model_manifest_sha256: 543e2c3b70c52392b615dec923aa0c6a99a90ee88248ae5106b3093a89165538
global_budget_bytes: 10
cache_scope: global
events: 2
manifest_experts: 2
```

At 9 bytes each expert still fits individually, but the atomic active set does
not, so the configuration is rejected before any simulation (exit code 5):

```bash
moe-sim capacity check \
  --trace fixtures/synthetic/active-set-0-1.jsonl \
  --model-manifest fixtures/models/two-experts-4-6.json \
  --global-budget-bytes 9
```

```text
error: capacity check failed: active set exceeds global capacity: event 0 request 1 layer 0 totals 10 bytes, global budget is 9 bytes
```

Replay the trace under one policy. `no-cache` retains nothing between events,
so every activation is a load and residency peaks at the largest atomic active
set:

```bash
moe-sim run \
  --trace fixtures/synthetic/active-set-0-1.jsonl \
  --model-manifest fixtures/models/two-experts-4-6.json \
  --global-budget-bytes 10 \
  --policy no-cache
```

```text
status: ok
tool_version: 0.1.0
input_format: v1
trace: fixtures/synthetic/active-set-0-1.jsonl
trace_sha256: ba96fdf54901d5f93e090714c539b63aa748b1b845434a92522a77dee3744556
model_manifest: fixtures/models/two-experts-4-6.json
model_manifest_sha256: 543e2c3b70c52392b615dec923aa0c6a99a90ee88248ae5106b3093a89165538
global_budget_bytes: 10
cache_scope: global
policy: no-cache
events: 2
object_loads: 3
byte_loads: 16
object_hits: 0
byte_hits: 0
object_reloads: 1
byte_reloads: 6
evictions: 0
evicted_bytes: 0
peak_resident_bytes: 10
```

`--policy lru` and `--policy lfu` retain experts between events under the same
byte budget. On this fixture both keep expert 1 resident, turning the second
event into a 6-byte hit instead of the reload the baseline pays:

```bash
moe-sim run \
  --trace fixtures/synthetic/active-set-0-1.jsonl \
  --model-manifest fixtures/models/two-experts-4-6.json \
  --global-budget-bytes 10 \
  --policy lru
```

```text
status: ok
tool_version: 0.1.0
input_format: v1
trace: fixtures/synthetic/active-set-0-1.jsonl
trace_sha256: ba96fdf54901d5f93e090714c539b63aa748b1b845434a92522a77dee3744556
model_manifest: fixtures/models/two-experts-4-6.json
model_manifest_sha256: 543e2c3b70c52392b615dec923aa0c6a99a90ee88248ae5106b3093a89165538
global_budget_bytes: 10
cache_scope: global
policy: lru
events: 2
object_loads: 2
byte_loads: 10
object_hits: 1
byte_hits: 6
object_reloads: 0
byte_reloads: 0
evictions: 0
evicted_bytes: 0
peak_resident_bytes: 10
```

`--cache-scope per-layer` runs the same policies inside one independent cache
per layer. Every simulated layer needs an explicit quota, the quotas may not
sum past the total budget, and unused quota is not shared: an eviction in one
layer never frees room in another. The committed two-layer fixture makes the
partition visible — layer 1 evicts inside its own 5-byte quota, and the report
pairs each quota with that cache's peak so the per-layer capacity invariant
can be audited from the output alone:

```bash
moe-sim run \
  --trace fixtures/synthetic/two-layers.jsonl \
  --model-manifest fixtures/models/two-layers.json \
  --global-budget-bytes 15 \
  --cache-scope per-layer \
  --layer-quota-bytes 0:10 \
  --layer-quota-bytes 1:5 \
  --policy lru
```

```text
status: ok
tool_version: 0.1.0
input_format: v1
trace: fixtures/synthetic/two-layers.jsonl
trace_sha256: 35c61891c72dba7d6eeac758215f320afbef900e106646face1c58a3b268f824
model_manifest: fixtures/models/two-layers.json
model_manifest_sha256: 1c94b98f26c0f18f85a5aaca95b1a2d70ea8a1befeb55d8d8a893e792c0d7596
global_budget_bytes: 15
cache_scope: per-layer
policy: lru
events: 4
object_loads: 4
byte_loads: 18
object_hits: 1
byte_hits: 6
object_reloads: 0
byte_reloads: 0
evictions: 1
evicted_bytes: 5
peak_resident_bytes: 15
layer 0: quota_bytes: 10, peak_resident_bytes: 10
layer 1: quota_bytes: 5, peak_resident_bytes: 5
```

The aggregate `peak_resident_bytes` is the high-water mark of summed residency
across the layer caches, not the sum of the per-layer peaks: caches that fill
at different times never overstate simultaneous residency. On this example the
two coincide because both caches are full at once; replaying the same fixture
under `--policy no-cache` separates them — the aggregate peak drops to 10
while the layer peaks still read 10 and 5.

Capacity is validated before replay, so an infeasible configuration is
rejected with exit code 5 and no metrics are emitted. Resident bytes never
exceed the applicable capacity — the budget under a global scope, each layer's
quota under a per-layer scope — and no member of the active set can be evicted
while its event is in flight.

Two metric definitions are worth stating plainly, because comparisons depend
on them:

**A release is not an eviction.** Eviction is the capacity-driven removal of an
object a policy chose to retain. A policy that retains nothing therefore evicts
nothing, which keeps the baseline comparable with the caching policies measured
against it.

**Churn is rework, not turnover.** `object_reloads` and `byte_reloads` count
loads of an expert that was loaded earlier in the run and is no longer
resident, so `object_loads` splits into unavoidable cold misses plus reloads.
That separation is what distinguishes a policy that thrashes from one that
simply faces a large working set.

Churn counts the rework, not its cause. A retaining policy loses residency by
eviction; `no-cache` loses it by releasing the active set. So the baseline
reports reloads while its `evictions` stay `0` — the pairing above is
intended, and reading it as a broken report would be a mistake.

`lfu` breaks ties by least recent use, and a frequency count belongs to a
resident entry: it restarts when an object is admitted again, so a once-hot
expert does not become immortal after eviction. One atomic active set counts
as one access, so members of the same event can tie on every criterion; a
genuine tie evicts the lowest expert key first, as an explicit rule.

`--policy belady` is an offline reference, not an online policy: it evicts the
expert whose next use is farthest away — never-reused experts first, ties by
lowest expert key — which requires reading the whole trace beyond the current
event. It only accepts manifests whose experts all share one size, and its
report carries an `objective:` line so the numbers never pose as an online
policy's outcome. The committed cycle fixture repeats `[0], [1], [2]` twice
over three 2-byte experts with room for two: LRU always evicts the expert
needed next and loads 6 objects, while the offline reference loads 4:

```bash
moe-sim run \
  --trace fixtures/synthetic/three-experts-cycle.jsonl \
  --model-manifest fixtures/models/three-experts-uniform.json \
  --global-budget-bytes 4 \
  --policy belady
```

```text
status: ok
tool_version: 0.1.0
input_format: v1
trace: fixtures/synthetic/three-experts-cycle.jsonl
trace_sha256: 8005f20747211b4fb2da49c5d68606f6229c940aa3f61a4f4828e5646b33eaaf
model_manifest: fixtures/models/three-experts-uniform.json
model_manifest_sha256: 3b1a153a9c889a77c78229ff440ab8f071326f317eac6ace984454048f2694e5
global_budget_bytes: 4
cache_scope: global
policy: belady
objective: minimum object loads (offline reference, uniform expert sizes, whole-trace lookahead)
events: 6
object_loads: 4
byte_loads: 8
object_hits: 2
byte_hits: 4
object_reloads: 1
byte_reloads: 2
evictions: 2
evicted_bytes: 4
peak_resident_bytes: 4
```

Classic Belady MIN is proven optimal for single-object requests of uniform
size. Atomic active sets fall outside that classic model, so `moe-sim` does
not call this policy optimal by proof: it is checked against a deliberately
bounded exhaustive oracle (at most 12 events and 8 distinct experts) that
explores every eviction schedule, and greedy farthest-next-use matches the
oracle's optimum on every enumerated uniform-size case. A manifest with more
than one expert size is rejected with exit code 6 rather than approximated,
because general variable-size caching has no greedy byte-aware optimum.

### Generating synthetic traces

`trace generate` writes a deterministic synthetic trace and its twin
manifest, then reports the parameters it consumed and the SHA-256 of each
written file. Six patterns exist: `repetition`, `cyclic`, `random`,
`hotset-shift`, `variable-sizes` (linearly growing expert sizes, so byte and
object metrics separate), and `adversarial-lru`. Only `random` is
stochastic: its `--seed` is required, recorded in the report, and reproduces
the trace byte for byte; the deterministic patterns reject a seed instead of
silently ignoring it. The generator is a pure function in `moe-sim-core`
spread by an in-repo `SplitMix64` mixer, so equal parameters produce equal
files on every platform. Parameters are explicitly bounded — at most 65,536
experts, 10 million events, and 50 million total activations — so an
impossible request fails with a typed error before anything allocates, and
output paths that resolve to the same physical file (including symlink and
hard-link aliases) are rejected before anything is written.

The following two commands reproduce a full synthetic comparison from a
clean checkout (after `cargo build`):

```bash
moe-sim trace generate \
  --pattern cyclic \
  --experts 3 \
  --events 6 \
  --out-trace target/cycle.jsonl \
  --out-model-manifest target/cycle-manifest.json
```

```text
status: ok
tool_version: 0.1.0
input_format: v1
source: synthetic
pattern: cyclic
experts: 3
events: 6
out_trace: target/cycle.jsonl
out_trace_sha256: 0681a6723000b94373ab6809ef5ed2d50d8e2a00a4c80c87e5ee9558616a7932
out_model_manifest: target/cycle-manifest.json
out_model_manifest_sha256: 822c6fa7b3cd162ec189d5c70c6acf006daad1b2d3ca5535e1240e73d3e04f9e
```

A local ~100k-event stress trace is regenerable the same way (for example
`--pattern random --experts 64 --events 100000 --active-per-event 4 --seed
42`); the test suite generates the equivalent traces in memory, so nothing
large is committed.

### Comparing policies

`compare` replays one trace and manifest across a policy and budget matrix:
`--policies` and `--global-budgets-bytes` are comma-separated lists whose
order is preserved in the report, duplicates and unknown values are rejected
by name, and only the global scope exists in `v0.1` (per-layer flags point
back at `run`). Every replay completes before one byte of report exists, so
an inapplicable combination — an infeasible budget, or `belady` selected
against a variable-size manifest — rejects the whole comparison instead of
emitting a partial table:

```bash
moe-sim compare \
  --trace target/cycle.jsonl \
  --model-manifest target/cycle-manifest.json \
  --global-budgets-bytes 2,3 \
  --policies no-cache,lru,lfu,belady
```

```text
status: ok
tool_version: 0.1.0
input_format: v1
trace: target/cycle.jsonl
trace_sha256: 0681a6723000b94373ab6809ef5ed2d50d8e2a00a4c80c87e5ee9558616a7932
model_manifest: target/cycle-manifest.json
model_manifest_sha256: 822c6fa7b3cd162ec189d5c70c6acf006daad1b2d3ca5535e1240e73d3e04f9e
cache_scope: global
policies: no-cache,lru,lfu,belady
global_budgets_bytes: 2,3
events: 6
belady_objective: minimum object loads (offline reference, uniform expert sizes, whole-trace lookahead)
results: 8
policy no-cache budget 2: object_loads: 6, byte_loads: 6, object_hits: 0, byte_hits: 0, object_reloads: 3, byte_reloads: 3, evictions: 0, evicted_bytes: 0, peak_resident_bytes: 1
policy no-cache budget 3: object_loads: 6, byte_loads: 6, object_hits: 0, byte_hits: 0, object_reloads: 3, byte_reloads: 3, evictions: 0, evicted_bytes: 0, peak_resident_bytes: 1
policy lru budget 2: object_loads: 6, byte_loads: 6, object_hits: 0, byte_hits: 0, object_reloads: 3, byte_reloads: 3, evictions: 4, evicted_bytes: 4, peak_resident_bytes: 2
policy lru budget 3: object_loads: 3, byte_loads: 3, object_hits: 3, byte_hits: 3, object_reloads: 0, byte_reloads: 0, evictions: 0, evicted_bytes: 0, peak_resident_bytes: 3
policy lfu budget 2: object_loads: 6, byte_loads: 6, object_hits: 0, byte_hits: 0, object_reloads: 3, byte_reloads: 3, evictions: 4, evicted_bytes: 4, peak_resident_bytes: 2
policy lfu budget 3: object_loads: 3, byte_loads: 3, object_hits: 3, byte_hits: 3, object_reloads: 0, byte_reloads: 0, evictions: 0, evicted_bytes: 0, peak_resident_bytes: 3
policy belady budget 2: object_loads: 4, byte_loads: 4, object_hits: 2, byte_hits: 2, object_reloads: 1, byte_reloads: 1, evictions: 2, evicted_bytes: 2, peak_resident_bytes: 2
policy belady budget 3: object_loads: 3, byte_loads: 3, object_hits: 3, byte_hits: 3, object_reloads: 0, byte_reloads: 0, evictions: 0, evicted_bytes: 0, peak_resident_bytes: 3
```

`--output json` renders the same matrix as one machine-readable JSON object
(provenance first, one row object per cell), and `--output csv` renders a
table whose rows repeat the provenance columns so every line is
self-contained; belady rows carry their objective in both, and all three
formats are byte-identical across repeated runs:

```bash
moe-sim compare \
  --trace target/cycle.jsonl \
  --model-manifest target/cycle-manifest.json \
  --global-budgets-bytes 2 \
  --policies lru,belady \
  --output csv
```

```text
tool_version,input_format,trace,trace_sha256,model_manifest,model_manifest_sha256,cache_scope,policy,global_budget_bytes,objective,events,object_loads,byte_loads,object_hits,byte_hits,object_reloads,byte_reloads,evictions,evicted_bytes,peak_resident_bytes
0.1.0,v1,target/cycle.jsonl,0681a6723000b94373ab6809ef5ed2d50d8e2a00a4c80c87e5ee9558616a7932,target/cycle-manifest.json,822c6fa7b3cd162ec189d5c70c6acf006daad1b2d3ca5535e1240e73d3e04f9e,global,lru,2,,6,6,6,0,0,3,3,4,4,2
0.1.0,v1,target/cycle.jsonl,0681a6723000b94373ab6809ef5ed2d50d8e2a00a4c80c87e5ee9558616a7932,target/cycle-manifest.json,822c6fa7b3cd162ec189d5c70c6acf006daad1b2d3ca5535e1240e73d3e04f9e,global,belady,2,"minimum object loads (offline reference, uniform expert sizes, whole-trace lookahead)",6,4,4,2,2,1,1,2,2,2
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

Classic Belady MIN is proven optimal for single-object requests of uniform
size; atomic active sets fall outside that proof, which is why the belady
section above claims oracle-checked equality on enumerated cases rather than
optimality. Once expert objects have
different sizes or fetch costs, the general offline caching problem is
NP-hard. `moe-sim` will therefore not describe a scalable greedy
"byte-aware Belady" implementation as an optimum.

`v0.1` uses classic Belady on uniform-size fixtures and a deliberately
bounded exhaustive solver for tiny variable-size correctness tests, both
landed with slice 1C — Belady on the CLI, the solver as a test-only gate.
Practical
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
