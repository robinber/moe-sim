---
name: kira
description: >
  Use when coordinating work through Kira (kira-mux): opening the moe-sim
  multi-agent workspace, sending prompts to claude/codex/grok panes, capturing
  output, checking status, restarting or killing sessions, or following the
  supervised slice orchestration model. Trigger on kira, kira-mux, multi-agent
  workspace, dispatch to agents, or tmux agent panes.
---

# Kira (moe-sim)

Kira (`kira-mux`) is a local **tmux multi-agent workspace**. Agents are real
panes you can attach to, watch, and type into. Config is XDG TOML; the CLI
launches, inspects, sends, captures, restarts, and kills sessions.

This skill is the operator/agent contract for **moe-sim**. Load it before
dispatching work through Kira or claiming that a Kira-coordinated slice is done.

## Project config

Active project file (user machine, not in-repo):

```text
~/.config/kira-mux/projects/moe-sim.toml
```

Global templates (optional; moe-sim uses explicit commands):

```text
~/.config/kira-mux/config.toml
```

| Field | Value |
|---|---|
| Project id | `moe-sim` |
| Root | `~/Desktop/Projects/moe-sim` |
| Layout | `side-by-side` |
| Profile | none (default workspace) |

### Agents (allow-all)

| Agent id | Command | Permission bypass |
|---|---|---|
| `claude` | `claude` | `--dangerously-skip-permissions` |
| `codex` | `codex` | `-a never -s danger-full-access` |
| `grok` | `grok` | `--always-approve --permission-mode bypassPermissions` |

These flags auto-approve tool use so unattended `send` is not blocked by
permission prompts. They do **not** make the agent input-ready after cold start
(trust dialogs, login, first-run UI still need an operator once).

## Running vs input-ready

| Term | Meaning |
|---|---|
| **running** | tmux pane process is alive |
| **input-ready** | agent TUI past setup and will treat paste as a task |

`status` / `agents` report pane liveness only. `send` refuses **dead** panes but
will paste into a setup UI if the agent is not ready. Cold start is
operator-managed.

## Cold-start workflow

```bash
# From anywhere, or use `.` inside the repo root
kira-mux open moe-sim
# In each pane: finish trust/login/first-run UI until the normal prompt is ready
# Detach: Ctrl-b d

kira-mux status moe-sim
kira-mux agents moe-sim
```

Once bootstrapped, prefer `start` (no attach) or reuse the live session.
If config drifts the live session, `kill` then `open`/`start` again.

## Everyday commands

Project id may be `moe-sim` or `.` when the cwd is under the configured root.

```bash
# Workspace lifecycle
kira-mux list
kira-mux open moe-sim          # create/repair + attach
kira-mux start moe-sim         # create/repair without attach
kira-mux attach moe-sim
kira-mux status moe-sim
kira-mux agents moe-sim
kira-mux restart moe-sim       # all agents
kira-mux restart moe-sim claude
kira-mux kill moe-sim --yes

# Dispatch and inspect
kira-mux send moe-sim claude "bounded task text…"
kira-mux send moe-sim codex "…"
kira-mux send moe-sim grok "…"
kira-mux capture moe-sim claude --lines 80
kira-mux capture moe-sim codex --lines 80
kira-mux capture moe-sim grok --lines 80
```

Optional send metadata when orchestration needs traceability:

```bash
kira-mux send moe-sim codex "…" --thread <thread> --trace-id <id> --from orchestrator
```

Optional capture persistence onto a thread (when msgbus/thread features are
configured on this install):

```bash
kira-mux capture moe-sim grok --lines 120 --save-thread <thread> --trace-id <id>
```

Deeper CLI notes: `references/commands.md`.

## Orchestration model (repo contract)

From `AGENTS.md` / `ROADMAP.md` — supervised, traceable slices:

1. Propose **one bounded slice** tied to a roadmap gate.
2. Wait for **operator approval** before worker dispatch.
3. **One worker implements** unless workstreams are truly independent.
4. **Independent reviewers** cover correctness and reproducibility.
5. Record prompts, captures, commands, results, decisions, and gaps.
6. **Pause** before scope expansion, merge, publication, or irreversible actions.

Suggested role split for the three-pane pool:

| Agent | Typical axis |
|---|---|
| `codex` or `claude` | implementation (test-first, smallest ownership) |
| the other of codex/claude | correctness (invariants, edge cases, errors) |
| `grok` | reproducibility / independent review (provenance, scope, docs) |

Do **not** treat worker completion as proof a gate passed. Close a slice only
from reviewable evidence (tests run, captures, explicit remaining gaps).

## Prompting rules when dispatching

When composing `send` prompts for moe-sim workers:

1. Name the **roadmap slice / gate** and out-of-scope boundaries.
2. Point at `AGENTS.md`, `ROADMAP.md`, and `.agents/skills/rust-strict/SKILL.md`
   for any Rust change.
3. Require the smallest change that satisfies the slice; no deferred surfaces.
4. Require impact-scoped verification and exact command evidence.
5. Ask for a short completion report: what changed, commands run, gaps left.
6. Reviewers must not re-implement; they assess correctness/reproducibility and
   list concrete findings.

Example implementation dispatch:

```bash
kira-mux send moe-sim codex "$(cat <<'EOF'
Slice: Milestone 0 / <slice-id> only. Read AGENTS.md, ROADMAP.md, and
.agents/skills/rust-strict/SKILL.md before editing.

Task: <one bounded outcome>.
Out of scope: <explicit non-goals>.

Work test-first when practical. Smallest change. Report exact cargo commands
and remaining gaps. Do not merge or expand scope.
EOF
)"
```

Example review dispatch:

```bash
kira-mux send moe-sim grok "$(cat <<'EOF'
Independent review only. Do not implement.

Scope: <files / slice>. Axes: correctness + reproducibility per AGENTS.md.
Check simulation invariants, verification evidence, and deferred-surface creep.
Return findings ordered by severity with file references.
EOF
)"
```

## Config drift and edits

- Live sessions store a **config fingerprint**. Changing agent command/args/cwd/env
  (literals), layout, root, or project id drifts the session → `kill` then
  `open`/`start`.
- Cosmetic fields (`name`, `label`, `capabilities`) do not drift.
- Project config lives under `~/.config/kira-mux/projects/` (not the git tree).
  Do not commit machine-local paths or secrets into the repo.
- If the installed `kira-mux` help disagrees with this skill, trust
  `kira-mux <cmd> --help` and the live config file.

## Safety

- Allow-all agents can edit files and run commands without per-tool prompts.
  Keep dispatches bounded; never send open-ended “fix everything” goals.
- Operator remains the gate for merge, publish, force-push, and irreversible
  actions even when agents can execute tools freely.
- Prefer `capture` evidence over paraphrasing pane output.

## Activation checklist

Before claiming Kira coordination is in use or complete:

1. Confirm project resolves: `kira-mux agents moe-sim` (or `.`).
2. Confirm agents are **running** and operator-confirmed **input-ready**.
3. Dispatch one bounded, approved slice at a time.
4. Capture replies; do not invent results from `running` state alone.
5. Close only with verification evidence and an explicit operator decision.
