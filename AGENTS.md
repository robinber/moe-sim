# AGENTS.md

Machine-facing engineering rules for this repository. Humans start with
[`README.md`](README.md).

## Project status

**`v0.1` complete. Project parked.** No active milestone, no implicit next task.

- Do **not** implement items from [`docs/ideas.md`](docs/ideas.md) unless the
  operator opens a **new** bounded scope with an explicit stop condition.
- Do **not** invent unfinished work from historical milestones.
- Do not describe planned commands, crates, formats, or results as implemented.

## Documentation authority

Full map: [`docs/README.md`](docs/README.md). Short form:

| Topic | Authority |
| --- | --- |
| Product status / quick start | [`README.md`](README.md) |
| Simulation contracts | [`docs/contracts.md`](docs/contracts.md) |
| CLI examples | [`docs/cli.md`](docs/cli.md) |
| Milestone history | [`docs/history.md`](docs/history.md) |
| Parked ideas / non-goals | [`docs/ideas.md`](docs/ideas.md) |
| Agent + Rust + verification rules | **this file** |

If documents disagree, stop and fix the authority file. Do not silently choose
the interpretation that permits more work.

## Load order

1. This file (status, architecture, Rust policy, verification).
2. [`docs/contracts.md`](docs/contracts.md) before any change to replay,
   policies, capacity, metrics, or reports.
3. [`README.md`](README.md) for product shape.
4. Shared Rust skill **rust-strict** (v1.3.0+) before any Rust or Cargo work.
   One canonical checkout; Claude/Grok paths are symlinks to it:

   | Tool | Path |
   | --- | --- |
   | Codex (canonical submodule) | [`.agents/skills/rust-strict/SKILL.md`](.agents/skills/rust-strict/SKILL.md) |
   | Claude Code | [`.claude/skills/rust-strict/SKILL.md`](.claude/skills/rust-strict/SKILL.md) → symlink |
   | Grok | [`.grok/skills/rust-strict/SKILL.md`](.grok/skills/rust-strict/SKILL.md) → symlink |

   Source: https://github.com/robinber/agent-skills-rust (pin tag, currently `v1.3.0`)
5. [`.agents/skills/kira/SKILL.md`](.agents/skills/kira/SKILL.md) before
   Kira multi-agent coordination.
6. Rust policy files: [`Cargo.toml`](Cargo.toml),
   [`rust-toolchain.toml`](rust-toolchain.toml),
   [`.cargo/config.toml`](.cargo/config.toml),
   [`.rustfmt.toml`](.rustfmt.toml), [`clippy.toml`](clippy.toml),
   [`deny.toml`](deny.toml).
7. Code and tests next to the change; CLI detail in [`docs/cli.md`](docs/cli.md).

## Workspace

- Cargo workspace, resolver `3`, edition `2024`, Rust `1.97.0`
  (`rust-toolchain.toml` + workspace package metadata).
- License MIT; workspace packages `publish = false`.
- Crates: `moe-sim-core`, `moe-sim-cli` (dependency: CLI → core).
- Layout: `crates/`, `fixtures/{synthetic,models}/`, `docs/`.
- New crates need multiple consumers, isolated verification, or a clear
  ownership boundary; otherwise keep the two-crate layout.
- New workspace members inherit `[lints] workspace = true`.
- Commit `Cargo.lock` (application workspace).
- Nightly only when required (repo fmt uses `cargo +nightly fmt`).
- Drift profile: rust-strict defaults (800 / 1000 LOC, ≤ 6 params) unless
  overridden here.
- `clippy::pedantic` is workspace-wide policy here — do not weaken without an
  explicit operator decision.

## Architecture boundaries

### `moe-sim-core`

Owns domain types, event validation, deterministic replay, cache policies,
capacity accounting, metrics, offline reference solvers, and domain errors.

Must not depend on CLI parsing/presentation, filesystem adapters, async
runtimes, physical storage APIs, CUDA, inference runtimes, HTTP, or plugins.
Inputs are explicit values or streams from callers. Equal inputs and
configuration produce equal results.

### `moe-sim-cli`

Owns CLI parsing, file-format adapters, configuration loading, report
rendering, and composition of core use cases. Keep policy logic and simulation
semantics out of this crate. No dynamic plugin ABI without real external
consumers.

## Simulation contracts

**Do not maintain a second full copy here.** Load and obey
[`docs/contracts.md`](docs/contracts.md).

In one line: file-order replay, atomic pinned active sets, capacity rejection
before results, byte budgets (global or fixed per-layer quotas), separate
object/byte metrics, deterministic reports, no silent approximation, Belady
only as a labeled offline uniform-size reference with a bounded oracle for
tiny cases.

Any invariant change updates `docs/contracts.md`, fixtures, tests, and
(if needed) `docs/cli.md` in the same change set.

## Engineering rules

- Smallest change that satisfies the approved request.
- One bounded slice at a time; prove the gate; stop.
- Test-first when practical; for bugs, reproduce before editing.
- Match the style of the file being edited; leave unrelated drift untouched.
- No secrets, gated datasets, large generated results, machine-specific
  profiles, or private agent/runtime state in git.
- Prefer simplicity; if a senior engineer would call it overcomplicated, simplify
  before claiming completion.
- Prefer workspace-level dependencies; add none without a current use and a
  license/source-policy check.
- Secrets from environment or untracked local config only; redact from logs,
  reports, diagnostics, and provenance.

## Rust policy

Denied in non-test Rust code:

- `unsafe_code`;
- `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`, `dbg!`;
- undocumented public items (unless a crate states a narrower contract).

Cargo lints enforce most of this; `panic!` is banned by repository rule even
where a lint cannot catch every case. Tests may use stronger assertions and
intentional panics for diagnostics.

Preferred patterns:

- `thiserror` in libraries; `anyhow` only in binaries / entrypoints.
- Typed, actionable errors for malformed inputs and unsupported combinations.
- Thin `main.rs`; testable modules hold behavior.
- Core stays synchronous and deterministic; async only at a measured I/O need.
- Local `#[expect(..., reason = "...")]` only; no broad lint allowances.
- `clippy::pedantic` is workspace-wide; only selected restriction rules apply
  (see `Cargo.toml`).

Details and deeper Rust guidance: the shared **rust-strict** skill (see load
order paths). Product contracts, crate boundaries, and parked status stay in
this file and `docs/`.

## Verification

Impact-scoped by default: run the narrowest checks that exercise the change,
then widen when evidence is insufficient.

```bash
cargo +nightly fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo deny check advisories licenses sources
```

Aliases: `lint`, `lint-app`, `lint-pedantic`, `test-all`, `doc-all`, `deny-all`
(see [`.cargo/config.toml`](.cargo/config.toml)).

Focused during implementation:

```bash
cargo test -p moe-sim-core <filter>
cargo test -p moe-sim-cli <filter>
```

Widen when public APIs, invariants, core/CLI boundary, workspace policy,
dependencies, or report contracts change. Documentation-only edits need
link/content review; run broader gates only if they could invalidate prior
evidence.

Only claim a command passed if it was run and the output checked. A pass is
evidence for its actual scope only.

## Change workflow

**Feature** (only after an explicit new scope):

1. One question and a hard stop condition.
2. Smallest failing test or fixture when practical.
3. Implement only enough for the slice.
4. Impact-scoped verification + doc updates to the authority files.
5. Stop — do not chain into parked ideas.

**Bug:**

1. Reproduce or localize.
2. Regression test or fixture when practical.
3. Smallest fix.
4. Re-check the failure and neighboring invariants.

Kira multi-agent work: load
[`.agents/skills/kira/SKILL.md`](.agents/skills/kira/SKILL.md). Prefer
supervised slices; after a closed gate, **pause and re-decide**.

## Research and public claims

- Re-evaluate novelty against current literature before any public claim.
- State objective, applicability, provenance, hardware, and limits.
- Do not call an algorithm optimal outside the objective actually proven or
  exhaustively checked.
- Do not present estimates as measurements.
- Keep generated benchmarks out of git unless they are small, intentional,
  reproducible release artifacts.

## Completion checklist

- [ ] Operator-approved slice (not an unrequested parked idea).
- [ ] No deferred surface or unnecessary crate.
- [ ] [`docs/contracts.md`](docs/contracts.md) still holds (or was deliberately
      updated with fixtures/tests).
- [ ] Determinism and provenance preserved.
- [ ] Public items and authority docs updated where behavior changed.
- [ ] Relevant fmt / clippy / test / rustdoc / deny checks run and recorded.
- [ ] Unrelated worktree changes left untouched.
- [ ] Stop condition explicit; no assumed next milestone.
