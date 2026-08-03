# Project Card — `moe-sim`

Last updated: 2026-08-03

## In one sentence

`moe-sim` is a deterministic logical simulator that replays Mixture-of-Experts
(MoE) activations to compare cache policies under a byte-denominated memory
budget.

## Current state

| Field | Value |
| --- | --- |
| Version | `v0.1` complete |
| Status | Deliberately **parked** |
| Works | Synthetic traces, capacity check, `no-cache` / LRU / LFU / Belady, global or fixed per-layer budgets, `compare`, text/JSON/CSV, provenance |
| Not planned | Model execution, physical storage, latency, prefetch, CUDA, multi-node, HTTP |
| Next task | **None** |

## Why it exists

Compare cache policies on the same activation order and expert sizes without
building an inference engine. Reproducible test bench; policy logic separated
from the rest of inference cost.

## Mental model

```text
trace + size manifest + budget + scope + policy
                 │
                 ▼
        validate before simulation
                 │
                 ▼
           replay in file order
                 │
                 ▼
      object/byte metrics + provenance
```

## Architecture

| Path | Role |
| --- | --- |
| [`crates/moe-sim-core`](crates/moe-sim-core) | Types, validation, synthetic gen, policies, pure replay |
| [`crates/moe-sim-cli`](crates/moe-sim-cli) | File adapters, CLI, provenance, reports |
| [`fixtures`](fixtures) | Small valid/invalid cases |

Dependency: CLI → core. Core: synchronous, deterministic, no FS/async/inference.

## Contracts (summary)

Authority: [docs/contracts.md](docs/contracts.md).

- File-order replay; metadata never reorders events.
- Atomic active set per event (fit + pin for the event).
- Impossible configs reject before results.
- Resident bytes ≤ global budget or layer quota.
- Object and byte metrics separate; release ≠ eviction; churn = reloads.
- Equal inputs → equal machine-readable reports.
- Belady: offline, uniform sizes only.

## Building blocks

| Block | Location |
| --- | --- |
| Event | [`trace.rs`](crates/moe-sim-core/src/trace.rs) |
| Manifest | [`manifest.rs`](crates/moe-sim-core/src/manifest.rs) |
| Scope | [`scope.rs`](crates/moe-sim-core/src/scope.rs) |
| Replay | [`replay.rs`](crates/moe-sim-core/src/replay.rs) |
| Synthetic | [`synthetic.rs`](crates/moe-sim-core/src/synthetic.rs) |
| Provenance | [`provenance.rs`](crates/moe-sim-cli/src/provenance.rs) |
| Oracle (tests) | [`belady_oracle.rs`](crates/moe-sim-core/tests/belady_oracle.rs) |

Extract crates only if a second consumer appears.

## Recovered practice

Pure core vs CLI adapters; validated constructors and typed errors;
fixtures/seeds/digests for reproducibility; object vs byte metrics; bounded
oracle for heuristics; clean stop without turning optional ideas into debt.

## Pitfalls already handled

Logical sim ≠ inference engine; object hit rate hiding large experts; evicting
the active set; partial results on invalid config; Belady overclaim; summing
non-simultaneous per-layer peaks; early storage/async/crate sprawl.

## Run again

```bash
cargo build
cargo test --workspace --all-features
```

```bash
./target/debug/moe-sim compare \
  --trace fixtures/synthetic/three-experts-cycle.jsonl \
  --model-manifest fixtures/models/three-experts-uniform.json \
  --global-budgets-bytes 2,4,6 \
  --policies no-cache,lru,lfu,belady
```

More: [docs/cli.md](docs/cli.md), [docs/learning.md](docs/learning.md),
[AGENTS.md](AGENTS.md). Full map: [docs/README.md](docs/README.md).

## If reopened

1. One question.
2. Prefer existing behavior if it already answers it.
3. Bounded slice + stop condition.
4. Re-read [docs/contracts.md](docs/contracts.md) and [docs/ideas.md](docs/ideas.md).
5. Run tests before edits.
6. Update this card with outcome and new stop.
