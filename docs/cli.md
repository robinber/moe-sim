# CLI reference

**Authority** for command-line behavior, exit codes, and report examples.
Product overview stays in the root [README](../README.md); simulation rules
live in [contracts.md](contracts.md).

Five commands ship in `v0.1`: `trace inspect`, `trace generate`,
`capacity check`, `run`, and `compare`.

All are flag-driven (no positional arguments), take budgets as plain byte
counts, print a deterministic report to stdout on success, and report typed
errors on stderr with a meaningful exit code:

| Code | Meaning |
| ---: | --- |
| 0 | success |
| 2 | bad arguments |
| 3 | file I/O failure |
| 4 | parse failure |
| 5 | capacity rejection |
| 6 | replay failure |

Failures never emit partial stdout.

`capacity check` and `run` take `--cache-scope global|per-layer` (default
`global`). A per-layer scope additionally requires one repeated
`--layer-quota-bytes LAYER:BYTES` flag per simulated layer; quotas under a
global scope, a per-layer scope without quotas, or one layer quoted twice are
argument errors (exit code 2). Every report records the selected scope.

Every success report opens with provenance — the tool version, the input
contract version, and a SHA-256 digest of each input beside its path. The
digests are reproducible with `shasum -a 256 <path>`.

Metric definitions (release vs eviction, churn) live in
[contracts.md](contracts.md).

## Inspect

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

## Capacity check

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

## Run (single policy)

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

### Per-layer scope

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
across the layer caches, not the sum of the per-layer peaks. On this example
the two coincide because both caches are full at once; replaying the same
fixture under `--policy no-cache` separates them — the aggregate peak drops to
10 while the layer peaks still read 10 and 5.

Capacity is validated before replay, so an infeasible configuration is
rejected with exit code 5 and no metrics are emitted. Resident bytes never
exceed the applicable capacity, and no member of the active set can be evicted
while its event is in flight.

### Belady offline reference

`--policy belady` only accepts uniform-size manifests (exit code 6 otherwise).
The committed cycle fixture repeats `[0], [1], [2]` twice over three 2-byte
experts with room for two: LRU always evicts the expert needed next and loads
6 objects, while the offline reference loads 4:

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

See [contracts.md](contracts.md) for optimality limits and the oracle gate.

## Generate synthetic traces

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

From a clean checkout (after `cargo build`):

```bash
./target/debug/moe-sim trace generate \
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

## Compare policies

`compare` replays one trace and manifest across a policy and budget matrix:
`--policies` and `--global-budgets-bytes` are comma-separated lists whose
order is preserved in the report, duplicates and unknown values are rejected
by name, and only the global scope exists on `compare` in `v0.1` (per-layer
flags point back at `run`). Every replay completes before one byte of report
exists, so an inapplicable combination — an infeasible budget, or `belady`
selected against a variable-size manifest — rejects the whole comparison
instead of emitting a partial table:

```bash
./target/debug/moe-sim compare \
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

## Fixture-based comparison

Without generating files first:

```bash
./target/debug/moe-sim compare \
  --trace fixtures/synthetic/three-experts-cycle.jsonl \
  --model-manifest fixtures/models/three-experts-uniform.json \
  --global-budgets-bytes 2,4,6 \
  --policies no-cache,lru,lfu,belady
```
