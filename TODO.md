# TODO

## Done (this pass)

- Backend: investigated the "drift" around `test-projects/04-2nd-order-system.json` — the discrete feedback engine was already correct (matches an independent reference and the analytic closed-loop DC gain 0.3917). The stale finite-only test hid it; it is now a reference-value golden.
- Backend: replaced forward Euler for continuous blocks with exact ZOH discretization at `Ts` (controllable-canonical state space + augmented matrix exponential), matching MATLAB `c2d(..., 'zoh')`. See `backend/AGENTS.md` for the locked numerical contract.
- Backend: transfer-function handling verified for continuous, discrete `z`, and discrete `z^-1`; deterministic and covered by unit + golden tests.
- Validation: canonical conventions (highest-power-first, single-clock `Ts`, GCD base-rate rule, strictly-proper feedback) documented in `backend/AGENTS.md`.
- Backend: golden regression against MATLAB references landed — every `test-projects/NN-*.json` has a matching `NN-*.m` producing `NN-*.ref.csv`, compared sample-by-sample by `backend/tests/golden.rs` (r/e/u/y and switch/gain signals included).
- Tooling: the generated `*.ref.csv` references are committed, so the golden suite runs without MATLAB installed. `golden.rs` still skips (rather than fails) a case whose CSV is missing.

## Now

Stage D is closed: the runtime runs on the board and is bit-exact. Nothing below
is blocked on hardware access. Current state and context:
[`PROJECT_STATUS.md`](PROJECT_STATUS.md).

- ~~Backend/Firmware: reconcile `firmware/AGENTS.md` with what `plan.rs` actually encodes.~~ **Done.** The packed discrete state space wins over the biquad SOS cascade, **capped at order 2**. The cap was measured, not chosen: at order 3 a clustered-pole filter in f32 is already ~100× past the 5.8e-6 noise floor, and by order 6 it diverges outright. Both parsers now reject order > 2 — previously the discrete path had no check at all while the continuous path capped at 2, so a z-domain order 8 ran and produced nonsense. If order > 2 is ever needed, add the SOS cascade as a **new `KernelId`**; do not raise the constant. See `backend/AGENTS.md`, "Transfer function order limit".
- ~~Backend: settle `io_bindings` and `wcet_estimate_ns` before a firmware kernel can be written.~~ **The premise was wrong.** The kernels were written without either, and `POC-PLAN.md` had it right all along: `io_bind[]` blocks **stage E**, and `wcet_estimate_ns` is chicken-and-egg — hardware measures it, the backend then stamps it. Both are still open, just not here:
  - `io_bindings` is always empty. `firmware/ctrl/src/dcp.c` *refuses* a non-empty one rather than pretending to bind channels it has no HAL for. Stage E.
  - `wcet_estimate_ns` is hardcoded to 0, so the loader's WCET rejection check is written but vacuous. Stamp it from the `step_ns max` the device prints.
- Frontend: run the `frontend/AGENTS.md` manual checklist against the transfer-function `domain` / `discreteVariable` inspector fields (`bfaa9c6`) — committed on a clean build but never exercised in the running app. The only committed change in the project with no verification behind it.
- ~~Hardware: pick the board.~~ **Done** — NUCLEO-F767ZI, running the probe. No overlay was needed; the board already chooses DTCM. See [`firmware/BRINGUP.md`](firmware/BRINGUP.md).
- ~~Firmware: flash `ctrl` and grade the device trace.~~ **Done, on hardware.** All four fixtures return the reference digest bit-for-bit on the NUCLEO-F767ZI. A step costs 13-19 us.
- ~~Firmware: re-run the cache A/B.~~ **Done.** Caches on vs. off is bit-identical *and* cycle-identical (3989/3997/7782 either way) with ~68 KB of cacheable working set. Closes the item carried since bring-up.
- **Firmware: drive the step from a hardware timer.** The last structural piece of stage D. Budget is measured: 3992 cycles (18.5 us) worst-case uninterrupted, plus ~3930 for an ISR intrusion — under 4% of a 1 kHz period.
- Backend: decide what to stamp into `wcet_estimate_ns`. The number exists now, but it is per-board and per-plan, so this needs a policy (per-kernel cost table summed over a plan, plus margin), not a constant.
- Validation: document the exact RST equation form the project will use. Note that stage C established a PID needs no new kernel — a discrete PID *is* a second-order discrete transfer function, which the existing kernel already runs.

## Next

- Backend: extend golden coverage to internal controller states sample-by-sample (currently r/e/u/y and block I/O).
- Backend: add dedicated tests for:
  - step tracking
  - ramp tracking
  - disturbance rejection
  - measurement noise injection
  - startup and reset behavior
  - one-sample delay mismatch
  - near-stability-boundary pole placements
- Backend: add explicit tests for sign convention mistakes in `R`, `S`, and `T`.
- Frontend contract hardening: `graphIndex` is currently trusted by the backend to save parse/validation time. Add frontend-side consistency checks so `graphIndex` cannot drift from serialized `nodes` and `edges`.

## PoC: first controller on hardware

Staged plan in [`POC-PLAN.md`](POC-PLAN.md) — PID on one STM32
(NUCLEO-F767ZI), plant emulated on a second, compared against the simulator.

- **Stage C is done.** `backend/src/exec.rs` is the f32 reference executor;
  `--emit-plan` / `--emit-trace` / `--dump-plan` are on the CLI; committed
  vectors and their regression tests are in place. Measured f32-vs-f64 bound:
  **5.8e-6**.
- **Stage D is done, on hardware.** `firmware/ctrl/` is the plan loader, the
  two-pass scheduler and all ten kernels. All four fixtures run on the
  NUCLEO-F767ZI and return the reference digest **bit-for-bit**; a control step
  costs 13-19 us. The same sources also build natively
  (`firmware/ctrl/host/`), which is how a numerical bug gets caught before a
  flash. See [`firmware/ctrl/README.md`](firmware/ctrl/README.md).
- **The one timing outlier is an interrupt**, proven with an `irq_lock` build:
  1 step in 500 costs ~3930 extra cycles. Worst uninterrupted step is 3992.
- **Caches cost nothing**, now measured with a cacheable working set rather than
  only DTCM.
- ~~**The console drops bytes.**~~ **Fixed**: the console runs at 921600 now,
  and nine captures at 460800/921600 lost nothing at all. The loss was
  time-in-flight, not rate — faster is cleaner. `grade-trace.py` still matches
  by time rather than position and still lets the digest decide, which is worth
  keeping for the MCU-to-MCU link in stage E.
- **`-ffp-contract=off` is load-bearing.** Compiler FMA contraction changes the
  trace digest on three of four fixtures while staying *inside* the 5.8e-6
  tolerance — a wrong answer a tolerance check passes. Both firmware builds set
  the flag.
- **Bring-up is finished, on hardware.** `firmware/bringup/` runs on
  `nucleo_f767zi`: the pools are in DTCM (verified at runtime, no overlay
  required), the console is `usart3` on the ST-Link VCP, the DWT cycle counter
  runs at 216 MHz, and `fpu_dp=1`. Reference measurement: 63 dependent f32 MACs
  in 1653–1670 cycles, spread 17. The caches-off A/B is bit-identical. Build,
  flash and console scripts are in `firmware/scripts/`. See
  `firmware/BRINGUP.md`.
- One anomaly is open and documented: the cycle counter reported itself dead on
  the first flash and has never repeated it. Three causes were tested and
  rejected. **A stage-D timing of `0` is that bug, not a fast control step.**

## Before Firmware RST

- Backend: define a deployable, versioned controller contract for RST blocks.
- Backend: generate the exact runtime representation the firmware will execute, without frontend-specific semantics.
- Firmware: define the controller runtime API for coefficients, state reset, step execution, and telemetry.
- Firmware: decide saturation and safety behavior:
  - output limits
  - reset policy
  - invalid input handling
  - watchdog/failsafe behavior
- Firmware: define timing requirements and measurement points for:
  - control step time
  - WCET
  - jitter
  - telemetry latency

## Hardware Validation

- HIL: run the same test vectors through MATLAB/Simulink, backend, and firmware, then compare traces.
- HIL: verify cold start, warm reset, missed deadline, stale sample, and sensor fault behavior.
- HIL: verify plant-model mismatch robustness before claiming controller portability.
- Metrics: report `max_abs_error`, RMS error, settling time, overshoot, and deadline misses for each validation case.

## Nice To Have

- Frontend: surface transfer-function domain and variable more clearly in the node summary and inspector.
- Frontend: warn when a discrete transfer function is created without an explicitly stated sample-time assumption.
- Tooling: add an import/export path for MATLAB comparison datasets.
