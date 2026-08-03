# Contracts and correctness

**Authority** for simulation inputs, metrics, and invariants. Other docs may
summarize; they must not contradict this page.

These contracts ship with `v0.1`. They are correctness requirements, not
implementation suggestions. Changing them is a shared-contract change and needs
an explicit design decision, updated documentation (including this file),
adversarial fixtures, and independent review.

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

Canonical events are replayed in **file order**. `step_id`, `token_position`,
and `layer_id` validate source structure but do not silently reorder events.
`expert_ids` is one **atomic active set**: all unique experts in the event must
be resident together, remain pinned until that event completes, and are then
released before the next event. Duplicate expert identifiers in one event are
invalid.

### Model manifest

The `v0.1` manifest maps each `(layer_id, expert_id)` pair to one logical size
in bytes, used consistently for both cache capacity and load accounting. Sizes
must be strictly positive; duplicate keys are invalid; lookups for undeclared
experts fail rather than inventing a size. It does not model a checkpoint whose
stored precision differs from its resident cache precision.

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
tool belongs to synthetic input generation (`trace generate --pattern random`)
and is recorded in that command's report.

Policy and scope are independent. A global cache may use the full budget. A
per-layer cache requires an explicit quota for every simulated layer; quotas
must sum to no more than the total budget and unused quota is not shared in
`v0.1`. The same LRU or LFU policy runs within the selected scope.

If one expert exceeds its applicable capacity, or an atomic active set cannot
fit in its applicable cache, the configuration is rejected. The simulator does
not bypass the cache or partially execute the event.

Every report records its inputs and their versions or checksums.

## Metric definitions

Two definitions matter for fair comparison:

**A release is not an eviction.** Eviction is the capacity-driven removal of an
object a policy chose to retain. A policy that retains nothing therefore
evicts nothing, which keeps the baseline comparable with the caching policies
measured against it.

**Churn is rework, not turnover.** `object_reloads` and `byte_reloads` count
loads of an expert that was loaded earlier in the run and is no longer
resident, so `object_loads` splits into unavoidable cold misses plus reloads.
That separation distinguishes a policy that thrashes from one that simply faces
a large working set.

Churn counts the rework, not its cause. A retaining policy loses residency by
eviction; `no-cache` loses it by releasing the active set. The baseline
therefore reports reloads while its `evictions` stay `0` — that pairing is
intentional.

### Policy-specific rules

- `lfu` breaks ties by least recent use. A frequency count belongs to a
  resident entry: it restarts when an object is admitted again, so a once-hot
  expert does not become immortal after eviction. One atomic active set counts
  as one access. A genuine remaining tie evicts the lowest expert key first.
- `belady` is an offline reference, not an online policy: it evicts the expert
  whose next use is farthest away (never-reused experts first, ties by lowest
  expert key) and needs whole-trace lookahead. It only accepts manifests whose
  experts all share one size. Reports carry an `objective:` line so the numbers
  never pose as an online policy's outcome.

## Correctness principles

The logical simulator must satisfy these invariants:

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

Under a per-layer scope, aggregate `peak_resident_bytes` is the high-water mark
of **summed** residency across layer caches, not the sum of the per-layer peaks:
caches that fill at different times must not overstate simultaneous residency.

## Offline baselines

Classic Belady MIN is proven optimal for single-object requests of uniform
size. Atomic active sets fall outside that classic model, so `moe-sim` does
not call the shipped Belady policy optimal by proof. It is checked against a
deliberately bounded exhaustive oracle (at most 12 events and 8 distinct
experts) that explores every eviction schedule; greedy farthest-next-use matches
the oracle's optimum on every enumerated uniform-size case.

A manifest with more than one expert size is rejected rather than approximated,
because general variable-size caching has no greedy byte-aware optimum. The
general offline caching problem with different object sizes or fetch costs is
NP-hard.

`v0.1` uses classic Belady on uniform-size fixtures and a deliberately bounded
exhaustive solver for tiny variable-size correctness tests (test-only). Practical
bounds for large variable-size traces were never in scope.

References:

- [Practical Bounds on Optimal Caching with Variable Object Sizes](https://arxiv.org/abs/1711.03709)
- [General Caching Is Hard: Even with Small Pages](https://arxiv.org/abs/1506.07905)

Related work on MoE caching (context only; not claimed as novelty for this
project):

- [Cache Management for Mixture-of-Experts LLMs](https://arxiv.org/abs/2509.02408)
- [In-depth Analysis on Caching and Pre-fetching in Mixture of Experts Offloading](https://arxiv.org/abs/2511.05814)

## Data and reproducibility

- Large or gated datasets stay outside source control.
- Small deterministic, redistributable fixtures may be committed when they are
  necessary for tests and documentation.
- A real-data experiment must record dataset revision, selected files,
  checksums, adapter version, and every transformation applied.
- Preserve source ordering and explicit boundaries. Missing prefill/decode
  phase remains `unknown`; never infer it silently.
- Label every input or model component as measured, estimated, or synthetic.
- Treat device profiles as machine-specific. Never present one machine's
  measurements as portable defaults.
- Preserve raw observations and failed or poor cases needed to audit a reported
  error envelope.
- Do not publish or imply latency accuracy without calibration, held-out
  validation, and physical replay under an explicitly opened scope.

## Changing these contracts

1. State the contract change and why the current rule is wrong or insufficient.
2. Update this file, fixtures, and tests in the same change set.
3. Update CLI examples in [cli.md](cli.md) when report fields or behavior change.
4. Keep [AGENTS.md](../AGENTS.md) free of a second full copy of the rules —
   agents load this page for simulation correctness.
