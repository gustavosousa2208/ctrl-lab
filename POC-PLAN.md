# PoC Plan — first controller on hardware

Goal: a PID designed in ctrl-lab, compiled to a Deployable Control Plan, executed
on an STM32, closing a loop around a second STM32 emulating the plant — with the
resulting trajectory compared against the simulator and the error *attributed*
rather than hand-waved.

This is the plan for `TODO.md`'s "Before Firmware RST" and "Hardware Validation"
sections. Read [`PROJECT_STATUS.md`](PROJECT_STATUS.md) first if you are cold.

## The method: one unknown at a time

Everything here follows from a single idea. There is a chain from the reference
math to the real hardware, and **each link differs from the previous one in
exactly one way**:

| # | Executor | Numerics | New variable introduced |
| --- | --- | --- | --- |
| A | MATLAB `.m` | f64 | — (reference) |
| B | `simulate_validated_dag` | f64 | ctrl-lab's engine |
| C | Host plan executor | **f32** | the DCP format + f32 |
| D | Firmware, 1 MCU, plan compiled in | f32 | the C kernels + scheduler |
| E | Firmware, 2 MCUs, digital link | f32 | inter-MCU transport + delay |
| F | Firmware, 2 MCUs, analog link | f32 | DAC/ADC, quantization, clock skew |

A↔B already passes (`backend/tests/golden.rs`, within `1e-6`). Every stage below
adds one link and compares it to the one before. When a number disagrees you know
which transition caused it, because only one thing changed.

Skipping links is the failure mode to avoid. Going straight to F means a
discrepancy could be the format, the kernels, the transport, the ADC, or clock
skew, and you will burn days guessing.

## Hardware

**2 × WeAct Studio MiniSTM32H743 Core Board** (STM32H743VIT6). Zephyr board id
`mini_stm32h743`, present in v4.3.0 and declaring 512 KB RAM / 2 MB flash.

- **Control rate: 1 kHz** (`base_ts_ns = 1_000_000`). At 480 MHz that is ~480,000
  cycles per tick against a kernel cost of a few dozen. The headroom is absurd,
  which is correct for this PoC — it proves *determinism and correctness*, not
  throughput.
- **The M7 has a double-precision FPU** (FPv5-D16), so `f64` runs in hardware
  here, not in software. An earlier draft of this plan claimed otherwise; that
  was true of the Cortex-M4F originally targeted, not of this part. `f32` in the
  DCP is still the right call — half the memory and bus traffic, and it keeps the
  format portable to parts without a `d`-variant FPU — but it is a footprint and
  portability choice, not a performance necessity. Stage C measured what it costs
  (below), which is what actually matters.
- **Caches are the real determinism hazard on this part**, and they are new
  relative to an M4. The M7 has separate I- and D-caches — both **on by default**,
  verified in a real build — so execution time becomes history-dependent and
  jitter stops being a simple function of the code path. Mitigations, both
  already verified: put the signal and state pools in DTCM (zero-wait, never
  cached), and measure the cost with caches on and off rather than guessing.

### Bring-up: verified, and less work than expected

`firmware/bringup/` is a probe that builds for this board and answers stage D's
prerequisites. Full detail in [`firmware/BRINGUP.md`](firmware/BRINGUP.md);
the short version:

- **There is a console.** USB CDC ACM over the board's USB-C port, working with
  no configuration from us. An earlier draft here claimed the board had no
  console and no UART — that was read off the `chosen` block without following
  the `#include` on line 161 of the board `.dts`.
- **DTCM already exists**: 128 KB at `0x20000000`, plus 64 KB of ITCM, inherited
  from `stm32h742.dtsi`. The board just never selects it. A four-line overlay
  choosing `zephyr,dtcm` is the whole fix, and the probe confirms both pools
  link at `0x20000000+`. An earlier draft claimed we would have to write the
  memory region ourselves; also wrong, same reason.
- Because the console rides the USB device stack, the workspace's
  `udc_stm32.c` resume-callback patch **is** in our build path — it was
  previously filed as only mattering if we used USB CDC.
- **No ADC, DAC or PWM enabled**, which stays true and lands on stage F.
  `nucleo_h743zi` is the reference to copy from, against the WeAct schematic.
- **No onboard debugger**, but SWD is wired up by hand, so J-Link works — and
  with it RTT, which is a better telemetry path than the USB console.

### Workspace split

The repo and backend live on Windows; Zephyr and the boards live on the Mac
(`remote-macos-gusta-mac`). The backend is pure Rust + serde, so it builds on
macOS too. Arrangement:

- Mac clones this repo from GitHub, builds `firmware/`, flashes both boards, and
  runs the host-side comparison tools locally against the attached hardware.
- Windows stays the editor/UI machine.
- Everything crossing between them goes through git, not file sync.

**Verified end to end on macOS** (2026-09-04, arm64) from a clean clone: all 57
backend tests pass, the frontend builds, and the Tauri shell compiles in ~50 s.
The f32 plan vectors were generated on Windows/x86_64 and reproduce bit-for-bit
on macOS/arm64 — a useful independent check on the determinism the whole project
depends on. Disk footprint is tabulated in
[`firmware/BRINGUP.md`](firmware/BRINGUP.md).

The Mac's Zephyr environment has been surveyed — see
[`firmware/ZEPHYR-WORKSPACE.md`](firmware/ZEPHYR-WORKSPACE.md). Zephyr v4.3.0,
SDK 0.17.4, `mini_stm32h743` supported. Two things to know before building there:
it is a **shared work workspace** carrying ten uncommitted patches in the Zephyr
tree (seven cannot affect an H7 build; three can, none blocking), and `west`
lives in a venv that is not on the non-interactive SSH `PATH`.

## Stage C — host plan executor — **DONE**

Landed. `backend/src/exec.rs` runs a `ControlPlan` the way the firmware must: a
flat `f32` signal pool, a flat `f32` state pool, blocks walked in `blocks[]`
order, one function per `KernelId`, no allocation in the step, no `f64` anywhere.

Shipped with it:

- `ctrl-backend --emit-plan <out.dcp>`, `--emit-trace <out.csv>`, `--dump-plan`.
- Committed vectors per fixture: `test-projects/NN-*.plan.dcp` (the exact bytes
  the firmware loads) and `NN-*.f32.csv` (the exact trace its kernels must
  reproduce), guarded by two tests in `backend/tests/golden.rs`.

### Result: the f32 bound

Worst divergence between the f32 executor and the f64 simulator, across all four
fixtures:

| Fixture | max &#124;f64 − f32&#124; | Worst signal |
| --- | --- | --- |
| 01-double-integrator | 5.34e-6 | `integrator-2` |
| 02-feedback-TF | 1.90e-7 | `gain-12` |
| 03-TF-test | 1.30e-7 | `sum-3` |
| 04-2nd-order-system | 5.76e-6 | `transferFunction-9` |

**Bound: 5.8e-6.** That closes `TODO.md`'s long-standing "numeric robustness
checks for f32 execution versus MATLAB double" item. Anything on hardware that
exceeds it is a bug, not precision loss.

### Result: the execution model is two passes, not one

The finding stage C existed to produce, and it contradicts `firmware/AGENTS.md`.
Each tick must run **all** block outputs, then **all** state updates:

```text
pass 1:  for block in blocks:  signals[out] = kernel_output(state, signals[in])
pass 2:  for block in blocks:  state        = kernel_update(state, signals[in])
```

A single fused walk is wrong. The topological sort orders only *direct-feedthrough*
edges, so a strictly-proper plant `P` is scheduled **before** the controller `C`
that drives it. `P` needs no input for its output, so the early slot is fine —
but its state update needs `u[k]`, produced later in the same tick. Fusing feeds
it `u[k-1]`, silently inserting a one-sample delay into the loop.

This is not theoretical: `exec.rs` pins it with a test that runs fixture 04 both
ways. The fused version diverges by **1.2e-2** — about 2000× the f32 noise floor,
so it is unmissable if the firmware gets it wrong, but only if someone is
looking. `firmware/AGENTS.md` describes a single walk of `blocks[]` and needs
correcting before the scheduler is written.

## Stage D — firmware runtime, one MCU, no I/O

Flash one board. Load a plan containing **both** the controller and the plant —
exactly what `04-2nd-order-system.json` already is. No second MCU, no ADC, no
transport.

Two simplifications that make this dramatically easier, and which you should
take:

1. **Compile the plan in as `static const uint8_t plan[]`.** No transport, no
   loader protocol. Stage E adds those. The point here is the scheduler and the
   kernels.
2. **Record to RAM, dump afterwards.** Run a fixed number of ticks into a static
   buffer, then print it over the USB CDC console (or RTT) when the run ends.
   Keep that buffer in main SRAM, not DTCM — it is written once per tick and read
   never, so it gains nothing from tightly-coupled memory, while the signal and
   state pools are touched repeatedly and do. No real-time telemetry
   constraints, no dropped samples, and the comparison is exact. Streaming
   telemetry is a separate problem — do not entangle it with correctness.

   Budget: the H743 declares 512 KB RAM, so this is comfortable. 5000 ticks
   × 6 signals × 4 B = 120 KB, a 5 s window at 1 kHz — plenty for a step
   response to settle once you re-discretize the plant at `Ts = 0.001` (re-run
   the fixture's `.m` with the new `Ts`; the scripts make it a one-line
   change).

What to build in `firmware/`:

- Freestanding Zephyr application, board `mini_stm32h743`, starting from
  `firmware/bringup/` (already builds; console and DTCM are settled).
- Plan loader: verify magic, `format_version`, `kernel_set_version`, CRC32, then
  size the signal and state pools statically from the header.
- Kernel dispatch table indexed by `kernel_id`, matching `plan.rs`'s enum
  one-for-one.
- Scheduler: hardware-timer tick, high-priority thread or ISR, walking `blocks[]`
  top to bottom. Every `rate_div` is 1 in v1.
- Cycle counter (DWT `CYCCNT`) around the control step for timing.

**Exit criteria:** the on-device trace matches the stage-C `NN-*.f32.csv`
**bit-for-bit**, or the divergence is explained. This is a genuinely achievable
bar — same operations, same order, same IEEE-754 single precision — and it is
worth insisting on, because "close enough" here hides kernel bugs that will
resurface as a mystery at stage F. Expect the first attempt to fail on operation
*ordering* inside the state-space update; that is the thing to look at first.

Also measure, and write down: control step time, min/max/jitter over the run.

## Stage E — two MCUs, digital link

MCU A runs the controller plan, MCU B runs the plant plan. Link them over SPI
with **A as master**.

### The trap that will cost you a week

**The link adds delay, and the simulator does not know about it.** If you compare
a two-MCU run against a simulation of the undelayed diagram, it will not match,
and the natural instinct is to blame the firmware. It is not the firmware.

Handle it by making the delay *exactly one sample and provably so*, then modelling
it explicitly:

- SPI is full-duplex. At tick `k`, A transmits `u[k]` and simultaneously receives
  the word B loaded during tick `k-1`, i.e. `y[k-1]`.
- B computes `y[k]` from the `u[k]` it just received and loads it for the next
  exchange.
- The loop therefore contains exactly **one sample of transport delay**, by
  construction, with no dependence on baud rate or interrupt latency.
- Put a `delay` block in the ctrl-lab diagram at that point. You already have the
  kernel. Now the simulation models the physical loop and the comparison is
  meaningful.

`TODO.md` already lists "one-sample delay mismatch" as a wanted test case. This
is that test case, arriving as physical reality rather than as a unit test.

### Splitting the diagram

Do **not** build graph partitioning yet. It is a large feature and it is not what
this PoC is testing. Instead, hand-author two projects — `controller.json` and
`plant.json` — connected by explicit I/O blocks.

This does require one new backend capability: **`Input` / `Output` blocks that
bind to named hardware channels**. That is exactly the `io_bind[]` array the DCP
format already reserves and which `build_control_plan` currently leaves empty.
Minimal version: an `Input` block (no inputs, one output, reads a named channel)
and an `Output` block (one input, no output, writes a named channel), with the
channel name resolved by the HAL at load time.

Automatic partitioning of one diagram into N plans is the right long-term answer.
It is not stage E.

**Exit criteria:** the two-MCU trace matches a stage-C simulation *of the
delay-augmented diagram*. Measure and record round-trip link latency.

## Stage F — analog interconnect

A's DAC → B's ADC, and B's DAC → A's ADC. Now the loop is physical.

New error sources, all of which you should expect and budget for:

- **Quantization.** 12-bit over the DAC's output span. Compute the LSB in
  engineering units and state it as the noise floor before you run anything.
- **DAC/ADC nonlinearity and offset.** Calibrate: sweep A's DAC, read B's ADC,
  fit the line, and record the residual.
- **Sampling skew.** The two boards' ticks are not aligned, and their clocks
  drift. This is the hardest error to attribute. Either share a sync line (one
  board's tick drives the other's timer) or measure the drift explicitly and
  report it. Do not leave it unquantified.

**Exit criteria:** the analog run is compared against the stage-E digital run,
and the difference is accounted for by the quantization + calibration + skew
budget above. The comparison that matters at this stage is E↔F, not C↔F.

## The PID itself

**You do not need a new kernel.** A discrete PID *is* a second-order discrete
transfer function, and your `TransferFunction` kernel already executes exactly
that. `04-2nd-order-system.json`'s controller is already this shape.

So: design the PID in MATLAB, `c2d` it at `Ts = 0.001`, and paste the
coefficients into a `transferFunction` block as `num`/`den`, highest power
first — the convention `backend/AGENTS.md` already locks down. The whole existing
golden-test path applies unchanged. That is the fastest route to a green light,
and it reuses machinery that is already verified against MATLAB.

### What that form gives up, and when it starts to matter

A raw transfer function has no output saturation, no anti-windup, no separate
state reset. Through stages C, D and E that costs nothing — the signals are pure
numbers and nothing clips.

At stage F it matters immediately, because the DAC has a hard output range.
Recommended fix, in this order:

1. Add a **`Saturation` kernel** (stateless, direct-feedthrough, two params) and
   the matching editor block. Trivial to implement and to test.
2. Put it in the *diagram*, so the simulator clamps identically. A clamp that
   exists only in firmware guarantees sim/hardware divergence the moment the
   controller saturates.
3. Only then consider a dedicated `Pid` kernel with back-calculation
   anti-windup — at which point the controller must *know* its output was
   clamped, which a plain TF-plus-clamp cannot express.

Step 3 is a real design decision, not a formality, and it is the point where
"PID as a transfer function" genuinely stops being sufficient. Defer it until
stage F shows you windup.

## Backend work, concretely

Ordered by when it blocks something:

1. `--emit-plan` / `--dump-plan` on `ctrl-backend`. *(blocks C)*
2. `backend/src/exec.rs`, the f32 reference executor. *(blocks C)*
3. Test-vector generation: `NN-*.plan.dcp` + `NN-*.f32.csv`. *(blocks D)*
4. Re-discretize the fixtures at `Ts = 0.001` and regenerate references.
   *(blocks D)*
5. `Input` / `Output` blocks and `io_bind[]` population. *(blocks E)*
6. `Saturation` kernel + editor block. *(blocks F)*
7. `wcet_estimate_ns`: populate from per-kernel cycle costs measured at stage D.
   Chicken-and-egg by nature — hardware measures it, backend then stamps it, and
   only after that does the loader's WCET rejection check mean anything.

## Metrics to report

The set `AGENTS.md` commits to, per stage:

- control step time (mean, max)
- WCET, measured vs. the backend's estimate
- jitter (tick-to-tick deviation)
- communication latency (stages E, F)
- telemetry throughput
- simulation vs. embedded output error: `max_abs_error` and RMS per signal

Plus, for the step response: settling time, overshoot, steady-state error, and
deadline misses.

## Open decisions

- **The plan currently packs the plant as a block.** Fine for stages C and D,
  where the plant is software. But `backend/AGENTS.md` is emphatic that the plant
  is not the firmware contract, and nothing in the DCP distinguishes "controller,
  deploy this" from "plant, simulate only". Stage E forces the question. The
  `Input`/`Output` blocks are most of the answer, but the format may want an
  explicit marker.
- **Telemetry transport** is unspecified. Stage D dodges it with record-then-dump.
  Stage E onward needs a real answer — USB CDC is the obvious one on these boards,
  and at 1 kHz × 6 signals it is only ~24 kB/s.
- **Plan transport** is likewise unspecified. Stage D compiles the plan in. The
  hot-swap path in `firmware/AGENTS.md` needs a real link eventually.
- **Clock synchronization** between the two boards (stage F). Shared tick line is
  the cheap answer; decide before wiring.
