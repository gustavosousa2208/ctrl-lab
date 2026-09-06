# Project Status

The handoff document. Refresh this file rather than starting a new one.

| | |
| --- | --- |
| Updated | 2026-09-06 |
| Branch | `main`, clean, pushed |
| HEAD | `d704bee` + this session's hardware run |
| Remote | `origin` = `git@gitea-local:gusta/ctrl-lab` — **changed**, see Loose ends |
| Tests | 59 passing (`bun run backend:test`) |
| Branches / stashes / tags | none besides `main` |
| Untracked | `Open.ps1` only — a personal 2-line launcher, superseded by `bun run tauri:dev` |

Evidence tags: **[V]** verified by running it, **[I]** inferred, **[?]** unknown.

## Start here

```bash
bun run setup     # frontend deps + backend build
bun run verify    # 59 backend tests + frontend build
bun run tauri:dev # the desktop app

# the firmware control core, graded against the reference without a board
bash firmware/ctrl/host/build.sh
./firmware/ctrl/host/ctrl-host test-projects/04-2nd-order-system.plan.dcp 501 \
  | python3 firmware/scripts/grade-trace.py test-projects/04-2nd-order-system.f32.csv
```

Works identically on Windows and macOS. **[V]** If you are cold, read
[`AGENTS.md`](AGENTS.md) for what the project is for, then
[`POC-PLAN.md`](POC-PLAN.md) for where it is going.

## What the project is

Draw a control-system block diagram, simulate it deterministically on the PC,
then run the *same validated controller* on a microcontroller and compare the
traces. Not a Simulink replacement — a focused platform for measuring the gap
between simulation and embedded execution.

Maturity: **the whole chain works, end to end.** A diagram simulates on the PC,
compiles to a plan, and that plan runs on a NUCLEO-F767ZI producing a trace
identical to the simulator's f32 reference — bit for bit, on all four fixtures.
What is missing is not correctness but a control *loop*: no hardware timer, no
I/O, no transport. **[V]**

## Layer status

| Layer | State |
| --- | --- |
| **Frontend** `frontend/` | Working. React Flow canvas in a Tauri v2 shell: block library, project save/open, scope plotting, compile report. No automated tests — see the manual checklist in `frontend/AGENTS.md`. |
| **Backend** `backend/` | Working. Parse → validate → simulate (f64), plus `plan.rs` (compile to a Deployable Control Plan) and `exec.rs` (f32 reference executor). Verified sample-by-sample against MATLAB. |
| **Firmware** `firmware/` | `firmware/ctrl/` — the plan loader, two-pass scheduler and all ten kernels — **runs on the NUCLEO-F767ZI and is bit-exact against the reference on all four fixtures.** A step costs 13-19 us. See [`firmware/ctrl/README.md`](firmware/ctrl/README.md). |

## The three environments

- **Windows** (`C:\Users\gusta\source\ctrl-lab`) — the repo, editor and UI work,
  **and the board**: the Nucleo's ST-Link enumerates here (`COM3`), and
  STM32CubeProgrammer is installed.
- **WSL / Ubuntu 24.04** on the same machine — **where the firmware is built**.
  Zephyr v4.3.0 at `3568e1b6d5c`, SDK 0.17.4, in `~/zephyrproject`.
- **macOS** (`remote-macos-gusta-mac`, Tailscale `100.70.245.53`) — **a
  complete environment: build, flash, run and grade.** The board was moved here
  on 2026-09-06 and everything in the loop works natively. **[V]**
  - `build.sh` detects the platform and sources `mac-env.sh` (Zephyr workspace
    at the same commit as WSL, SDK at `~/zephyr-sdk-0.17.4`; `dtc` and `gperf`
    from Homebrew, since the macOS SDK ships no host tools).
  - `flash.sh` drives west's stm32cubeprogrammer runner. Needs **STM32CubeCLT**,
    which installs to `/opt/ST/STM32CubeCLT_*/` and is *not* on the default
    `PATH`; the script finds it.
  - `console.py` resets and captures. See Loose ends on why it must set
    `clocal -crtscts`.
  - Working checkout is `~/source/ctrl-lab`; `~/ctrl-lab` is a stale one, see
    Loose ends.

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
| D | C kernels + scheduler, 1 MCU | **done** — bit-exact on hardware, all four fixtures. No timer/IO yet |
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

### The stage D result, and the trap it found

`firmware/ctrl/` is the control core: plan loader, two-pass scheduler, all ten
kernels. It **reproduces `backend/src/exec.rs` bit-for-bit on all four
fixtures** — not to a tolerance, to the bit. **[V]**

That was established *without the board*, because the same sources also build
natively (`firmware/ctrl/host/`). The board is now needed only to confirm that a
different FPU agrees, which is a much smaller question than "is the runtime
right".

Bit-for-bit needed a new instrument. The committed `NN-*.f32.csv` files hold
nine decimal places, and nine decimals do not always round-trip an f32, so they
can only ever support a *tolerance* claim. `exec::trace_digest` hashes the raw
sample bits instead; `ctrl-backend --trace-hash` prints it, both harnesses
print it, and `firmware_trace_digests_are_pinned` pins all four in the test
suite.

**The trap: `-ffp-contract=off` is load-bearing.** GCC and Clang default to
fusing `acc += a * b` into an FMA, whose un-rounded intermediate is *more*
accurate than the Rust reference — and therefore wrong, since the contract is
bit-for-bit. Measured on the host harness: contraction changes the digest on
**three of the four fixtures**, and its worst error, **4.1e-6, is still inside
the 5.8e-6 noise floor**. A tolerance-based grading passes it silently. Both
builds now set the flag, and both say why. **[V]**

This is the same failure shape as the fused-passes bug and the order-3 transfer
function: a wrong answer that looks close enough. It is the third one this
project has caught by measuring instead of assuming.

### Then it ran on the board, and the digests still matched

All four fixtures, flashed and read from macOS on 2026-09-06. Every one returns
the digest the reference executor computes. The f32 arithmetic on a Cortex-M7
FPU is identical to Rust's on arm64 and x86_64. **[V]**

| fixture | steps | verdict | min | mean | max | ISR outliers |
| --- | --- | --- | --- | --- | --- | --- |
| `01-double-integrator` | 101 | bit-for-bit | 3108 | 3110 | 3341 | 0 / 100 |
| `02-feedback-TF` | 101 | bit-for-bit | 2736 | 2739 | 3080 | 0 / 100 |
| `03-TF-test` | 101 | bit-for-bit | 3824 | 3835 | 4161 | 0 / 100 |
| `04-2nd-order-system` | 501 | bit-for-bit | 3989 | 3997 | **7782** | 1 / 500 |

Cycles at 216 MHz: a control step costs **13-19 us**.

Three results came out of the run, each closing something that was open:

1. **The big outlier is an interrupt, proven not assumed.** Fixture 04's max is
   double its mean, reproducible to the cycle at step 373 across resets.
   Building with `-DCTRL_IRQ_LOCK=y` takes it from 1 outlier in 500 to **0**,
   and max from 7882 to 3992. So one ISR lands inside one step and costs ~3930
   cycles. The worst *uninterrupted* step is 3992 cycles — at a 1 kHz tick,
   1.9% of the period, with an ISR intrusion adding another 1.8%. **[V]**
   Unexplained: every step is also ~300 cycles faster under `irq_lock`, which
   moves the *minimum* and so is not an interrupt. Recorded, not guessed at.
2. **The cache A/B is closed.** Caches on versus off is **bit-identical and
   cycle-identical** — 3989/3997/7782 either way. Unlike the probe's run, this
   one has ~68 KB of trace buffer and the plan structures in ordinary cacheable
   SRAM, so the result now covers the case the probe could not speak to. **[V]**
3. **`wcet_estimate_ns` finally has a number to stamp**: 3992 cycles, 18.5 us.
   It is per-board and per-plan, so what the backend still needs is a policy
   rather than a constant.

### Then it ran on a clock

The loop is timer-driven now, and two measurement traps cost real time getting
there — both recorded in [`firmware/ctrl/README.md`](firmware/ctrl/README.md).
**DWT CYCCNT stops while the core sleeps in WFI**, so it measured a 50 ms period
as ~11 900 cycles (that is the *awake* time, not the period); tick timing now
uses `k_cycle_get_32()`. And **`k_timer` rounds a requested period up to a whole
kernel tick**, so an earlier sweep of 80/60/50/40 us measured the same 100 us
four times while reporting >100% CPU load, because load was divided by the
requested period rather than the delivered one. Both are fixed in the firmware.

The result that matters most is what happens when it *cannot* keep up: at a
20 us period the loop missed 3002 deadlines and delivered 140 us instead, and
the trace was **still bit-for-bit identical**. The scheduler drops ticks, not
steps — so overload silently changes the sampled-data model while the arithmetic
stays perfect. That is the failure this project exists to catch, and it is why
the deadline counter is not decoration. **[V]**

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
| Build the probe | `bash firmware/scripts/build.sh bringup nucleo_f767zi -p always` | **[V]** WSL and macOS |
| Build the runtime | `bash firmware/scripts/build.sh ctrl nucleo_f767zi -p always` | **[V]** warning-free, 30 KB flash / 75 KB RAM / 384 B DTCM |
| Flash it (macOS) | `bash firmware/scripts/flash.sh ctrl` | **[V]** |
| Read the console (macOS) | `python3 firmware/scripts/console.py --out run.txt` | resets, then reads **[V]** |
| Timer rate sweep | `VARIANT=f40 EXTRA_CONF=fast-tick.conf bash firmware/scripts/build.sh ctrl nucleo_f767zi -p always -- -DCTRL_TICK_NS=40000` | **[V]** 5 deadlines missed of 501 |
| Free-running (fast) | `bash firmware/scripts/build.sh ctrl nucleo_f767zi -p always -- -DCTRL_FREE_RUN=y` | **[V]** same digest, <1 s |
| Grade a device run | `python3 firmware/scripts/grade-trace.py test-projects/04-2nd-order-system.f32.csv run.txt --expect-digest 0xfddb22c1a9525b2c` | **[V]** PASS - bit-for-bit |
| Host harness, hex text | `./firmware/ctrl/host/ctrl-host --text <plan.dcp> <steps>` | **[V]** |
| Host harness | `bash firmware/ctrl/host/build.sh` | **[V]** |
| Grade a trace | `./firmware/ctrl/host/ctrl-host <plan.dcp> <steps> \| python3 firmware/scripts/grade-trace.py <ref.f32.csv>` | **[V]** PASS on all four |
| Bit-exact digest | `cargo run --manifest-path backend/Cargo.toml -- --trace-hash test-projects/04-2nd-order-system.json` | **[V]** matches the C core |
| Flash it (Windows) | `firmware\scripts\flash.ps1` | **[V]** |
| Read the console (Windows) | `firmware\scripts\console.ps1` | resets, then reads **[V]** |
| Desktop app | `bun run tauri:dev` | compiles on both platforms; **not run in a GUI session** **[?]** |

## Loose ends

- ~~**The board's console drops bytes.**~~ **Fixed by raising the console to
  921600 baud**, which is now the default in `firmware/ctrl` and in both console
  scripts. Zero rows lost or damaged across nine captures at 460800 and 921600,
  against 0-2 lost and 0-3 damaged per run at the board's default 115200. The
  intuition that a faster line would overrun a buffer harder was wrong: the loss
  is *time-in-flight*, so a shorter stream is a cleaner one. 921600 is also the
  ceiling — macOS rejects every rate above it on this driver. **[V]**
  - The bringup probe still runs at 115200; read it with `--baud 115200`.
  - Two earlier attempts failed and are recorded in `firmware/ctrl/README.md`:
    a firmware-side `k_msleep(1)` per row (did nothing, reverted) and host
    `stty clocal -crtscts` (helped materially, kept).
  - **The trace is also a binary frame now** (`DCPT`, layout in
    `firmware/ctrl/src/trace.h`), 2.26x smaller than the hex rows it replaces:
    15 086 bytes for fixture 04 against 34 078. Self-delimiting and CRC-checked,
    so it carries unchanged to USB CDC and to the stage E MCU-to-MCU link.
    `-DCTRL_TRACE_TEXT=y` restores hex rows; the grader reads either. **[V]**
  - **The loss figures published earlier today were partly my own measurement
    bug.** `console.py` did not drain the receive buffer before resetting, so it
    was reading a stale trace left over from the flash — which reported captures
    faster than the line rate and sometimes *more* rows than the run has steps.
    `console.ps1` had always done this right with `DiscardInBuffer()`. Fixed;
    all numbers above are from clean captures. Two fixes were tried: device-side pacing (`k_msleep(1)` per row) did
  nothing and was reverted; **`stty clocal -crtscts` helped substantially**,
  because macOS defaults the port to hardware flow control the VCP does not
  drive. `console.py` sets it. The loss is reduced, not eliminated, so
  `grade-trace.py` matches rows by time rather than position and skips damaged
  ones. **[V]**
- **There are two checkouts on the Mac, and they disagree.** Work happens in
  `~/source/ctrl-lab` (this one, `origin` = `git@gitea-local:gusta/ctrl-lab`).
  `~/ctrl-lab` is an older clone at `7585ca9`, clean, pointing at
  `git@github.com:gustavosousa2208/ctrl-lab.git` — the remote every document
  before this one names. **Which remote is authoritative is an open question for
  the author**; both were reachable when checked. The stale checkout is also
  what is serving the Vite process below. Delete it or re-point it, but do not
  leave two clones with different remotes. **[V]**
- **A Vite dev server may still be running on the Mac** — started 2026-09-04,
  pid 84712, bound to `100.70.245.53:5173` (tailnet only, not the LAN). Still
  running as of 2026-09-06, out of `~/ctrl-lab/frontend` — the stale checkout
  above, so it is serving four-commit-old code. `kill 84712`. Harmless if left;
  it will not survive a reboot. **[V]**
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
- **The DCP is a draft, but it is no longer unread.** An independent decoder now
  exists — `firmware/ctrl/src/dcp.c` — and it round-trips every committed plan,
  so the format has been exercised by something other than its own encoder.
  Rejection paths are exercised too: corrupt body, truncation, bad magic, and
  both version gates. **[V]** What is still open is the two empty fields:
  `io_bindings` is always empty (the loader *refuses* a non-empty one rather
  than pretending to bind) and `wcet_estimate_ns` is hardcoded to `0`, which
  makes the loader's WCET rejection check vacuous. The check is written; the
  backend has nothing to stamp yet.
- **The two planning documents disagreed about what these block, and POC-PLAN
  was right.** `PROJECT_STATUS.md` called them "what is left of the DCP draft
  before a firmware kernel can be written"; `POC-PLAN.md` (lines 311-315) has
  `io_bind[]` blocking **stage E**, not D, and `wcet_estimate_ns` as
  chicken-and-egg — hardware measures it, the backend then stamps it. Stage D
  was written without either, which settles it in POC-PLAN's favour. The
  ordering below reflects that.
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

Ordered. Stage D is closed and the board is on the Mac, so nothing here is
blocked on hardware access or on a machine switch.

1. ~~Drive the step from a hardware timer.~~ **Done and measured.** The step
   runs on the plan's own `base_ts_ns` in a cooperative thread woken by the
   timer ISR. On fixture 04's 50 ms tick: **78 ns of jitter peak-to-peak, 0.11%
   CPU, 0 deadlines missed of 501**, trace still bit-for-bit. The design tops
   out near **16-20 kHz**, limited by ~8300 cycles of scheduling overhead per
   tick rather than by the 18.9 us step. Full tables in
   [`firmware/ctrl/README.md`](firmware/ctrl/README.md). **[V]**
2. **Decide what the backend stamps into `wcet_estimate_ns`.** The number now
   exists but it is per-board and per-plan, so this needs a policy — probably a
   measured per-kernel cost table summed over a plan's blocks, with a margin.
   Until then the loader's WCET rejection is written but vacuous.
3. **Consider running the step in the timer ISR.** Of 12 343 awake cycles per
   tick only 4081 are the step; the rest is timer ISR, semaphore and two context
   switches. Reclaiming it needs care — FP in an ISR is only safe with
   `CONFIG_FPU_SHARING`, which is already on — and it is what would move the
   ceiling above 20 kHz. Not needed at 1 kHz, where load is 5.7%.
4. **Stage E**: inter-MCU transport and `io_bindings`. The plan is currently
   linked into the firmware; receiving one over a wire is the next unknown, and
   `io_bind[]` is what a source/sink block needs in order to reach a pin.
5. **Run the `frontend/AGENTS.md` manual checklist** against `bfaa9c6`. Still
   the only committed change in the project with no verification behind it, and
   the Mac has a GUI.
6. Loose ends worth an hour: the duplicate `~/ctrl-lab` checkout and its stale
   Vite server, and the `origin` remote question. Then the `TODO.md` backlog.

## History

Four sessions, 2026-09-03 to 2026-09-06, from a project untouched since
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

**Stage D, on the Mac** (this session). Moved the work to macOS and found the
Mac could do more than the documents assumed: it carries the Zephyr SDK and a
workspace at the same commit as WSL, so `build.sh` now detects the platform and
both firmware applications build here. Only flashing still needs Windows.

Then wrote the control core — `firmware/ctrl/`: plan loader, two-pass scheduler,
all ten kernels — 1478 lines of C, 911 of them non-comment. It builds
warning-free for the board, but the result that mattered came from building it a
second way. The same sources
compile natively under `firmware/ctrl/host/`, which turned "stage D is blocked
on the board" into "stage D is blocked on a flash": the runtime was verified
bit-for-bit against `exec.rs` on all four fixtures without hardware.

Proving *bit*-for-bit needed a new instrument, because the committed CSVs hold
nine decimals and nine decimals do not always round-trip an f32. Hashing the raw
sample bits does — `exec::trace_digest`, `--trace-hash`, and four digests pinned
in the test suite.

The instrument immediately earned itself. `-ffp-contract=off` turns out to be
load-bearing: with GCC's default contraction the C core produces different bits
on three of the four fixtures, while its worst error stays *inside* the 5.8e-6
noise floor. A tolerance check passes it. That is the third time this project
has found a wrong answer hiding under a plausible one, after the fused passes
and the order-3 transfer function.

Two documentation conflicts were resolved by the work rather than by argument.
`PROJECT_STATUS.md` claimed `io_bindings` and `wcet_estimate_ns` blocked stage D
while `POC-PLAN.md` put `io_bind[]` at stage E; stage D was written without
either, so POC-PLAN was right. And `firmware/AGENTS.md` still opened with "no
Zephyr code exists yet", which stopped being true in this session.

**Stage D on hardware** (same session, after the board moved to the Mac). The
Mac turned out to close the whole loop: STM32CubeCLT was already installed, so
`flash.sh` and `console.py` joined the Windows `.ps1` pair and build, flash, run
and grade all happen on one machine now.

All four fixtures ran and every digest matched. The claim the project has been
building toward for four sessions — that a diagram simulated on a PC produces
*the same numbers* on a microcontroller — is now measured rather than intended,
and it holds to the bit across three architectures.

The run also answered three open questions. The one large timing outlier turned
out to be an interrupt, proven by an `irq_lock` build that took it from 1 in 500
to 0. The cache A/B, open since bring-up because the probe only ever exercised
DTCM, came back bit-identical *and* cycle-identical with 68 KB of cacheable
working set. And `wcet_estimate_ns` finally has a number behind it.

One wrong diagnosis is worth recording. The console drops bytes, and the first
guess was the ST-Link's buffer, so the firmware got a 1 ms pause per row. It
changed nothing and was reverted. The actual cause was on the host: macOS
defaults the port to hardware flow control that the VCP does not drive. Reading
`stty -a` first would have been quicker than editing firmware — the same lesson
`BRINGUP.md` already draws about reading the generated `.config`.
