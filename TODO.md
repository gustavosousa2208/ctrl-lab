# TODO

## Done (this pass)

- Backend: investigated the "drift" around `test-projects/04-2nd-order-system.json` — the discrete feedback engine was already correct (matches an independent reference and the analytic closed-loop DC gain 0.3917). The stale finite-only test hid it; it is now a reference-value golden.
- Backend: replaced forward Euler for continuous blocks with exact ZOH discretization at `Ts` (controllable-canonical state space + augmented matrix exponential), matching MATLAB `c2d(..., 'zoh')`. See `backend/AGENTS.md` for the locked numerical contract.
- Backend: transfer-function handling verified for continuous, discrete `z`, and discrete `z^-1`; deterministic and covered by unit + golden tests.
- Validation: canonical conventions (highest-power-first, single-clock `Ts`, GCD base-rate rule, strictly-proper feedback) documented in `backend/AGENTS.md`.
- Backend: golden regression against MATLAB references landed — every `test-projects/NN-*.json` has a matching `NN-*.m` producing `NN-*.ref.csv`, compared sample-by-sample by `backend/tests/golden.rs` (r/e/u/y and switch/gain signals included).
- Tooling: the generated `*.ref.csv` references are committed, so the golden suite runs without MATLAB installed. `golden.rs` still skips (rather than fails) a case whose CSV is missing.

## Now

Neither of the first two needs hardware. Current state and context:
[`PROJECT_STATUS.md`](PROJECT_STATUS.md).

- ~~Backend/Firmware: reconcile `firmware/AGENTS.md` with what `plan.rs` actually encodes.~~ **Done.** The packed discrete state space wins over the biquad SOS cascade, **capped at order 2**. The cap was measured, not chosen: at order 3 a clustered-pole filter in f32 is already ~100× past the 5.8e-6 noise floor, and by order 6 it diverges outright. Both parsers now reject order > 2 — previously the discrete path had no check at all while the continuous path capped at 2, so a z-domain order 8 ran and produced nonsense. If order > 2 is ever needed, add the SOS cascade as a **new `KernelId`**; do not raise the constant. See `backend/AGENTS.md`, "Transfer function order limit".
- Backend: settle `io_bindings` (always empty) and `wcet_estimate_ns` (hardcoded to 0, which makes the loader's designed WCET rejection check vacuous). **This is what is left of the DCP draft before a firmware kernel can be written.**
- Frontend: run the `frontend/AGENTS.md` manual checklist against the transfer-function `domain` / `discreteVariable` inspector fields (`bfaa9c6`) — committed on a clean build but never exercised in the running app. The only committed change in the project with no verification behind it.
- ~~Hardware: pick the board.~~ **Done** — NUCLEO-F767ZI, running the probe. No overlay was needed; the board already chooses DTCM. See [`firmware/BRINGUP.md`](firmware/BRINGUP.md).
- Firmware: re-run the cache A/B once the runtime has a working set in `sram0`. The probe's "caches cost nothing" result is real but only covers DTCM-resident data, so it does not yet say anything about D-cache.
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
- **Stage D is in progress.** The board is up; the runtime is not written. Before
  the scheduler is written, note that the tick is **two passes** (all outputs,
  then all state updates) — see `firmware/AGENTS.md`.
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
