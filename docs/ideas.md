# Idea archive

**Authority** for parked explorations and default non-goals.

Parked ideas only. **Not a backlog, not a sequence, not unfinished work.**

Reopening any item would require a new bounded project decision and a single
question worth answering (for example: *“Does this teach me something I cannot
learn cheaper another way?”*).

## Optional explorations (never scheduled)

### O1 — One real trace adapter (former M2 / `v0.2`)

**Question:** do synthetic conclusions transfer to one real activation trace?

- Audit one dataset (schema, ordering, access terms, pin-able revision).
- Stream into the canonical format; checksums + provenance.
- Same replay engine and policies as synthetic runs.
- Tiny redistributable or schema-equivalent fixture when redistribution is
  blocked.

**Skip if:** synthetic fixtures already answered your curiosity, or data access
is painful relative to learning value.

**Candidate sources** (not integrated):

- [MoE-Beyond](https://github.com/ngavhane/moe-beyond), after its trace format
  is audited;
- [Patterns behind Chaos / MoE expert-selection traces](https://huggingface.co/datasets/core12345/MoE_expert_selection_trace),
  after gated access, revision pinning, and subset selection are resolved.

Large or gated traces must not be committed. Any real-data experiment would
need dataset revision, selected files, checksums, adapter version, and every
transformation recorded.

### O2 — Layout-aware read schedules (former M3)

**Question:** how much do file layout and alignment amplify logical misses?

- Offsets, alignment, packing → physical read schedules (bytes, not latency).
- Separate checkpoint-read bytes from resident-cache bytes when precision or
  representation differs.
- Attribute amplification categories explicitly.
- No timing claims.

**Skip if:** you only care about logical hit/miss under a byte budget.

### O3 — Storage timing sketches (former M4–M5)

**Question:** can a simple device model change policy rankings?

- Only after O2 schedules exist and you still care.
- Mark profiles as measured / estimated / synthetic.
- Prefer a **small** experiment on one machine over multi-device science.

**Skip if:** latency is out of scope (default for this project).

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

## Idea parking lot

Captured so they do not expand an active milestone:

- dynamic layer quotas;
- segmented LRU and larger policy catalogs;
- scalable offline bounds for variable-size caching;
- learned expert predictors;
- mixed-precision residency;
- compression-aware service models;
- production runtime integration;
- dashboards and hosted services.

## Explicit non-goals (default)

Unless a reopened exploration deliberately reopens them:

- model-weight or token execution; attention, KV caches, tokenizers, CUDA,
  kernels, or HTTP servers;
- end-to-end inference latency claims or cycle-accurate storage simulation;
- silently inferring missing prefill/decode boundaries;
- distributed / multi-node simulation;
- learned predictors as a core feature;
- dynamic plugin ABI;
- hosting large research datasets in-repo;
- device timing, prefetch, or compute-overlap assumptions inside the logical
  simulator (without an explicit new scope);
- a commitment to publish papers or cut a long-lived `v1.0` product.

## If something is reopened

1. State one question.
2. Check whether the existing `v0.1` tool already answers it.
3. Define one bounded slice and a hard stop condition.
4. Re-read [contracts.md](contracts.md).
5. Do not treat the rest of this archive as a queue.
