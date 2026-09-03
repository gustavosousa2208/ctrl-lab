# ctrl-lab

A proof of concept for closing the gap between control-system *simulation* and
*real-time embedded execution*: draw a Simulink-style block diagram, simulate it
on the PC, and (eventually) run the same validated controller on a
microcontroller and compare the traces.

It is deliberately **not** a Simulink replacement. See [`AGENTS.md`](AGENTS.md)
for the project philosophy, boundaries, and the decision filter used before
adding anything.

## Status (2026-09-03)

| Layer | State |
| --- | --- |
| **Frontend** (`frontend/`) | Working. React + React Flow canvas editor in a Tauri v2 desktop shell: block library, project save/open, scope plotting, compile report. |
| **Backend** (`backend/`) | Working. Rust crate `ctrl-backend`: project parsing, validation, and a deterministic fixed-step simulator, verified sample-by-sample against MATLAB references. |
| **Firmware** (`firmware/`) | **Design only.** `firmware/AGENTS.md` specifies the target runtime; there is no Zephyr code and no board bring-up yet. |

Deployment to hardware does not exist yet. The backend→firmware wire format
(the "Deployable Control Plan") has a first implementation in
`backend/src/plan.rs`, but nothing calls it yet — see
[`PROJECT_STATUS.md`](PROJECT_STATUS.md).

## Layout

```
AGENTS.md            project philosophy and architectural boundaries
TODO.md              prioritized work queue
PROJECT_STATUS.md    recovered state, in-progress work, next actions
backend/             Rust: parse -> validate -> simulate (AGENTS.md = numerical contract)
frontend/            Vite + React + React Flow editor, and the Tauri shell in src-tauri/
firmware/            design notes only (AGENTS.md = target runtime architecture)
test-projects/       .json fixtures + .m MATLAB references + .ref.csv golden traces
```

The Tauri shell depends on the backend crate directly
(`ctrl-backend = { path = "../../backend" }`) and exposes it to the UI as the
`simulate_project` and `compile_project_report` commands.

## Prerequisites

- **Rust** (stable, edition 2021)
- **Bun** 1.2.9 (pinned via `packageManager` in `frontend/package.json`)
- **Tauri v2 platform prerequisites** — on Windows: MSVC C++ Build Tools and
  WebView2. On Linux/WSL2 the package list is in
  [`frontend/README.md`](frontend/README.md).
- **MATLAB** — optional. Only needed to *regenerate* the golden references; the
  `.ref.csv` files are committed, so the test suite runs without it.

## Setup and run

```bash
# desktop app (the normal way to use it)
cd frontend
bun install
bun run tauri dev

# browser-only frontend, no backend commands available
cd frontend && bun run dev
```

`Open.ps1` at the repo root is a two-line Windows helper for the first of these.

## Backend on its own

```bash
# validate + simulate one project, printing execution order and final values
cargo run --manifest-path backend/Cargo.toml -- test-projects/01-double-integrator.json

# dump a full simulation trace as CSV
cargo run --manifest-path backend/Cargo.toml --example trace -- test-projects/04-2nd-order-system.json
```

## Validation

```bash
cargo test --manifest-path backend/Cargo.toml   # 47 unit + 4 golden tests
cd frontend && bun run build                    # tsc -b && vite build
```

The golden tests replay each `test-projects/NN-*.json` through the backend and
compare every signal, sample by sample, against the MATLAB-generated
`NN-*.ref.csv` within `1e-6`. Regenerate a reference with:

```bash
matlab -batch "cd('test-projects'); eval(fileread('04-2nd-order-system.m'))"
```

The frontend has no automated test suite. `frontend/AGENTS.md` lists the manual
checks to run before finishing a frontend change.

## Further reading

- [`AGENTS.md`](AGENTS.md) — purpose, subproject ownership, architectural rules
- [`backend/AGENTS.md`](backend/AGENTS.md) — the numerical contract (sampling,
  discretization, feedback, validation)
- [`firmware/AGENTS.md`](firmware/AGENTS.md) — target firmware runtime and the
  Deployable Control Plan container
- [`frontend/AGENTS.md`](frontend/AGENTS.md) — editor invariants and the manual
  regression checklist
