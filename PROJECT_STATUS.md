# Project Status

The handoff document. Refresh this file rather than starting a new one.

| | |
| --- | --- |
| Updated | 2026-09-04 |
| Branch | `main`, clean, pushed |
| HEAD | `7585ca9` |
| Remote | `origin` = `git@github.com:gustavosousa2208/ctrl-lab.git` |
| Tests | 57 passing (`bun run backend:test`) |
| Branches / stashes / tags | none besides `main` |
| Untracked | `Open.ps1` only — a personal 2-line launcher, superseded by `bun run tauri:dev` |

Evidence tags: **[V]** verified by running it, **[I]** inferred, **[?]** unknown.

## Start here

```bash
bun run setup     # frontend deps + backend build
bun run verify    # 57 backend tests + frontend build
bun run tauri:dev # the desktop app
```

Works identically on Windows and macOS. **[V]** If you are cold, read
[`AGENTS.md`](AGENTS.md) for what the project is for, then
[`POC-PLAN.md`](POC-PLAN.md) for where it is going.

## What the project is

Draw a control-system block diagram, simulate it deterministically on the PC,
then run the *same validated controller* on a microcontroller and compare the
traces. Not a Simulink replacement — a focused platform for measuring the gap
between simulation and embedded execution.

Maturity: **editor and simulator work and are verified against MATLAB. The
firmware does not exist yet** — only a design and a build-verified bring-up
probe.

## Layer status

| Layer | State |
| --- | --- |
| **Frontend** `frontend/` | Working. React Flow canvas in a Tauri v2 shell: block library, project save/open, scope plotting, compile report. No automated tests — see the manual checklist in `frontend/AGENTS.md`. |
| **Backend** `backend/` | Working. Parse → validate → simulate (f64), plus `plan.rs` (compile to a Deployable Control Plan) and `exec.rs` (f32 reference executor). Verified sample-by-sample against MATLAB. |
| **Firmware** `firmware/` | Design + a build-verified bring-up probe. **No control runtime yet.** |

## The two environments

- **Windows** (`C:\Users\gusta\source\ctrl-lab`) — the repo, editor and UI work.
- **macOS** (`remote-macos-gusta-mac`, Tailscale `100.70.245.53`) — Zephyr, the
  SDK, and the boards. Checkout at `~/ctrl-lab`.

Everything crosses between them through git, not file sync. The whole stack
builds on both, verified from a clean clone. **[V]**

Details that cost time to rediscover are in
[`firmware/ZEPHYR-WORKSPACE.md`](firmware/ZEPHYR-WORKSPACE.md) — chiefly that
`west` lives in a venv off the SSH `PATH`, and that zsh aborts an entire command
when any glob fails to match.

## Where the PoC stands

The plan is a chain where each stage adds exactly one unknown, so a bad number is
attributable to one transition. Full detail in [`POC-PLAN.md`](POC-PLAN.md).

| Stage | What it adds | State |
| --- | --- | --- |
| A → B | MATLAB → ctrl-lab engine, both f64 | **done**, within 1e-6 |
| C | the DCP format + f32 | **done**, bound 5.8e-6 |
| D | C kernels + scheduler, 1 MCU | **next** — needs a board |
| E | inter-MCU transport + delay | not started |
| F | DAC/ADC, quantization, clock skew | not started |

### Two results from stage C that shape everything after it

1. **The f32 bound is 5.8e-6.** Worst divergence between the f32 executor and the
   f64 simulator across all four fixtures. A device trace that differs from
   `test-projects/NN-*.f32.csv` by more than this is a bug, not precision loss.
   **[V]**
2. **The control step is two passes, not one** — all block outputs, then all
   state updates. The topological sort orders only direct-feedthrough edges, so a
   strictly-proper plant runs *before* the controller driving it; fusing the
   passes feeds it `u[k-1]` and silently inserts a sample of delay. Pinned by a
   test: the fused variant diverges by 1.2e-2, ~2000× the noise floor. **[V]**
   `firmware/AGENTS.md` has been corrected accordingly.

## Hardware

Currently targeted at the **WeAct MiniSTM32H743** (`mini_stm32h743`), and
`firmware/bringup/` builds for it. **[V]**

**This is likely to change.** A Nucleo with an integrated debugger was being
fetched as of 2026-09-04. If it is a **NUCLEO-H743ZI**, switch to it: same SoC so
every stage-C artifact carries over untouched, plus onboard ST-Link, a real UART
console on `usart3`, and `adc1`/`adc3`/`dac1`/PWM already enabled in its
devicetree — which is most of stage F's overlay work already done. A different
Nucleo family needs the FPU and cache assumptions re-checked. **[I]**

What bring-up established for the WeAct board, in
[`firmware/BRINGUP.md`](firmware/BRINGUP.md) — mostly still relevant whichever
H743 board wins:

- Console works with no configuration (USB CDC ACM on the WeAct; `usart3` on the
  Nucleo).
- **DTCM already exists**: 128 KB at `0x20000000`, inherited from
  `stm32h742.dtsi`. A four-line overlay choosing `zephyr,dtcm` places the signal
  and state pools there. Verified by `nm`. **Fails silently without the
  overlay** — the data falls back to SRAM and the build still passes, so check
  the linker's DTCM line.
- **Caches are on by default** and are the determinism hazard on Cortex-M7. The
  caches-off variant is verified to build, so the A/B jitter measurement is a
  two-line `prj.conf` change.

## Verified commands

| Purpose | Command | Result |
| --- | --- | --- |
| Everything | `bun run verify` | 57 tests + frontend build **[V]** |
| Backend tests | `bun run backend:test` | 51 unit + 6 vector/golden **[V]** |
| Compile a plan | `cargo run --manifest-path backend/Cargo.toml -- --emit-plan out.dcp test-projects/04-2nd-order-system.json` | 355 bytes **[V]** |
| Inspect a plan | `… -- --dump-plan out.dcp` | **[V]** |
| f32 reference trace | `… -- --emit-trace out.csv <project.json>` | **[V]** |
| Firmware probe | see [`firmware/BRINGUP.md`](firmware/BRINGUP.md) | builds **[V]** |
| Desktop app | `bun run tauri:dev` | compiles on both platforms; **not run in a GUI session** **[?]** |

## Loose ends

- **A Vite dev server may still be running on the Mac** — started 2026-09-04,
  pid 84712, bound to `100.70.245.53:5173` (tailnet only, not the LAN). Stop it
  with `ssh remote-macos-gusta-mac 'kill 84712'`. Harmless if left; it will not
  survive a reboot.
- **The editor's discrete-transfer-function fields were never exercised in the
  running app.** Committed in `bfaa9c6` on a clean build, but the
  `frontend/AGENTS.md` manual checklist needs a human at the app. **[?]** This is
  the only committed change in the project with no verification behind it.
- **`Open.ps1` is untracked.** Personal launcher, redundant with
  `bun run tauri:dev`. Commit it or delete it; nothing depends on it.
- **`frontend/PLAN.md`** is a scratch checklist with all items done. Superseded
  by `TODO.md`, kept because it documents a frontend working convention.
- **The Zephyr workspace on the Mac is shared work infrastructure** (Atletec
  EPTS) carrying 10 uncommitted patches in its `zephyr` tree. A snapshot is saved
  outside the tree at `~/zephyrproject/.local-patches/`. **Do not run
  `west update` there** for ctrl-lab reasons.

## Open decisions

- **Which board.** See Hardware above. Blocks stage D.
- **The DCP is a draft, not a frozen wire format.** Nothing has decoded a plan
  except its own round-trip test. `io_bindings` is always empty and
  `wcet_estimate_ns` is hardcoded to `0`, which makes the loader's designed WCET
  rejection check vacuous.
- **`firmware/AGENTS.md` and `plan.rs` disagree on the transfer-function
  kernel.** The doc prescribes a biquad second-order-section cascade for f32
  robustness; `plan.rs` packs a single discrete state space. Reconcile before
  writing the kernel.
- **RST controller form is undecided.** Long-standing `TODO.md` item. Note that
  stage C established a PID needs no new kernel — a discrete PID *is* a
  second-order discrete transfer function, which the existing kernel runs.
- **`compile_project_report` requires a dev checkout**: it shells out to
  `cargo build` and runs `backend/target/debug/ctrl-backend`. It cannot ship in a
  packaged app. **[V]**

## Next actions

Ordered. The first two need no hardware.

1. **Reconcile `firmware/AGENTS.md` with `plan.rs`** — state space or biquad SOS,
   and settle `io_bindings` and `wcet_estimate_ns`. This is the last thing
   blocking a firmware kernel from being written.
2. **Run the `frontend/AGENTS.md` manual checklist** against `bfaa9c6` in the
   desktop app.
3. **Pick the board**, then add its overlay and re-verify `firmware/bringup/`.
4. **Stage D**: flash the probe, read the cache/jitter numbers, then write the
   plan loader, kernel dispatch table and two-pass scheduler. Grade against
   `test-projects/NN-*.f32.csv`; the bar is bit-for-bit.
5. Later, from `TODO.md`: frontend `graphIndex` consistency checks, golden
   coverage of internal controller states, and the step/ramp/disturbance/noise
   cases.

## History

Two sessions, 2026-09-03 and 2026-09-04, from a project untouched since
2026-07-24.

**Recovery** (`37a9345`, `b449669`). The project was found with uncommitted work
and no root README. Committed the in-progress work as `7e312b3` (the Deployable
Control Plan) and `bfaa9c6` (editor domain fields), both byte-for-byte as found.
Added a root README, corrected stale claims in `backend/AGENTS.md` (it still
described forward Euler, replaced by ZOH in `fdf547e`), and untracked four
generated files — one of which, `frontend/vite.config.js`, was a live hazard
because Vite resolves `.js` before `.ts`. Salvaged root workspace scripts from
`origin/rst-impl` and deleted that branch, whose backend work had been superseded
by `fdf547e`; its tip was `e6d0da6`.

**Planning and survey** (`012fede`, `ada15e4`). Wrote `POC-PLAN.md` and surveyed
the Mac's Zephyr workspace.

**Stage C** (`acf417d`, `2701b54`). Built the f32 reference executor and the plan
CLI, producing the two results above. Retargeted the plan when the board turned
out to be an H743 rather than a G474 — which also corrected a claim that f64 is
software-emulated; true of the Cortex-M4F first assumed, false of the M7.

**Bring-up and portability** (`05a4299`, `077a6b4`, `3d8d8e2`, `7585ca9`).
Verified a Zephyr build for the board, normalized line endings, and fixed
`bun run frontend:build` failing on a fresh clone. Confirmed the whole stack
builds on macOS — and that the f32 vectors generated on Windows/x86_64 reproduce
bit-for-bit on macOS/arm64, an independent check on the determinism the project
depends on.
