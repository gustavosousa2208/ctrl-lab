# Project Status

The handoff document. Refresh this file rather than starting a new one.

| | |
| --- | --- |
| Updated | 2026-09-06 |
| Branch | `main`, clean, **not pushed** — four commits ahead of `origin` |
| HEAD | `481450f` |
| Remote | `origin` = `git@github.com:gustavosousa2208/ctrl-lab.git` |
| Tests | 58 passing (`bun run backend:test`) |
| Branches / stashes / tags | none besides `main` |
| Untracked | `Open.ps1` only — a personal 2-line launcher, superseded by `bun run tauri:dev` |

Evidence tags: **[V]** verified by running it, **[I]** inferred, **[?]** unknown.

## Start here

```bash
bun run setup     # frontend deps + backend build
bun run verify    # 58 backend tests + frontend build
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
control runtime does not exist yet** — there is a design, and a bring-up probe
that now runs on real hardware and has answered what it was written to ask.

## Layer status

| Layer | State |
| --- | --- |
| **Frontend** `frontend/` | Working. React Flow canvas in a Tauri v2 shell: block library, project save/open, scope plotting, compile report. No automated tests — see the manual checklist in `frontend/AGENTS.md`. |
| **Backend** `backend/` | Working. Parse → validate → simulate (f64), plus `plan.rs` (compile to a Deployable Control Plan) and `exec.rs` (f32 reference executor). Verified sample-by-sample against MATLAB. |
| **Firmware** `firmware/` | Design + a bring-up probe **running on a NUCLEO-F767ZI**. **No control runtime yet.** |

## The three environments

- **Windows** (`C:\Users\gusta\source\ctrl-lab`) — the repo, editor and UI work,
  **and the board**: the Nucleo's ST-Link enumerates here (`COM3`), and
  STM32CubeProgrammer is installed.
- **WSL / Ubuntu 24.04** on the same machine — **where the firmware is built**.
  Zephyr v4.3.0 at `3568e1b6d5c`, SDK 0.17.4, in `~/zephyrproject`.
- **macOS** (`remote-macos-gusta-mac`, Tailscale `100.70.245.53`) — a second
  Zephyr workspace at the same version. Not needed for firmware any more.
  Checkout at `~/ctrl-lab`.

Everything crosses between the machines through git, not file sync. The whole
stack builds on Windows and macOS, verified from a clean clone. **[V]**

**Windows and WSL do not need git between them.** The firmware source stays on
the Windows drive and WSL builds it in place over `/mnt/c`; the build tree goes
on the WSL native filesystem, where it is much faster. Flashing then reaches
back the other way — Windows reads the hex out of WSL over `\\wsl.localhost`, so
**no `usbipd` USB forwarding is involved**. `firmware/scripts/` implements all
of this; see [`firmware/BRINGUP.md`](firmware/BRINGUP.md).

Details that cost time to rediscover are in
[`firmware/ZEPHYR-WORKSPACE.md`](firmware/ZEPHYR-WORKSPACE.md) — chiefly that
`west` lives in a venv off the SSH `PATH`, and that zsh aborts an entire command
when any glob fails to match. Two more, for WSL: **`cmake` and `ninja` are in
the workspace venv**, and **`dtc` is inside the SDK's host tools**. Neither is
on the default `PATH`.

Both Zephyr workspaces belong to other projects — the Mac's to Atletec EPTS, the
WSL one to an imxrt1176-evkb bring-up. **Do not run `west update` in either.**

## Where the PoC stands

The plan is a chain where each stage adds exactly one unknown, so a bad number is
attributable to one transition. Full detail in [`POC-PLAN.md`](POC-PLAN.md).

| Stage | What it adds | State |
| --- | --- | --- |
| A → B | MATLAB → ctrl-lab engine, both f64 | **done**, within 1e-6 |
| C | the DCP format + f32 | **done**, bound 5.8e-6 |
| D | C kernels + scheduler, 1 MCU | **in progress** — board is up, runtime not written |
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

**Settled: the NUCLEO-F767ZI** (`nucleo_f767zi`), on the desk and running.
STM32F767ZI, Cortex-M7 at 216 MHz, 2 MB flash, 384 KB SRAM + 128 KB DTCM. The
WeAct MiniSTM32H743 is retired but its notes are kept. **[V]**

The switch cost almost nothing — the probe built for the new board unmodified
except for comments — and the Nucleo is the better target on every axis that
matters here. Full detail in [`firmware/BRINGUP.md`](firmware/BRINGUP.md).

Established **on hardware**, not by building:

- **The pools are in DTCM**, at `0x20000000` and `0x20000100`, and **no overlay
  is needed** — `nucleo_f767zi.dts` already has `zephyr,dtcm = &dtcm` in its
  chosen block. The probe now checks this at runtime against the devicetree and
  prints a per-pool verdict, so the old silent fallback to SRAM cannot pass
  unnoticed. **[V]**
- **The DWT cycle counter runs** at the full 216 MHz, so a control step can be
  timed. **[V]**
- **f64 is hardware**, `fpu_dp=1`. Same as the H743. **[V]**
- **The console is `usart3` on the ST-Link VCP** — a real UART, so no USB stack
  on the control path and nothing lost before enumeration. **[V]**
- **`adc1` (PA0), `dac1` (PA4) and `tim1_ch3` (PE13) are already enabled**, which
  is most of stage F's devicetree work. **[V]**

The reference measurement, reproducible to the cycle across resets: 63 dependent
f32 MACs run in **1653–1670 cycles, spread 17 (~1%)**. At 1 kHz the budget is
216 000 cycles.

**The caches turned out to cost nothing** — the `ICACHE=n`/`DCACHE=n` variant is
bit-identical. That contradicts the H743 analysis, which called caches the main
determinism hazard, and the reason is structural: the hot pools are in DTCM
(never cached), and `CONFIG_STM32_FLASH_PREFETCH=y` means the F7's ART
accelerator covers instruction fetch independently of L1. **This result does not
transfer to the control runtime** — the probe never touches cacheable memory, so
it says nothing about D-cache once the trace buffer lives in `sram0`. Re-run the
A/B then. **[V]** for the measurement, **[I]** for the explanation.

## Verified commands

| Purpose | Command | Result |
| --- | --- | --- |
| Everything | `bun run verify` | 58 tests + frontend build **[V]** |
| Backend tests | `bun run backend:test` | 52 unit + 6 vector/golden **[V]** |
| Compile a plan | `cargo run --manifest-path backend/Cargo.toml -- --emit-plan out.dcp test-projects/04-2nd-order-system.json` | 355 bytes **[V]** |
| Inspect a plan | `… -- --dump-plan out.dcp` | **[V]** |
| f32 reference trace | `… -- --emit-trace out.csv <project.json>` | **[V]** |
| Build firmware (WSL) | `bash firmware/scripts/build.sh bringup nucleo_f767zi -p always` | **[V]** |
| Flash it (Windows) | `firmware\scripts\flash.ps1` | **[V]** |
| Read the console (Windows) | `firmware\scripts\console.ps1` | resets, then reads **[V]** |
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
- **The WSL Zephyr workspace is someone else's too** — an imxrt1176-evkb
  bring-up, per its `.west/config` manifest. Same rule: **no `west update`.** It
  carries two uncommitted patches, both checked and harmless to us; the
  `arch/arm/core/cortex_m/prep_c.c` one is in every Cortex-M build path but is
  entirely inside `#ifdef CONFIG_MCUBOOT`. **[V]**
- **The cycle counter reported itself dead once, on the very first flash, and
  has never done it again.** Three hypotheses were tested on hardware and
  rejected; all are written up in `firmware/BRINGUP.md` so the time is not spent
  twice. The detection is now robust, but if a stage-D timing ever reads `0`,
  that is this, and it is a broken measurement rather than a fast step. **[?]**

## Open decisions

- ~~Which board.~~ **Settled** — NUCLEO-F767ZI, running. See Hardware above.
- **The DCP is a draft, not a frozen wire format.** Nothing has decoded a plan
  except its own round-trip test. `io_bindings` is always empty and
  `wcet_estimate_ns` is hardcoded to `0`, which makes the loader's designed WCET
  rejection check vacuous.
- ~~`firmware/AGENTS.md` and `plan.rs` disagree on the transfer-function
  kernel.~~ **Settled** — the packed discrete state space wins, **capped at
  order 2**, and the docs now say so. The cap is measured, not chosen: at order
  3 a clustered-pole filter in f32 is already 100× past the 5.8e-6 noise floor,
  and by order 6 it diverges outright. The SOS cascade is the documented path
  when order > 2 is genuinely needed — as a *new* `KernelId`, never by raising
  the constant. See `backend/AGENTS.md`, "Transfer function order limit". **[V]**
- **RST controller form is undecided.** Long-standing `TODO.md` item. Note that
  stage C established a PID needs no new kernel — a discrete PID *is* a
  second-order discrete transfer function, which the existing kernel runs.
- **`compile_project_report` requires a dev checkout**: it shells out to
  `cargo build` and runs `backend/target/debug/ctrl-backend`. It cannot ship in a
  packaged app. **[V]**

## Next actions

Ordered. Bring-up is finished, so the board no longer blocks anything — the
blocker is now a decision, not hardware.

1. **Settle `io_bindings` and `wcet_estimate_ns`.** The kernel-form question is
   now closed; these two are what is left of the DCP draft. `io_bindings` is
   always empty and `wcet_estimate_ns` is hardcoded to `0`, which makes the
   loader's designed WCET rejection check vacuous. No hardware needed.
2. **Stage D proper**: write the plan loader, the kernel dispatch table and the
   two-pass scheduler. Grade the device trace against
   `test-projects/NN-*.f32.csv`; the bar is bit-for-bit, and the f32 noise floor
   is 5.8e-6, so anything above that is a bug rather than precision loss.
   Remember the tick is **two passes**, not one — fusing them cost 1.2e-2 on
   fixture 04.
3. **Re-run the cache A/B once the runtime touches `sram0`.** The probe's
   "caches are free" result is real but only covers a DTCM-resident working set.
4. **Run the `frontend/AGENTS.md` manual checklist** against `bfaa9c6` in the
   desktop app. Still the only committed change with no verification behind it.
5. Later, from `TODO.md`: frontend `graphIndex` consistency checks, golden
   coverage of internal controller states, and the step/ramp/disturbance/noise
   cases.

## History

Three sessions, 2026-09-03 to 2026-09-06, from a project untouched since
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

**First hardware** (`1fd6afb`, `c99f191`). The board arrived and was a
NUCLEO-F767ZI, so the PoC moved off the WeAct H743 — cheaply, since the probe
built for it unmodified except for comments, and the Nucleo needs no DTCM
overlay at all. Built it in WSL, which already had a Zephyr workspace at the
same commit and SDK as the Mac, and flashed from Windows by reading the hex out
of WSL over a UNC path rather than forwarding USB with `usbipd`.

Then actually ran it, which is the part that mattered. DTCM placement went from
a linker claim to a runtime check; the cycle counter came up at 216 MHz; and the
cache A/B — expected, on the H743 analysis, to be the headline determinism
hazard — came back bit-identical, because the hot pools are in DTCM and the F7's
ART accelerator makes L1 redundant for a tight loop.

Two corrections came out of it, both of the same kind. The commit before had
claimed f64 was soft-emulated on the F7, reading `stm32f7x/Kconfig` against
`stm32h7x/Kconfig`; the symbol is actually supplied per die through an `rsource`
glob, and the board prints `fpu_dp=1`. And a first-flash report of a dead cycle
counter was attributed to a Cortex-M7 LAR unlock, which hardware then showed had
never been locked. Three hypotheses about that anomaly were tested and rejected;
it has not reproduced, and it is documented as unexplained rather than tidied
away. The lesson written into `BRINGUP.md` covers all three misreads across the
project: **read the generated `zephyr/.config`, not the Kconfig sources.**
