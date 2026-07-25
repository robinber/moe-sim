# kira-mux command reference (moe-sim)

Canonical binary: `kira-mux`. Prefer live help when flags differ by install:

```bash
kira-mux --help
kira-mux <command> --help
```

## Project selection

| Target | Meaning |
|---|---|
| `moe-sim` | Explicit project id from `~/.config/kira-mux/projects/moe-sim.toml` |
| `.` | Deepest configured root containing the current working directory |

Example from a crate subdirectory:

```bash
cd ~/Desktop/Projects/moe-sim/crates/moe-sim-core
kira-mux status .
kira-mux send . codex "…"
```

## Lifecycle

| Command | Purpose |
|---|---|
| `list` | Configured projects and live state |
| `open <project>` | Create/repair workspace and attach |
| `start <project>` | Create/repair without attach |
| `attach <project>` | Attach to existing session |
| `status <project>` | Workspace + agent pane state |
| `agents <project>` | Agent table (command, state, capabilities) |
| `restart <project> [agent]` | Restart all agents or one id |
| `kill <project> --yes` | Tear down managed session |

moe-sim currently has **no profiles**. If a future config adds
`[profiles.<name>]`, pass `--profile <name>` on commands that accept it.

## Send

```bash
kira-mux send <project> <agent> <prompt>
  [--profile <profile>]
  [--no-template]
  [--from <from>]          # default: user
  [--trace-id <id>]
  [--thread <thread>]
```

- `<agent>` for moe-sim: `claude` | `codex` | `grok`
- Prompt is delivered to a **live** pane; readiness is not checked
- Use heredocs for multi-line tasks
- `--from`, `--thread`, and `--trace-id` support orchestration bookkeeping when
  the install has thread/msgbus features enabled

## Capture

```bash
kira-mux capture <project> <agent>
  [--lines <n>]            # default: 30
  [--json]
  [--profile <profile>]
  [--save-thread <thread>]
  [--save-profile <save_profile>]
  [--trace-id <id>]
```

Use enough `--lines` to cover the reply. Prefer saving thread evidence when
coordination requires an audit trail.

## Config locations

| Path | Role |
|---|---|
| `~/.config/kira-mux/config.toml` | Global defaults and agent templates |
| `~/.config/kira-mux/projects/moe-sim.toml` | moe-sim agents and root |

## Drift

Fingerprint includes project id, profile id, root, layout, main pane ratio,
window name, shell/remain-on-exit defaults, and per-agent mode, command,
shell_command, args, cwd, and env (literal values hashed). Mismatch → drifted
session; fix with `kill` then `open`/`start`.

Excluded from fingerprint: display `name`/`label`, `capabilities`, `groups`,
`prompt_template`.

## moe-sim agent launch args

From the active project file (verify before relying on memory):

```toml
# claude
args = ["--dangerously-skip-permissions"]

# codex
args = ["-a", "never", "-s", "danger-full-access"]

# grok
args = ["--always-approve", "--permission-mode", "bypassPermissions"]
```

## Related repo docs

- `AGENTS.md` — Kira orchestration rules and completion checklist
- `ROADMAP.md` — Kira execution model and review axes
- `README.md` — high-level development workflow
- `.agents/skills/rust-strict/SKILL.md` — required for Rust work on any pane
