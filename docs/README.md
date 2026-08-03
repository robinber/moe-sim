# Documentation

`moe-sim` is a **finished `v0.1` artifact**, parked at its intended stopping
point. These pages hold detail that would bloat the root README.

## Map

| Document | Contents |
| --- | --- |
| [cli.md](cli.md) | Command reference, exit codes, and worked CLI examples |
| [contracts.md](contracts.md) | Inputs, simulation invariants, metrics, Belady limits |
| [learning.md](learning.md) | Hand-worked policy comparison (no-cache vs LRU vs LFU) |
| [history.md](history.md) | Completed Milestone 0 and Milestone 1 record |
| [ideas.md](ideas.md) | Parked explorations — not a backlog |

Start with the root [README.md](../README.md). Human project card:
[PROJECT.md](../PROJECT.md). Agent rules: [AGENTS.md](../AGENTS.md).

## Source of truth

When content appears in more than one place, the **authority** wins. Other
files should summarize and link, not diverge.

| Topic | Authority |
| --- | --- |
| Product status and quick start | [`README.md`](../README.md) |
| Simulation inputs, metrics, invariants | [`contracts.md`](contracts.md) |
| CLI behavior and report examples | [`cli.md`](cli.md) |
| Completed milestones (M0 / M1) | [`history.md`](history.md) |
| Parked ideas and default non-goals | [`ideas.md`](ideas.md) |
| Engineering / agent rules, Rust policy, verification | [`AGENTS.md`](../AGENTS.md) |
| Human summary and reopen checklist | [`PROJECT.md`](../PROJECT.md) |
| Stable roadmap entry (pointers only) | [`ROADMAP.md`](../ROADMAP.md) |

If two documents disagree, stop and fix the authority file first.
