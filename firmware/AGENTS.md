# Firmware — Control Runtime Architecture

Design draft (pre-Zephyr). This is the companion to the project philosophy in
[`../AGENTS.md`](../AGENTS.md) and the numerical contract in
[`../backend/AGENTS.md`](../backend/AGENTS.md). It defines *how* a validated
controller executes deterministically on the microcontroller. No Zephyr code
exists yet; this is the target we build against.

## The model: a stable runtime executing data (Fork A)

The firmware is a **fixed, flashed-once runtime**. A specific controller is
**data** — a compact, versioned *Deployable Control Plan* (DCP) the backend emits
and ships over the wire. Changing a gain, a coefficient, or the whole diagram
changes the plan, **not** the firmware. No recompile, no reflash for routine
model changes.

Why this and not per-model code generation:

- The product exists to make iteration fast and to hide the toolchain from users
  who will never touch it. Codegen + reflash reintroduces the exact friction we
  remove.
- The math still runs in compiled, optimized **kernels**; only orchestration is
  data-driven, and at control rates that overhead is negligible.
- "Constrained, validated, versioned representation" (root `AGENTS.md`) *is* a
  data-driven runtime.

The engine is a **data-driven block scheduler**, not a hand-written control loop.
The difference equation does not disappear — it becomes one kernel among many.
Scaling capability means adding kernels to the library, never rewriting the
engine. (If free-form math is ever needed, contain a tiny bytecode inside one
"expression" kernel rather than turning the whole engine into a VM.)

## Layers

1. **Platform / HAL** — owns the hardware: clocks, ADC, PWM, encoders/timers,
   network stack, storage, telemetry transport. Dominates peripherals so the user
   configures nothing. Exposes **named I/O channels** (e.g. `adc0`, `pwm2`,
   `encoder1`), never vendor specifics, to the layer above.
2. **Control core** — plan loader, static memory pools, deterministic scheduler,
   and the kernel library. Driven by a hardware-timer tick (high-priority Zephyr
   thread / ISR-triggered work) with a fixed budget.
3. **I/O binding** — the plan's source/sink blocks resolve to HAL channels *by
   name* at load time. This is what decouples a student's diagram from the board.
4. **Lifecycle / safety FSM** — plan loading, validation, arming, running, fault
   handling, hot-swap.
5. **Telemetry** — lower priority, double-buffered, sampled inside the control
   step, drained off the real-time path. Never allocates or blocks the step.

Boundaries (from root `AGENTS.md`): the control core knows nothing about vendor
peripherals; only the HAL does. The backend never emits vendor specifics.

## Deployable Control Plan (DCP) — the backend→firmware contract

The backend already produces a validated DAG with a topological order and
per-block behavior. The DCP is the serialized *result* of that work, so the MCU
never runs graph algorithms at runtime. Proposed structure:

```
header    : magic, format_version, plan_id (hash), kernel_set_version,
            base_ts_ns, n_blocks, n_signals,
            signal_pool_bytes, state_pool_bytes, wcet_estimate_ns, crc32
signals   : f32[n_signals]                        // the wires; ZOH-persistent
blocks[]  : { kernel_id:u16, rate_div:u16,        // in execution order
              param_off:u32, param_len:u16,
              in_count:u8, in_signal_idx[in_count],
              out_signal_idx, state_off:u32, state_len:u16 }
params    : blob                                   // packed little-endian f32
io_bind[] : { block_idx, channel_role, channel_index }   // source/sink -> HAL
meta      : model name, generated_at, backend version   // non-executable
```

Rules:

- **Pre-scheduled.** `blocks[]` is already in the backend's topological order.
  The runtime executes it top to bottom; it does not sort or resolve feedback.
- **Static sizing.** Pools are allocated once at load from the header. Bounded by
  a compile-time maximum plan size. No allocation in the control loop.
- **Versioned twice.** `format_version` guards the container; `kernel_set_version`
  guards the available kernels. The loader rejects a plan the firmware cannot
  fully satisfy — it never executes a partially understood plan.
- **Integrity.** `crc32` (and `plan_id` hash) are checked before arming.
- **Self-describing timing.** `base_ts_ns` is the scheduler tick; `wcet_estimate_ns`
  is computed offline by the backend and checked against the period at load.

## Kernel interface — the runtime "ISA"

Every block type is a pure, deterministic, bounded-WCET function with no
allocation and no blocking:

```c
typedef struct {
    const float* params;   // read-only, from the param blob
    const float* inputs;   // gathered input signal values (in_count of them)
    float*       state;    // persistent across ticks (zeroed at arm)
    float*       outputs;  // output signal slot(s), usually one
    uint32_t     tick;     // current base tick (for time-based sources)
    float        ts;       // effective sample time = base_ts * rate_div
} kernel_ctx;

typedef void (*kernel_fn)(kernel_ctx* ctx);
```

Kernels are registered in a fixed dispatch table indexed by `kernel_id`. Adding a
block type = new id + table entry + `kernel_set_version` bump. There is a
one-to-one registry mapping backend node types (`gain`, `sum`, `transferFunction`,
`integrator`, `switch`, `delay`, sources) to `kernel_id`s, versioned together
with the backend.

Numerical note (ties to the f32-vs-double concern): implement the LTI /
transfer-function kernel as a **biquad second-order-section cascade
(direct-form II transposed)**, not one high-order difference equation, to keep
f32 coefficient sensitivity and roundoff bounded as order grows.

## Scheduling and multi-rate

- A hardware timer fires every `base_ts_ns`. Each tick, the scheduler makes
  **two** passes over the pre-ordered `blocks[]`, running a block when
  `tick % rate_div == 0`:

  ```text
  pass 1:  signals[out] = kernel_output(state, signals[in])   // all blocks
  pass 2:  state        = kernel_update(state, signals[in])   // all blocks
  ```

  **One fused pass is wrong.** The topological order covers only
  direct-feedthrough edges, so a strictly-proper plant is scheduled *before* the
  controller driving it. Its output needs no input, so the early slot is fine —
  but its state update needs `u[k]`, produced later in the same tick. Fusing
  feeds it `u[k-1]` and silently inserts a sample of delay into the loop.
  `backend/src/exec.rs` is the reference implementation and pins this with a
  test; on fixture 04 the fused variant diverges by 1.2e-2 against an f32 noise
  floor of 5.8e-6.
- Between a slower block's ticks, its last output persists in its signal slot —
  the zero-order hold from the backend numerical contract.
- v1 sets every `rate_div = 1` (single clock, `stepSize == Ts`). Multi-rate later
  keeps this format: `base_ts` becomes the **GCD of block periods** and each
  block carries its `rate_div`. No format change required.
- Strictly-proper blocks read previous outputs from `state`, so feedback loops
  resolve without algebraic loops — already guaranteed by backend validation.

## Real-time and WCET

- The control step runs at fixed priority above telemetry and networking.
- `WCET = Σ kernel WCETs` over the due blocks. The backend stamps
  `wcet_estimate_ns`; the loader **rejects** a plan whose estimate exceeds the
  period. Determinism is a checked property, not a hope.
- Measured and reported (root `AGENTS.md` metrics): control step time, WCET,
  jitter, communication latency, telemetry throughput, simulation-vs-embedded
  output error.

## Lifecycle and safety FSM

```
BOOT -> IDLE(no plan) -> LOADING -> ARMED -> RUNNING -> FAULT
                            ^                    |         |
                            +-------- reset -----+         |
          IDLE <---------------------------------- (safe) -+
```

- **LOADING**: receive DCP into a spare buffer, verify `crc32`, check
  `format_version` / `kernel_set_version`, bind I/O channels, size pools.
- **ARMED**: state pools set to each kernel's **declared initial conditions**,
  not blindly zeroed — an integrator arms to its `initialValue` and a delay line
  fills with its initial value. Both come from the packed parameter blob, so no
  separate initial-state section is needed. Outputs driven to safe defaults.
- **RUNNING**: scheduler active.
- **Hot-swap**: a new plan is loaded into the spare buffer and switched in
  atomically at a tick boundary (double buffering) — the mechanism behind
  no-reflash iteration.
- **FAULT** triggers: control-step deadline miss (watchdog), invalid/NaN signal,
  stale or out-of-range sensor sample, unsupported plan. On fault, actuator
  outputs go to per-channel **failsafe values**, and the runtime returns to a
  safe idle rather than running degraded.
- Every actuator output passes a **saturation** clamp before the HAL.

## Telemetry

Selected signal slots are sampled inside the control step into a lock-free
double/ring buffer and drained by a lower-priority thread over the network. The
control step never allocates, never blocks, and never waits on the transport.

## Decision filter (before adding anything here)

1. Does it keep the control step deterministic and its WCET computable?
2. Does it let controllers change as *data*, without reflashing?
3. Does it respect the HAL boundary (no vendor specifics in the control core)?
4. Can it be measured against the simulation?

## Non-goals (v1)

- No per-model code generation or reflash-per-change.
- No dynamic allocation or unbounded work on the control path.
- No arbitrary user code execution on the device.
- No multi-rate execution yet (format reserves it; scheduler runs one rate).
- Deployable RST / PID authoring form is decided at the backend layer; firmware
  only executes kernels, so it is unaffected by that choice.
