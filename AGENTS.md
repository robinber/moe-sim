# AGENTS.md

Machine-facing development guidance for coding agents working in this
repository. Humans should start with [`README.md`](README.md); this file exists
to make the engineering, simulation, and verification contracts explicit.

## Load order

1. This file — repository-wide agent rules.
2. [`README.md`](README.md) — product scope, architecture, and correctness
   principles.
3. [`ROADMAP.md`](ROADMAP.md) — exploratory stance, default path (M0→M1), and
   optional later directions (not a mandatory tunnel).
4. [`.agents/skills/rust-strict/SKILL.md`](.agents/skills/rust-strict/SKILL.md)
   — required before changing, reviewing, debugging, or claiming verification
   for Rust or Cargo work.
5. [`.agents/skills/kira/SKILL.md`](.agents/skills/kira/SKILL.md) — required
   before opening, dispatching, capturing, or claiming completion for
   Kira-coordinated multi-agent work.
6. Rust policy files: [`Cargo.toml`](Cargo.toml),
   [`rust-toolchain.toml`](rust-toolchain.toml),
   [`.cargo/config.toml`](.cargo/config.toml),
   [`.rustfmt.toml`](.rustfmt.toml), [`clippy.toml`](clippy.toml), and
   [`deny.toml`](deny.toml).
7. Subsystem documentation next to the code being changed.

When these documents appear to disagree, stop and surface the conflict. Do not
silently choose the interpretation that permits more work.

## Current workspace facts

- Cargo workspace with resolver `3`, edition `2024`, and Rust `1.97.0`.
- Source license: MIT. Workspace packages are not published by default.
- The intended initial layout is deliberately small:

  ```text
  crates/
    moe-sim-core/    pure replay, cache policies, metrics, domain errors
    moe-sim-cli/     file adapters, commands, report rendering
  fixtures/
    synthetic/
    models/
  docs/
  ```

- Initial dependency direction: `moe-sim-cli -> moe-sim-core`.
- Every future workspace crate must inherit workspace lint policy with
  `[lints] workspace = true`.
- `Cargo.lock` must be committed once generated because the workspace contains
  an application.

Do not describe planned commands, crates, formats, or results as implemented.

## Working rules

- This repository is **exploratory**. Prefer a finished narrow slice over
  advancing the roadmap. Stopping after `v0.1` is a valid success.
- Make the smallest change that satisfies the approved request.
- Work one bounded slice at a time and satisfy its gate before starting another.
- Do **not** auto-chain into the next milestone. After a gate, the operator
  chooses continue / side quest / pause.
- Do not implement optional-exploration infrastructure (real datasets, storage
  timing, prefetch, multi-device calibration) early “because it is on the
  roadmap.” Those items are a menu, not a queue.
- Follow existing boundaries before introducing new abstractions.
- Start with the two planned crates. Add a crate only when it has multiple real
  consumers, requires isolated dependencies or verification, or represents a
  demonstrated ownership boundary.
- Work test-first when practical. For bugs, reproduce or localize the root
  cause before editing.
- Match the style of the file being edited.
- Mention unrelated drift and leave it untouched.
- Do not commit secrets, gated datasets, large generated results,
  machine-specific profiles, or private agent/runtime state.
- Self-check every design: if a senior engineer would call it overcomplicated,
  simplify it before claiming completion.

## Rust contract

Denied in non-test Rust code:

- `unsafe_code`;
- `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`, and `dbg!`;
- undocumented public items, unless a crate has an explicit narrower contract.

The Cargo lint tables enforce most of this policy. The ban on `panic!` is also
a repository rule even where a lint cannot enforce every occurrence. Tests may
use stronger assertions and intentional panics when they improve diagnostics.

Preferred patterns:

- Use `thiserror` for library errors. Use `anyhow` only in binaries and
  application entrypoints.
- Return typed, actionable errors for malformed traces, incompatible
  configurations, and unsupported comparisons.
- Keep `main.rs` thin; put behavior in testable modules.
- Keep the core synchronous and deterministic. Add async only at a measured
  I/O boundary that establishes a real need.
- Prefer workspace-level dependencies over repeated per-crate declarations.
  Add no dependency without a current use and a license/source-policy check.
- Keep intentional lint exceptions local with
  `#[expect(..., reason = "...")]`; do not add broad allowances.
- `clippy::pedantic` is enabled workspace-wide.
  `clippy::restriction` is not enabled as a group; only the selected
  low-noise rules in `Cargo.toml` apply.
- Read secrets from environment variables or untracked local configuration and
  redact them from logs, reports, diagnostics, IPC, and provenance.

## Architecture boundaries

### `moe-sim-core`

Owns pure domain types, canonical event validation, deterministic replay,
cache policy behavior, capacity accounting, metrics, offline reference
solvers, and domain errors.

The core must not depend on:

- CLI parsing or presentation;
- filesystem-specific adapters;
- async runtimes;
- physical storage APIs;
- CUDA, inference runtimes, HTTP services, or plugin systems.

Inputs should be explicit values or streams supplied by callers. Equal inputs
and configuration must produce equal results.

### `moe-sim-cli`

Owns command-line parsing, file-format adapters, configuration loading, report
rendering, and composition of core use cases. Keep policy logic and simulation
semantics out of this crate.

Do not add a dynamic plugin ABI before the roadmap establishes stable contracts
and real external consumers.

## Simulation invariants

These rules are correctness requirements, not implementation suggestions:

- Replay canonical events in file order. Metadata such as `step_id`,
  `token_position`, and `layer_id` validates source structure but must not
  silently reorder events.
- Treat the unique `expert_ids` in one event as one atomic active set. All must
  be resident together and pinned until the event completes. Duplicate expert
  identifiers in one event are invalid.
- Reject a configuration before emitting simulation results if an expert or an
  atomic active set exceeds its applicable capacity. Never bypass the cache,
  partially admit an event, or invent fallback behavior.
- Resident bytes must never exceed the selected total or per-layer capacity.
- Policy and cache scope are independent. A global cache uses the total budget.
  A per-layer cache requires explicit quotas whose sum does not exceed that
  budget; unused quota is not shared in `v0.1`.
- Deterministic inputs and seeds must produce byte-identical machine-readable
  reports.
- Report object and byte metrics separately. Do not let object-hit rate conceal
  expensive misses on larger experts.
- Reject unsupported policy/input combinations with an actionable error. Never
  approximate silently.
- Classic Belady MIN is an optimum only for the declared uniform-size
  objective. Variable-size general caching is not represented by a greedy
  "byte-aware Belady" optimum.
- Use a deliberately bounded exhaustive oracle only for tiny variable-size
  correctness cases, and make its objective and limits explicit.

Any change to these invariants is a shared-contract change. It requires an
explicit design decision, updated documentation, adversarial fixtures, and
independent review.

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
- Do not publish or imply latency accuracy before calibration, held-out
  validation, and physical replay satisfy the corresponding roadmap gates.

## Deferred surfaces

Until a roadmap entry condition explicitly opens them, do not add:

- model-weight or token execution;
- attention, KV caches, tokenizers, CUDA kernels, or HTTP services;
- distributed or multi-node simulation;
- learned predictors;
- dynamic plugins;
- device timing, prefetch, or compute-overlap assumptions inside the logical
  simulator;
- claims of end-to-end inference latency or cycle accuracy.

These are deliberate non-goals for the first releases, not missing scaffolding
to create in advance.

## Commands

Reference quality gates:

```bash
cargo +nightly fmt --all
cargo +nightly fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo deny check advisories licenses sources
```

Cargo aliases in `.cargo/config.toml`: `lint`, `lint-app`, `lint-pedantic`,
`test-all`, `doc-all`, and `deny-all`.

Prefer narrow verification commands scoped to the packages that changed.
Do not claim source or test coverage for packages that do not exist.

## Verification baseline

Default verification is impact-scoped. Run the narrowest checks that exercise
what changed, then widen when evidence is insufficient.

Use focused package or test filters during implementation, for example:

```bash
cargo test -p moe-sim-core <test-filter>
cargo test -p moe-sim-cli <test-filter>
```

Widen static checks when:

- public APIs or simulation invariants change;
- a change crosses the core/CLI boundary;
- workspace policy, dependencies, or report contracts change;
- preparing a release;
- narrow checks cannot demonstrate the roadmap gate.

Workspace-wide tests are appropriate for cross-cutting runtime/shared-contract
changes or when the operator explicitly requests them. Documentation-only and
workspace-policy changes remain impact-scoped unless they invalidate broader
evidence.

Only claim a command passed if it was run and its output was checked. Record
the exact commands, results, and any unverified gaps. A passing command is
evidence for its actual scope, not for unrelated roadmap criteria.

## Feature and bug workflow

For a feature:

1. Identify the current roadmap slice and its gate.
2. Write or approve a bounded plan.
3. Add the smallest failing test or deterministic fixture that expresses the
   contract when practical.
4. Implement only enough to satisfy the slice.
5. Run impact-scoped verification.
6. Review correctness, reproducibility, and documentation separately.
7. Demonstrate the gate before expanding scope.

For a bug:

1. Reproduce or localize the root cause.
2. Add a regression test or fixture when practical.
3. Apply the smallest fix.
4. Verify the original failure and relevant neighboring invariants.

## Working in slices

Load [`.agents/skills/kira/SKILL.md`](.agents/skills/kira/SKILL.md) for
project config, agent ids, CLI commands, cold-start, and dispatch patterns.
When work is coordinated through Kira, use supervised, traceable slices:

Stay small. Prefer one bounded slice tied to an **active** goal (default path
M0/M1, or one chosen optional exploration):

1. State the slice and its stop condition before coding.
2. Implement the smallest change that meets the gate.
3. Prove correctness and reproducibility with tests or fixtures when useful.
4. Record what ran, what passed, and what remains open.
5. Pause before scope expansion, merge, publication, or any irreversible
   action. Default after a closed gate: **pause and re-decide**, not “start the
   next milestone.”

Close a slice only from reviewable evidence. Prefer several small changes over
one long march through the roadmap.

## Research and public claims

- Re-evaluate novelty claims against current literature immediately before
  making them public.
- State the objective, applicability, provenance, hardware, and limitations of
  every comparative result.
- Do not call an algorithm optimal outside the objective that was actually
  proven or exhaustively checked.
- Do not turn estimates into measurements through presentation wording.
- Keep generated benchmark results out of source control unless they are small,
  intentional, reproducible release artifacts.

## Completion checklist

Before claiming a change complete, confirm that:

- it belongs to the active slice (not an unrequested later exploration);
- no deferred / optional-exploration surface or unnecessary crate was introduced;
- simulation invariants still hold;
- deterministic and provenance requirements are covered;
- public items and behavior changes are documented;
- relevant format, lint, test, rustdoc, and dependency-policy checks were run;
- exact verification evidence and gaps are reported;
- unrelated worktree changes were left untouched;
- the next roadmap gate, not merely the implementation task, is explicit.
