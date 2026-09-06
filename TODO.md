# TODO

**The work queue lives in Beads now, not in this file.**

```bash
bd ready              # what can be worked on
bd list --status=open # everything open
bd show <id>          # one issue in full
```

## Why this file is a pointer

`AGENTS.md` draws the line: **Git stores implementation state, Beads stores
operational work state, and project documentation stores durable technical
knowledge.** A backlog is operational work state, so it belongs in Beads.

It used to live here *and* in `PROJECT_STATUS.md`'s "Next actions" *and* in
Beads, which meant three copies drifting apart. Every item from this file was
migrated on 2026-09-06; nothing was dropped. The full history of what this file
used to say is in `git log -- TODO.md`.

## What did not move

Measured results, board facts, numerical contracts and the reasoning behind
settled decisions are **durable technical knowledge**, and they stay in the
documents that own them:

| | |
| --- | --- |
| [`PROJECT_STATUS.md`](PROJECT_STATUS.md) | current state, environments, loose ends, history |
| [`POC-PLAN.md`](POC-PLAN.md) | the staged route and what each stage adds |
| [`AGENTS.md`](AGENTS.md) | purpose, boundaries, the decision filter |
| [`backend/AGENTS.md`](backend/AGENTS.md) | the numerical contract and the f32 bound |
| [`firmware/AGENTS.md`](firmware/AGENTS.md) | the target runtime architecture |
| [`firmware/ctrl/README.md`](firmware/ctrl/README.md) | what the runtime does, and what the board measured |
| [`firmware/BRINGUP.md`](firmware/BRINGUP.md) | board facts and the anomalies not to re-debug |

A rule of thumb for anything new: if it says **what to do next**, it is a bead.
If it says **what is true about this system**, it is documentation.
