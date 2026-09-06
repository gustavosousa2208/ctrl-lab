# AGENTS.md

## Purpose

`ctrl-app` is a proof of concept for reducing the gap between control-system simulation and real-time embedded execution.

The goal is to let users familiar with Simulink-style workflows model a controller, simulate it on the PC, deploy it to a microcontroller running Zephyr-based firmware, and compare simulated results against measured real-time behavior.

This is not a full Simulink replacement. It is a focused platform for validating model-to-embedded execution with measurable metrics.

## Subprojects

### Frontend

Owns:

- UI
- canvas
- block library
- editor
- project/file management
- visualization

Must remain responsive and reliable.  
Must not own simulation or real-time logic.

### Backend

Owns:

- PC-side simulation
- model validation
- transformation to deployable representation
- communication with the microcontroller
- telemetry routing
- metric calculation and comparison

This is the orchestration layer between frontend and firmware.

Implementation constraints:

- Backend foundation and performance-critical backend logic must be written in Rust.
- Prefer explicit, versioned data contracts between frontend and backend.
- Keep parsing, validation, optimization, simulation, and deployable-model generation in backend-owned Rust modules.

### Firmware

Owns:

- deterministic execution on the microcontroller
- signal processing and actuation
- telemetry generation
- execution of validated deployable representations

The firmware should act as a stable runtime, not require reflashing for routine model changes.

## Architectural rules

- Frontend, backend, and firmware must be decoupled and communicate through explicit contracts.
- The frontend must not depend on firmware internals.
- The backend must not contain UI concerns.
- The firmware must not depend on frontend concepts.
- Real-time requirements increase toward the microcontroller:
  - frontend: no real-time guarantees
  - backend: soft real-time at most
  - firmware: deterministic execution required
- Logic should live in one layer only. Avoid duplicated responsibility across layers.

## Development principles

- Prefer familiar Simulink-style interaction, not feature parity.
- Prioritize PoC depth over feature breadth.
- Prefer a small number of reliable blocks over broad unsupported coverage.
- Do not add features that weaken determinism or blur architectural boundaries.
- The backend must reject invalid or ambiguous models before deployment.
- The firmware must execute only constrained, validated, versioned representations.
- Performance claims must be measurable.

## Key metrics

Whenever possible, expose and compare:

- control step time
- worst-case execution time
- jitter
- communication latency
- telemetry throughput
- simulation vs embedded output error

## Non-goals

This project is not trying to:

- replicate full Simulink coverage
- support arbitrary code execution on the device
- hide all embedded-system constraints
- guarantee real-time behavior on the desktop side

## Decision filter

Before adding a feature, ask:

1. Does this reduce the gap between simulation and embedded execution?
2. Does this preserve or improve determinism where it matters?
3. Does this respect frontend/backend/firmware boundaries?
4. Can it be measured or validated?
5. Is it necessary for the PoC?

If most answers are no, do not add it.

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:970c3bf2 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.

## Agent Context Profiles

The managed Beads block is task-tracking guidance, not permission to override repository, user, or orchestrator instructions.

- **Conservative (default)**: Use `bd` for task tracking. Do not run git commits, git pushes, or Dolt remote sync unless explicitly asked. At handoff, report changed files, validation, and suggested next commands.
- **Minimal**: Keep tool instruction files as pointers to `bd prime`; use the same conservative git policy unless active instructions say otherwise.
- **Team-maintainer**: Only when the repository explicitly opts in, agents may close beads, run quality gates, commit, and push as part of session close. A current "do not commit" or "do not push" instruction still wins.

## Session Completion

This protocol applies when ending a Beads implementation workflow. It is subordinate to explicit user, repository, and orchestrator instructions.

1. **File issues for remaining work** - Create beads for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Handle git/sync by active profile**:
   ```bash
   # Conservative/minimal/default: report status and proposed commands; wait for approval.
   git status

   # Team-maintainer opt-in only, unless current instructions forbid it:
   git pull --rebase
   bd dolt push
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**
- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.
<!-- END BEADS INTEGRATION -->

<!-- BEGIN BEADS CODEX SETUP: generated by bd setup codex -->
## Beads Issue Tracker

Use Beads (`bd`) for durable task tracking in repositories that include it. Use the `beads` skill at `.agents/skills/beads/SKILL.md` (project install) or `~/.agents/skills/beads/SKILL.md` (global install) for Beads workflow guidance, then use the `bd` CLI for issue operations.

### Quick Reference

```bash
bd ready                # Find available work
bd show <id>            # View issue details
bd update <id> --claim  # Claim work
bd close <id>           # Complete work
bd prime                # Refresh Beads context
```

### Rules

- Use `bd` for all task tracking; do not create markdown TODO lists.
- Run `bd prime` when Beads context is missing or stale. Codex 0.129.0+ can load Beads context automatically through native hooks; use `/hooks` to inspect or toggle them.
- Keep persistent project memory in Beads via `bd remember`; do not create ad hoc memory files.

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.
<!-- END BEADS CODEX SETUP -->

## Project continuity

The conversation is not the source of truth for project state. Git stores
implementation state, Beads stores operational work state, and project
documentation stores durable technical knowledge.

At the beginning of work, determine the logical workspace root, run `bd prime`,
inspect the single `in_progress` task and its notes, and inspect `git status` and
the relevant diff. Continue that task unless the user explicitly requests
something else.

During meaningful work, keep the active Bead current. Record important
discoveries, failed approaches worth avoiding, blockers, tests and results, and
the exact next step.

When the user says `checkpoint`, inspect current changes and update the active
Bead with what works, what fails, important tests and discoveries, unresolved
problems, and the exact next step. Leave unfinished work `in_progress`; close it
only when it is actually complete.

When the user says `continue`, run `bd prime`, inspect the current
`in_progress` task and relevant Git state, reconstruct the project state from
those durable sources, and continue from the recorded next step. If no task is
active, inspect `bd ready` and report available work rather than inventing one.

Never discard existing uncommitted work without inspecting it first.
