# moe-sim

`moe-sim` is a trace-driven **logical** simulator for studying cache behavior in
single-node, out-of-core Mixture-of-Experts (MoE) inference.

It answers a deliberately narrow question:

> Given an expert-activation trace, explicit expert sizes, and a memory budget,
> how do different caching policies affect misses, bytes loaded, and cache
> churn?

## Status

**`v0.1` complete. Project parked.**

| | |
| --- | --- |
| Ships | Canonical events, size manifests, deterministic replay, global or fixed per-layer budgets, `no-cache` / LRU / LFU / Belady (offline, uniform sizes), synthetic generation, `compare` (text / JSON / CSV), input provenance |
| Does not ship | Storage timing, prefetch, compute overlap, physical replay, real-dataset adapters, latency claims |
| Ideas (not backlog) | [docs/ideas.md](docs/ideas.md) |
| History | [docs/history.md](docs/history.md) |
| Project card | [PROJECT.md](PROJECT.md) |
| Agent rules | [AGENTS.md](AGENTS.md) |

## Why this exists

MoE models activate only a subset of routed experts per token and layer. That
sparsity makes out-of-core execution interesting, but usefulness depends on
locality, expert size, capacity, and load cost.

`moe-sim` is a **small personal lab bench**: same traces, same byte accounting,
same objectives — honest policy comparison without an inference runtime. It is
not an engine and was never a multi-year research program.

## Build and quick start

```bash
cargo build
```

```bash
./target/debug/moe-sim trace inspect \
  --trace fixtures/synthetic/active-set-0-1.jsonl
```

```bash
./target/debug/moe-sim run \
  --trace fixtures/synthetic/active-set-0-1.jsonl \
  --model-manifest fixtures/models/two-experts-4-6.json \
  --global-budget-bytes 10 \
  --policy lru
```

```bash
./target/debug/moe-sim compare \
  --trace fixtures/synthetic/three-experts-cycle.jsonl \
  --model-manifest fixtures/models/three-experts-uniform.json \
  --global-budgets-bytes 2,4,6 \
  --policies no-cache,lru,lfu,belady
```

Commands: `trace inspect`, `trace generate`, `capacity check`, `run`, `compare`.

- Full CLI and sample reports: [docs/cli.md](docs/cli.md)
- Inputs, metrics, invariants: [docs/contracts.md](docs/contracts.md)
- Hand-worked LRU vs LFU: [docs/learning.md](docs/learning.md)

## Non-goals

No model execution, attention/KV/tokenizers/CUDA/HTTP, end-to-end latency or
cycle-accurate storage claims, silent prefill/decode inference, multi-node
simulation, learned predictors, dynamic plugins, or large in-repo research
traces. Parked ideas are optional curiosities, not follow-up debt — see
[docs/ideas.md](docs/ideas.md).

## Architecture

```text
moe-sim/
  crates/
    moe-sim-core/    pure replay, policies, metrics, domain errors
    moe-sim-cli/     file adapters, commands, report rendering
  fixtures/{synthetic,models}/
  docs/              cli · contracts · learning · history · ideas
  README.md  PROJECT.md  ROADMAP.md  AGENTS.md
```

Dependency: `moe-sim-cli` → `moe-sim-core`. The core is synchronous and free of
filesystem adapters, async runtimes, and storage models.

Documentation map and source-of-truth table: [docs/README.md](docs/README.md).

## Quality gates

```bash
cargo +nightly fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo deny check advisories licenses sources
```

Aliases in [`.cargo/config.toml`](.cargo/config.toml): `lint`, `test-all`,
`doc-all`, `deny-all`. Agent-oriented verification detail: [AGENTS.md](AGENTS.md).

## License

[MIT](LICENSE). Dataset licenses and access terms remain independent of the
source-code license.
