# Backend — Simulation & Numerical Contract

This file is the numerical contract for the backend. It sits *under* the
project-wide philosophy in [`../AGENTS.md`](../AGENTS.md); read that first for
the frontend/backend/firmware boundaries and the decision filter. This file
only covers the math: how a validated model is turned into a deterministic,
firmware-faithful simulation.

The backend is Rust-first. Parsing, validation, simulation, and (later)
deployable-model generation all live here.

## What must match the firmware, and what must not

This distinction drives every numerical decision below.

- **The controller (discrete blocks) is the contract.** The difference equation
  the backend simulates must be the *same* one the firmware executes — same
  coefficients, same sample time, same update order. If simulation and device
  disagree on the controller, the product has failed at its one job.
- **The plant is not the contract.** On hardware the plant is physical. In
  pure PC simulation it is only a *preview* model. How we integrate a continuous
  plant (Euler, RK4, or discretize-to-`Ts`) is a **fidelity knob**, not part of
  the firmware contract. Choose it for plot quality and MATLAB agreement, not
  for device match.

## Time and sampling

- **Fixed step only.** No variable-step integration. The simulation must be
  deterministic and bit-reproducible across runs and machines.
- **v1: one clock.** A project has a single control rate `Ts`, and the
  simulation step `stepSize` *is* that `Ts`. Every block advances once per step.
  Lowering `stepSize` genuinely runs the controller faster — it is never a
  cosmetic "smoother plot" knob that silently changes the math.
- **General rule (reserved for multi-rate, not built in v1).** When blocks are
  allowed different periods, the simulation base step is the **greatest common
  divisor** of all block periods. A block with period `Tb` fires every
  `Tb / base` steps and holds its output (zero-order hold) between its own ticks.
  - Example: periods `7 ms` and `3 ms` → `base = gcd = 1 ms`; the 3 ms block
    fires on steps 3, 6, 9…, the 7 ms block on steps 7, 14, 21…, realigning at
    `LCM = 21 ms`. No tick is ever dropped or rounded onto the wrong step.
  - "Run at the smallest period" is only correct when every period is an integer
    multiple of the smallest (then `gcd == smallest`). Do not assume it in
    general.
- **Reject, do not fake.** If block periods are not rational multiples of each
  other, or the GCD collapses to an impractically small base (gross
  oversampling just to place ticks), reject the model at validation with a clear
  message. Ambiguous timing must never be silently approximated.

## Discrete transfer functions

- **Author convention:** MATLAB `tf`/`c2d` — z-domain, **highest power first**,
  as written in fixtures (`domain: discrete`, `discreteVariable: z`,
  e.g. `num "0 0.0178 0.0177"`, `den "1 -1.9696 0.9739"`). `z^-1` inputs are
  also accepted and mean the coefficients are already in ascending negative
  powers.
- **Internal canonical form:** one difference equation in `z⁻¹`.
  For `H(z) = B(z)/A(z)` with `A` of degree `n` (highest power first), divide
  both polynomials by `z^n`. Align by left-padding the shorter coefficient list
  with zeros, then normalize both by `a0`:

  ```
  y[k] = ( b0·u[k] + b1·u[k-1] + … )  −  ( a1·y[k-1] + a2·y[k-2] + … )   (all / a0)
  ```

- **Direct feedthrough** iff `b0 ≠ 0`, i.e. iff `deg B == deg A`. A strictly
  proper block (`b0 == 0`, e.g. the `"0 …"` numerators in fixture 04) does *not*
  feed through and is therefore legal inside a feedback loop.

## Continuous blocks (integrator, continuous transfer functions)

- **Discretized once, up front, with exact ZOH.** A continuous transfer function
  is realized in controllable canonical form and converted to a discrete state
  space at `Ts` via the augmented matrix exponential — the same result as MATLAB
  `c2d(sys, Ts, 'zoh')`. The simulation then advances
  `x[k+1] = Ad x[k] + Bd u[k]`, `y[k] = C x[k] + D u[k]` once per step.
  See `zoh_discretize` / `matrix_exponential` in `src/lib.rs`.
- Forward Euler was used before commit `fdf547e` and is gone. Because ZOH is
  exact at the sample instants for a piecewise-constant input, there is nothing
  left for RK4 to improve here; the golden suite matches MATLAB to `1e-6`.
- This is still a **fidelity** choice for the plant preview (see "plant is not
  the contract"), not part of the firmware contract. The benefit is that the
  preview runs the same difference equations a discretized reference would.

## Feedback and algebraic loops

- Execution order is a topological sort over **direct-feedthrough edges only**.
  Stateful and strictly-proper blocks break cycles because their output at step
  `k` depends only on past state, not on the current input.
- A cycle consisting entirely of direct-feedthrough blocks is a true algebraic
  loop and is **rejected**, not solved.

## Validation-first

The backend rejects invalid or ambiguous models *before* simulating: unknown
block types, unconnected required inputs, multiple edges into one input port,
non-rational multi-rate timing, unsupported transfer-function shapes, and
degenerate denominators. A model that reaches the simulator is already known to
be well-formed.

## f32 execution contract

The firmware executes `f32`; the simulator computes in `f64`. `backend/src/exec.rs`
is the reference `f32` executor and the executable specification the firmware
kernels are graded against — same operations, same order, same single precision.

Measured worst-case divergence between the two, over all four fixtures:
**5.8e-6** (`cargo test -p ctrl-backend exec -- --nocapture` prints the per-fixture
table). Treat that as the bound: a device trace that differs from
`test-projects/NN-*.f32.csv` by more than this is a bug, not precision loss.

`NN-*.plan.dcp` and `NN-*.f32.csv` are committed and regression-tested, so a
change to parameter packing, kernel arithmetic, or the wire format shows up as a
failing test rather than as a surprise on the bench.

## Testing contract

- Golden regression against MATLAB/Simulink-exported traces.
- Compare not only final output but `r`, `e`, `u`, and internal controller
  states **sample-by-sample**.
- Cover: step and ramp tracking, disturbance rejection, measurement noise,
  startup/reset, one-sample delay mismatch, near-stability-boundary poles, and
  sign-convention mistakes in the controller.

## Open / deferred (do not build in v1)

- **Deployable representation.** A first cut now exists in `src/plan.rs`
  (Deployable Control Plan, `DCP_FORMAT_VERSION = 1`): a pre-scheduled block
  list over a signal pool, packed f32 parameters, and a little-endian byte
  encoding, matching the container described in `../firmware/AGENTS.md`.
  Status: landed in `7e312b3` but **not wired to any caller** — no CLI flag or
  Tauri command emits a plan yet, and no firmware consumes one. Treat the layout
  as a proposal until a firmware loader has read it back. Transfer functions are
  packed as discrete state space (continuous ones ZOH-discretized first) rather
  than as raw `{b, a, Ts}`; that choice is not yet reflected here or in the RST
  discussion below.
- **Multi-rate.** Not in v1. The GCD base-rate rule above reserves the semantics
  so it can be added without changing the project format.
