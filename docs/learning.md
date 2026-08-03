# Learning exercise: no-cache vs LRU vs LFU

Pedagogical walkthrough (not a contract). Simulator rules:
[contracts.md](contracts.md). CLI detail: [cli.md](cli.md).

This exercise starts with observable data, asks for a prediction, and then
checks that prediction against the simulator.

## Question

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

## Input trace

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

## Size manifest

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

## Run the three policies

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

## Replay it by hand

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

## Expected metrics

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

## What the result means

- `no-cache` is the retention-free baseline. It releases expert `0` after each
  event, so both later uses are reloads. Releases are not counted as evictions.
- LRU uses only recency. The short access to expert `1` makes the previously hot
  expert `0` the least recent entry, so loading expert `2` removes it.
- LFU uses resident access frequency before recency. It remembers that expert
  `0` was used twice and keeps it for the final hit.

This trace deliberately favors LFU. It demonstrates how the policies differ;
it does not prove that LFU is generally better than LRU.
