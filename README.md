# ctrl-lab

A proof of concept for closing the gap between control-system *simulation* and
*real-time embedded execution*: draw a Simulink-style block diagram, simulate it
on the PC, and (eventually) run the same validated controller on a
microcontroller and compare the traces.

It is deliberately **not** a Simulink replacement. See [`AGENTS.md`](AGENTS.md)
for the project philosophy, boundaries, and the decision filter used before
adding anything.

## Status (2026-09-06)

| Layer | State |
| --- | --- |
| **Frontend** (`frontend/`) | Working. React + React Flow canvas editor in a Tauri v2 desktop shell: block library, project save/open, scope plotting, compile report. |
| **Backend** (`backend/`) | Working. Rust crate `ctrl-backend`: project parsing, validation, and a deterministic fixed-step simulator, verified sample-by-sample against MATLAB references. |
| **Firmware** (`firmware/`) | **Design plus a bring-up probe running on real hardware.** `firmware/AGENTS.md` specifies the target runtime; `firmware/bringup/` runs on a **NUCLEO-F767ZI** and has settled the DTCM, console, cycle-counter and cache questions. There is no control runtime yet. |

Deployment to hardware does not exist yet. The backend→firmware wire format
(the "Deployable Control Plan") is implemented and can be compiled, inspected and
executed on the host in `f32` exactly as the firmware must — see
[`POC-PLAN.md`](POC-PLAN.md) for the staged route to running it on a board, and
[`PROJECT_STATUS.md`](PROJECT_STATUS.md) for where things stand right now.

## Layout

```
AGENTS.md            project philosophy and architectural boundaries
PROJECT_STATUS.md    START HERE - current state, loose ends, next actions
POC-PLAN.md          staged route to running a controller on real hardware
TODO.md              prioritized work queue
backend/             Rust: parse -> validate -> simulate -> compile a control plan
  src/plan.rs          the backend -> firmware wire format (DCP)
  src/exec.rs          f32 reference executor; the firmware's executable spec
  AGENTS.md            numerical contract, and the f32 bound
frontend/            Vite + React + React Flow editor, and the Tauri shell in src-tauri/
firmware/            design + a bring-up probe running on hardware; no runtime yet
  AGENTS.md            target runtime architecture
  BRINGUP.md           board facts, disk footprint, what to copy from other boards
  ZEPHYR-WORKSPACE.md  the Mac build environment and its local patches
  bringup/             minimal Zephyr app; runs on a NUCLEO-F767ZI
  scripts/             build (WSL), flash and console (Windows)
test-projects/       .json fixtures, .m MATLAB references, and the golden traces:
                       NN-*.ref.csv   MATLAB f64 reference
                       NN-*.plan.dcp  compiled plan, the exact bytes the MCU loads
                       NN-*.f32.csv   f32 trace the firmware kernels must reproduce
```

The Tauri shell depends on the backend crate directly
(`ctrl-backend = { path = "../../backend" }`) and exposes it to the UI as the
`simulate_project` and `compile_project_report` commands.

## Prerequisites

- **Rust** (stable, edition 2021) — rustup or Homebrew both work
- **Bun** (`packageManager` pins 1.2.9; newer works)
- **Tauri v2 platform prerequisites**:
  - **macOS** — Xcode Command Line Tools. Nothing else; the webview is the
    system WKWebView.
  - **Windows** — MSVC C++ Build Tools and WebView2.
  - **Linux / WSL2** — the package list is in
    [`frontend/README.md`](frontend/README.md).
- **MATLAB** — optional. Only needed to *regenerate* the golden references; the
  `.ref.csv` files are committed, so the test suite runs without it.

Verified building on **Windows 11 (x86_64)** and **macOS 26 (arm64)** from a
clean clone. The backend's f32 test vectors reproduce bit-for-bit on both, and
the frontend's built asset hashes match across platforms.

## Setup and run

From the repo root, on any platform:

```bash
bun run tauri:dev   # desktop app - the normal way to use it
bun run dev         # browser-only frontend, no backend commands available
```

Both run `setup` first (`bun install` in `frontend/`, then `cargo build` for the
backend), so a fresh checkout needs no separate install step. Note that plain
`bun install` at the repo root does nothing useful — the root `package.json`
only holds scripts. Use `bun run setup`.

The full script list is in the root `package.json`; `bun run verify` runs the
backend tests and the frontend build together.

## Backend on its own

```bash
# validate + simulate one project, printing execution order and final values
bun run backend:run
cargo run --manifest-path backend/Cargo.toml -- test-projects/01-double-integrator.json

# dump a full simulation trace as CSV
cargo run --manifest-path backend/Cargo.toml --example trace -- test-projects/04-2nd-order-system.json
```

## Validation

```bash
bun run verify        # backend tests + frontend build
bun run backend:test  # 52 unit + 6 vector/golden tests
bun run frontend:build
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

- [`POC-PLAN.md`](POC-PLAN.md) — staged plan for the first controller on real
  hardware, and the comparison chain used to attribute error
- [`AGENTS.md`](AGENTS.md) — purpose, subproject ownership, architectural rules
- [`backend/AGENTS.md`](backend/AGENTS.md) — the numerical contract (sampling,
  discretization, feedback, validation)
- [`firmware/AGENTS.md`](firmware/AGENTS.md) — target firmware runtime and the
  Deployable Control Plan container
- [`frontend/AGENTS.md`](frontend/AGENTS.md) — editor invariants and the manual
  regression checklist
- [`firmware/BRINGUP.md`](firmware/BRINGUP.md) — what the board gives you, what
  it does not, disk footprint, and how to build the probe
- [`firmware/ZEPHYR-WORKSPACE.md`](firmware/ZEPHYR-WORKSPACE.md) — the macOS
  build environment, its local Zephyr patches, and which of them reach us. The
  primary build environment is now WSL; see `firmware/BRINGUP.md`
