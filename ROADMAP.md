# moe-sim roadmap

This roadmap turns the broad simulator vision into independently useful,
testable milestones. Each milestone has an explicit stop condition. Later work
does not begin merely because the previous code was written; its exit criteria
must be demonstrated and reviewed first.

The roadmap is directional rather than a calendar commitment. Milestones 0–2
are conventional software engineering. Milestones 3–6 contain progressively
more research and hardware-validation risk.

## Delivery rules

- Build one vertical slice at a time.
- Keep the logical simulator independent of storage timing.
- Add one real dataset adapter before adding a second.
- Treat every approximation as named input data, not an invisible default.
- Reject unsupported comparisons instead of filling missing metadata.
- Keep generated results, large traces, and machine-specific profiles out of
  source control unless they are intentionally small fixtures.
- Do not publish latency claims before physical replay establishes a documented
  error envelope.
- Do not create a new crate until its dependency or ownership boundary is real.

## Milestone 0 — Repository and contracts

**Outcome:** a small Rust workspace with stable initial contracts and no
simulation claims.

### Deliverables

- [x] Initialize Git.
- [x] Use the MIT License for the source code.
- [ ] Create `moe-sim-core` and a thin `moe-sim-cli`.
- [ ] Define the canonical activation event and phase semantics.
- [ ] Define file-order replay and atomic active-set semantics.
- [ ] Define the minimal expert-size manifest.
- [ ] Define oversize expert and oversize active-set rejection.
- [ ] Define run provenance fields and deterministic seed handling.
- [ ] Add tiny valid and invalid fixtures.
- [ ] Document error behavior and compatibility rules.
- [ ] Add impact-scoped format, Clippy, test, and rustdoc checks.
- [ ] Pin the reference CI runner used by later resource gates.

### Exit criteria

- Canonical events round-trip without information loss.
- Unknown phase remains explicit.
- Event order is file order and is never reconstructed silently from metadata.
- Each event's expert IDs form one atomic pinned set.
- Configurations where one expert or active set exceeds its applicable capacity
  fail before simulation results are emitted.
- Duplicate or out-of-range expert identifiers follow a documented rule.
- Malformed traces and manifests return typed, actionable errors.
- Public items are documented and the narrow Rust quality gates pass.

### Explicitly deferred

Cache policies, real datasets, async I/O, plugins, storage profiles, and
benchmarks.

## Milestone 1 — Deterministic logical cache simulator (`v0.1`)

**Outcome:** a reproducible methodology infrastructure — canonical event
format, deterministic replay, and fair policy comparison — on synthetic traces
under byte budgets.

v0.1 is methodology and reproducibility infrastructure, not a decision tool or
a novelty claim. LRU is a reference baseline, not a presumed winner. Recent
work reports both theoretical support for LRU-like policies and empirical
improvements from layer-aware variants or LFU; see
[Cache Management for Mixture-of-Experts LLMs](https://arxiv.org/abs/2509.02408)
and
[In-depth Analysis on Caching and Pre-fetching in Mixture of Experts Offloading](https://arxiv.org/abs/2511.05814).
The value of v0.1 is a canonical format, shared memory accounting, and explicit
byte/object metrics. Any claim that these fill a literature gap must be
re-evaluated against current work before publication.

### Slice 1A — Replay and accounting

- [ ] Implement sequential file-order event replay.
- [ ] Implement atomic pin/use/release behavior.
- [ ] Implement byte-accurate residency and eviction accounting.
- [ ] Implement the no-cache baseline.
- [ ] Provide `trace inspect` and a text-only `run` command.

**Gate:** hand-calculated no-cache and residency fixtures match exactly.

### Slice 1B — Online policies and cache scopes

- [ ] Implement LRU and LFU independently of cache scope.
- [ ] Support one global budget.
- [ ] Support explicit fixed per-layer quotas whose sum does not exceed the
  total budget.
- [ ] Report object hits, byte hits, loads, evictions, resident bytes, and
  churn.

**Gate:** every policy respects atomic pinning and byte capacity on adversarial
fixtures.

### Slice 1C — Offline correctness references

- [ ] Implement classic Belady MIN for uniform-size traces.
- [ ] Implement a bounded exhaustive oracle for tiny variable-size test cases.
- [ ] Label every offline result with its objective and applicability.

**Gate:** classic Belady matches exhaustive uniform-size cases; the bounded
solver independently verifies tiny variable-size optima.

### Slice 1D — Comparison and reproducible outputs

- [ ] Provide the `compare` command.
- [ ] Provide text, JSON, and CSV output.
- [ ] Add deterministic synthetic patterns: repetition, uniform random,
  cyclic, changing hotset, variable expert sizes, and adversarial LRU.
- [ ] Add a `synthetic-100k` reference workload: 100,000 total events, 32
  layers, 64 experts per layer, routed top-2, and one published reference seed
  for byte-identical regression runs.
- [ ] Before inspecting policy results, preregister an evaluation matrix with
  deterministic workload families, 20 fixed seeds for each stochastic family,
  an ordered set of memory budgets, `total bytes loaded` as the primary cost
  metric, and the paired comparison procedure used at the milestone gate.

### Milestone exit criteria

- Repeated runs produce byte-identical machine-readable reports.
- Resident bytes never exceed the selected total or per-layer capacity.
- Active expert sets cannot be evicted or partially admitted.
- Hand-calculated fixtures match simulator output.
- Classic Belady does not lose to online policies on the same uniform-size
  objective.
- Online variable-size policy costs remain informative for relative policy
  comparisons and are checked against the bounded optimum on tiny cases.
  Because general variable-size caching is NP-hard, v0.1 reports no scalable
  global-optimality gap for large traces. Practical lower bounds for large
  variable-size traces remain deferred research work.
- No scalable greedy policy is presented as the general offline optimum.
- Reports clearly separate object and byte metrics.
- On the pinned CI runner, online policies process `synthetic-100k` with peak
  RSS at or below 256 MiB and median wall time at or below 30 seconds across
  five warm runs.
- Online policies stream the reference workload without loading the full trace;
  offline references report their peak RSS separately.

### Gate

Stop and assess whether logical comparisons already answer useful questions.
Do not add storage timing simply to continue the roadmap.

Every milestone exit criterion above must pass before this gate is evaluated.
The comparison protocol is committed before any policy result is inspected;
changing it after that point creates a new protocol revision and restarts the
evaluation.

For each stochastic workload-family and budget pair, compare every online
policy with LRU on the same 20 seeds. For each seed, define percentage reduction
as `100 * (LRU bytes - policy bytes) / LRU bytes`. Report its median and a 95%
percentile-bootstrap confidence interval from 10,000 paired-seed resamples,
using a bootstrap RNG seed published in the preregistered protocol. A material
improvement requires a median reduction of at least 5%, with a lower confidence
bound above zero, at two adjacent preregistered budgets. A policy that is worse
than LRU never satisfies this condition.

For a deterministic workload family, material improvement requires a reduction
of at least 5% at two adjacent preregistered budgets, without a confidence
interval. An isolated single-budget improvement is inconclusive.

Decision criteria (pre-committed to limit continuation bias):

- If an online policy demonstrates a material improvement, M2 may be planned
  to test whether the result transfers to one real trace.
- If the upper confidence bound stays below 5% for every online policy at every
  preregistered stochastic workload and budget, and no deterministic workload
  family and budget pair reaches a 5% reduction, stop expanding the
  online-policy catalog. The operator may close at M1 or plan M2 solely to test
  whether a real trace changes that conclusion; do not infer that the wider
  field does not need the tool.
- If neither condition is satisfied, the policy result is inconclusive. Add
  preregistered seeds or redesign the workload suite, then rerun the complete
  protocol before planning M2.

### Minimum standalone artifact

Completing M1 produces a standalone software artifact: a versioned canonical
activation-event format, deterministic fixtures, and a reproducible policy
comparison on synthetic traces under byte budgets. Calling it a citable
research contribution requires a current prior-art review and an archived,
versioned public release. If later milestones never ship, M1 remains a valid
stopping point.

## Milestone 2 — One reproducible real-trace adapter (`v0.2`)

**Outcome:** the logical simulator produces reproducible results for one real
trace source.

### Entry decision

Audit candidate datasets and select exactly one initial adapter based on:

- documented schema and ordering;
- prefill/decode information;
- redistribution and access conditions;
- manageable subset selection;
- stable revision identifiers;
- independent sanity checks.

### Deliverables

- [ ] Record the adapter decision and rejected alternatives.
- [ ] Implement streaming conversion into the canonical format.
- [ ] Pin a source revision and selected file list.
- [ ] Record source and converted-file checksums.
- [ ] Validate layer count, expert ranges, routed top-k, and event ordering.
- [ ] Preserve request, prefill, and decode boundaries when present.
- [ ] Include a tiny redistributable fixture or a generated schema-equivalent
  fixture when source terms prohibit redistribution.
- [ ] Publish one fully reproducible policy and budget sweep.

### Exit criteria

- A new user with authorized source access can reproduce the canonical subset.
- Conversion is deterministic and restartable.
- Source assumptions appear in report provenance.
- Unsupported or ambiguous fields remain explicit.
- Synthetic and real-trace results use the same replay engine and policies.

### Explicitly deferred

A second dataset adapter, embeddings, learned predictors, and full-dataset
sweeps.

## Milestone 3 — Layout-aware storage schedules (`v0.3`)

> **Optional research extension.** Milestones 3–6 are research work, not the
> canonical path forward. The exit decisions after M3, M4, and M5 may stop at
> the highest completed and validated milestone. M5 alone requires at least 20
> randomized repetitions per scenario, a second physical device, and strict
> separation between calibration and held-out replay; a second operating
> system remains optional. Scope beyond M2 must be re-estimated against current
> models, datasets, hardware access, and literature at each gate.

**Outcome:** logical misses become inspectable physical read schedules, without
claiming device latency.

### Deliverables

- [ ] Extend manifests with file identifiers, offsets, stored sizes, and
  alignment.
- [ ] Convert cache misses into physical read requests.
- [ ] Separate useful bytes, alignment amplification, layout amplification,
  duplicate reads, and total physical bytes.
- [ ] Export a deterministic schedule for later replay.
- [ ] Add fixtures for one-file-per-expert, one-file-per-layer, and packed
  aligned blocks.
- [ ] Add optional adjacent-read coalescing behind explicit configuration.

### Exit criteria

- Every physical byte is attributable to a named amplification category.
- Equivalent layouts produce equivalent schedules.
- Hand-calculated offset and alignment cases pass.
- Coalescing never changes the logical experts requested.
- No report labels schedule-derived byte counts as measured time.

### Gate

Stop after M3 if deterministic schedules already answer the layout question.
Before starting M4, re-estimate the calibration scope, available devices, and
current literature; do not add timing merely to continue the roadmap.

## Milestone 4 — Calibrated storage simulation (`v0.4`)

**Outcome:** schedules receive device-specific timing estimates from measured
service-time profiles.

### Entry conditions

- Milestone 3 schedules are stable and replayable.
- One initial operating system and I/O mode are selected.
- The calibration protocol and held-out validation method are documented before
  results are measured.

### Deliverables

- [ ] Implement calibration by read size, queue depth, alignment, and access
  pattern.
- [ ] Mark every profile as measured, estimated, or synthetic.
- [ ] Implement deterministic interpolation between measured points.
- [ ] Implement a discrete-event storage queue with bounded concurrency.
- [ ] Model request submission, start, completion, priority, and coalescing.
- [ ] Report service time, queue depth, utilization, and limitations.
- [ ] Compare predictions with held-out calibration samples.

### Exit criteria

- The acceptance threshold is fixed before held-out evaluation.
- Prediction error is reported by read size and queue depth.
- Equal byte counts with different request patterns may produce different,
  explainable estimates.
- Reports refuse to treat another machine's profile as local measurement.
- Limitations and failed calibration regions are visible.

### Gate

If held-out error is not useful for policy ranking, improve or narrow the model.
Do not proceed to richer timing models.

## Milestone 5 — Physical replay validation (`v0.5`)

**Outcome:** simulated schedules are compared with real storage execution and
receive a documented accuracy envelope.

### Delivery order

1. Portable buffered reads on one operating system.
2. One platform-specific asynchronous or direct-I/O backend.
3. A second physical device.
4. A second operating-system backend only when needed.

Linux `io_uring` and Apple-specific I/O remain separate adapters. Neither is
required for the logical simulator or the first replay result.

### Deliverables

- [ ] Generate synthetic expert files from a model manifest.
- [ ] Replay exported schedules with bounded concurrency.
- [ ] Pre-register warm-cache and cold-cache procedures, request ordering,
  cache-reset steps, and device power-state controls.
- [ ] Keep calibration workloads separate from held-out trace-derived replay
  schedules.
- [ ] Run at least 20 repetitions per scenario in randomized order.
- [ ] Preserve raw per-request observations and run-level metadata.
- [ ] Record predicted and measured timelines.
- [ ] Report per-read and aggregate error, dispersion, tail behavior, and
  confidence intervals.
- [ ] Recalibrate without changing acceptance criteria after seeing results.

### Exit criteria

- Runs record hardware, OS, filesystem, I/O mode, cache state, and profile
  revision.
- Calibration and held-out replay artifacts are disjoint and checksummed.
- Raw observations and all repetition results remain available for audit.
- Predicted and measured results can be compared from saved artifacts.
- The error envelope includes poor cases and tail behavior.
- The project makes no cycle-accuracy claim.

### Gate

If replay cannot validate policy rankings within a useful error envelope,
narrow or improve the storage model before adding prefetch or compute overlap.

## Milestone 6 — Prefetch and compute overlap (`v0.6`)

**Outcome:** the simulator distinguishes blocking I/O from I/O hidden behind
explicit compute assumptions, on top of a physically checked storage model.

### Deliverables

- [ ] Label compute profiles as measured, estimated, or synthetic.
- [ ] Add demand and prefetch priorities.
- [ ] Track submission, completion, consumption, lateness, waste, and
  pollution.
- [ ] Model dependency-driven compute readiness.
- [ ] Implement previous-token reuse and external prediction-file policies.
- [ ] Add an optional oracle prefetch bound.
- [ ] Report blocking time, hidden time, idle compute, and queue contention.
- [ ] Extend physical replay with calibrated synthetic compute delays.

### Exit criteria

- Late prefetches do not count as useful hits.
- Unused prefetches consume storage and cache resources.
- Demand requests can be delayed by speculative traffic.
- Perfect overlap is not a default assumption.
- Every latency report identifies compute-profile provenance.
- At least one overlap scenario is checked by physical replay; unvalidated
  scenarios are labeled simulation-only.

## Milestone 7 — Public research release (`v1.0`)

**Outcome:** stable documented interfaces and a reproducible comparative study.

### Deliverables

- [ ] Stabilize the Rust API and CLI only after real consumers exercise them.
- [ ] Extract crates only where dependency boundaries have become real.
- [ ] Document how to add a policy, adapter, manifest, and report format.
- [ ] Add capability declarations for metadata-dependent policies.
- [ ] Add a second dataset only for a distinct validation question.
- [ ] Publish reproducible benchmark definitions and checksums.
- [ ] Re-evaluate prior art and narrow the novelty statement.
- [ ] Compare policies, budgets, layouts, and validated device profiles.

### Exit criteria

- A clean checkout reproduces public examples.
- Public APIs and examples pass rustdoc checks.
- Dataset and model assumptions are visible in every published result.
- Physical claims stay within the measured accuracy envelope.
- The repository is useful without gated datasets.

## Deferred ideas

- dynamic layer quotas;
- segmented LRU and larger policy catalogs;
- scalable offline bounds for variable-size general caching;
- learned expert predictors;
- mixed-precision residency;
- compression-aware service models;
- general-purpose plugin ABI;
- production inference-engine integration;
- CUDA kernels;
- distributed simulation;
- dashboards and hosted services.

## Kira execution model

Each bounded slice should be a separate, operator-gated Kira workflow rather
than one long autonomous run.

Recommended review axes for a three-worker pool:

1. **Implementation:** smallest test-first slice with exact ownership.
2. **Correctness:** invariants, oracles, edge cases, and error behavior.
3. **Reproducibility:** provenance, reports, documentation, and scope control.

A slice closes only after focused verification, independent review, saved
evidence, and an operator decision. Publish and merge remain separate gates.

## Next approved planning target

The first implementation plan should cover **Milestone 0 only**. It should name
exact files, tests, commands, and commits. Milestone 1 receives separate plans
for slices 1A–1D after the contracts and repository policy pass review.
