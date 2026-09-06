# TODO

## Done (this pass)

- Backend: investigated the "drift" around `test-projects/04-2nd-order-system.json` — the discrete feedback engine was already correct (matches an independent reference and the analytic closed-loop DC gain 0.3917). The stale finite-only test hid it; it is now a reference-value golden.
- Backend: replaced forward Euler for continuous blocks with exact ZOH discretization at `Ts` (controllable-canonical state space + augmented matrix exponential), matching MATLAB `c2d(..., 'zoh')`. See `backend/AGENTS.md` for the locked numerical contract.
- Backend: transfer-function handling verified for continuous, discrete `z`, and discrete `z^-1`; deterministic and covered by unit + golden tests.
- Validation: canonical conventions (highest-power-first, single-clock `Ts`, GCD base-rate rule, strictly-proper feedback) documented in `backend/AGENTS.md`.
- Backend: golden regression against MATLAB references landed — every `test-projects/NN-*.json` has a matching `NN-*.m` producing `NN-*.ref.csv`, compared sample-by-sample by `backend/tests/golden.rs` (r/e/u/y and switch/gain signals included).
- Tooling: the generated `*.ref.csv` references are committed, so the golden suite runs without MATLAB installed. `golden.rs` still skips (rather than fails) a case whose CSV is missing.

## Now

The runtime is written and correct off-target, so the board is the blocker
again. Current state and context: [`PROJECT_STATUS.md`](PROJECT_STATUS.md).

- ~~Backend/Firmware: reconcile `firmware/AGENTS.md` with what `plan.rs` actually encodes.~~ **Done.** The packed discrete state space wins over the biquad SOS cascade, **capped at order 2**. The cap was measured, not chosen: at order 3 a clustered-pole filter in f32 is already ~100× past the 5.8e-6 noise floor, and by order 6 it diverges outright. Both parsers now reject order > 2 — previously the discrete path had no check at all while the continuous path capped at 2, so a z-domain order 8 ran and produced nonsense. If order > 2 is ever needed, add the SOS cascade as a **new `KernelId`**; do not raise the constant. See `backend/AGENTS.md`, "Transfer function order limit".
- ~~Backend: settle `io_bindings` and `wcet_estimate_ns` before a firmware kernel can be written.~~ **The premise was wrong.** The kernels were written without either, and `POC-PLAN.md` had it right all along: `io_bind[]` blocks **stage E**, and `wcet_estimate_ns` is chicken-and-egg — hardware measures it, the backend then stamps it. Both are still open, just not here:
  - `io_bindings` is always empty. `firmware/ctrl/src/dcp.c` *refuses* a non-empty one rather than pretending to bind channels it has no HAL for. Stage E.
  - `wcet_estimate_ns` is hardcoded to 0, so the loader's WCET rejection check is written but vacuous. Stamp it from the `step_ns max` the device prints.
- Frontend: run the `frontend/AGENTS.md` manual checklist against the transfer-function `domain` / `discreteVariable` inspector fields (`bfaa9c6`) — committed on a clean build but never exercised in the running app. The only committed change in the project with no verification behind it.
- ~~Hardware: pick the board.~~ **Done** — NUCLEO-F767ZI, running the probe. No overlay was needed; the board already chooses DTCM. See [`firmware/BRINGUP.md`](firmware/BRINGUP.md).
- Firmware: **flash `ctrl` and grade the device trace.** Everything else is done — the runtime is bit-exact off-target, builds warning-free, and `flash.ps1 -App ctrl` needs no edit. The bar is the digest (`0xfddb22c1a9525b2c` for fixture 04), not the tolerance. Needs the Windows machine.
- Firmware: re-run the cache A/B — `caches-off.conf` now exists for `ctrl`, whose ~68 KB trace buffer is the first cacheable working set in the project. The probe's "caches cost nothing" result only ever covered DTCM. The digest must be identical either way; only timing may move.
- Firmware: drive the step from a hardware timer. It currently runs steps back to back, which is right for stage D's question but not a control loop.
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
- **Stage D is written and verified off-target.** `firmware/ctrl/` is the plan
  loader, the two-pass scheduler and all ten kernels; it reproduces `exec.rs`
  **bit-for-bit** on all four fixtures and builds warning-free for the board.
  What remains is a flash and a device trace. The same sources also build
  natively (`firmware/ctrl/host/`), which is how the numbers were verified
  without hardware. See [`firmware/ctrl/README.md`](firmware/ctrl/README.md).
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
