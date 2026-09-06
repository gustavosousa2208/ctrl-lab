# Control runtime — stage D

The first thing in this project that executes a Deployable Control Plan. It
loads a plan, arms it, runs it, and prints a trace that can be graded
bit-for-bit against `backend/src/exec.rs`.

The architecture it implements is in [`../AGENTS.md`](../AGENTS.md); the
numerical contract is in [`../../backend/AGENTS.md`](../../backend/AGENTS.md).
This file is about what exists and how to run it.

## Status

| | |
| --- | --- |
| Loader, scheduler, all 10 kernels | written |
| Bit-exact vs. the f32 reference | **verified, all four fixtures** — off-target |
| Built for `nucleo_f767zi` | **[V]** warning-free, on macOS and WSL |
| Run on the board | **not yet** — needs a flash from Windows |
| Driven by a hardware timer | no. Steps run back to back; see below |

## The two builds

The same sources under `src/` build twice, which is the point.

```bash
# 1. Natively, and grade it against the committed reference. No board needed.
bash firmware/ctrl/host/build.sh
./firmware/ctrl/host/ctrl-host test-projects/04-2nd-order-system.plan.dcp 501 \
  | python3 firmware/scripts/grade-trace.py test-projects/04-2nd-order-system.f32.csv

# 2. For the board.
bash firmware/scripts/build.sh ctrl nucleo_f767zi -p always
#   ...then flash and read the console from Windows:
#   firmware\scripts\flash.ps1
#   firmware\scripts\console.ps1 > run.txt
python3 firmware/scripts/grade-trace.py test-projects/04-2nd-order-system.f32.csv run.txt
```

Pick a different fixture with `-- -DCTRL_PLAN=02-feedback-TF`. The step count is
read from that fixture's `.f32.csv` at configure time, so the device always runs
exactly as many ticks as the trace it is graded against.

The host build is not a mock. It is the same loader, the same scheduler and the
same kernels, and it catches a numerical bug before the board is involved — the
board then only has to confirm that a different FPU agrees. It is also the only
way to work on this from a machine without the hardware attached.

## Grading: two claims, not one

`grade-trace.py` reports both, and they are not interchangeable.

- **max |device − reference|**, against the project's 5.8e-6 f32 noise floor.
- **the trace digest**, which is the actual bit-for-bit check —
  `ctrl-backend --trace-hash <project.json>` prints the value to match.

The committed `NN-*.f32.csv` cannot settle bit-for-bit on its own: it holds nine
decimal places, and nine decimals do not always round-trip an f32 (`1e-8` prints
as `0.000000010`). That costs nothing for the error bound, since such values are
tiny in absolute terms, but it means only the digest can prove exactness.

Current digests, pinned by `firmware_trace_digests_are_pinned` in `exec.rs`:

| fixture | digest |
| --- | --- |
| `01-double-integrator` | `0xe4b8b80578162eaf` |
| `02-feedback-TF` | `0xf6a43fbffe09b100` |
| `03-TF-test` | `0xf2ef1769744e1a56` |
| `04-2nd-order-system` | `0xfddb22c1a9525b2c` |

All four are reproduced exactly by the C control core. **[V]**

## The one flag that decides all of this

`-ffp-contract=off`, set in `CMakeLists.txt` and `host/build.sh`.

GCC and Clang both default to contracting `acc += a * b` into a fused
multiply-add. FMA does not round the intermediate product, so the fused form is
*more* accurate than `exec.rs` — and therefore wrong here, because the contract
is bit-for-bit agreement, not "close enough". Rust does not contract.

This was measured, not assumed. Rebuilding the host harness with
`-ffp-contract=fast`:

| fixture | digest changes? | max error vs. reference |
| --- | --- | --- |
| `01-double-integrator` | no | 5.0e-10 (unchanged) |
| `02-feedback-TF` | **yes** | 1.2e-7 |
| `03-TF-test` | **yes** | 2.4e-7 |
| `04-2nd-order-system` | **yes** | 4.1e-6 |

The third column is the point. The contracted build is wrong on three of four
fixtures, and even its worst case — 4.1e-6 — is still **inside** the 5.8e-6
noise floor. A tolerance-based grading passes all four. This is why the digest
exists. **[V]**

## Layout

| file | what it owns |
| --- | --- |
| `src/dcp.{h,c}` | decode and validate a plan; CRC-32 and FNV-1a64 |
| `src/kernels.{h,c}` | the 10 kernels and the dispatch table |
| `src/runtime.{h,c}` | static pools, arming, the two-pass scheduler |
| `src/trace.{h,c}` | the sample digest, shared by both harnesses |
| `src/main.c` | Zephyr harness: DTCM check, self-test, run, report |
| `host/main.c` | native harness, same output format |
| `plan_blob.h.in` | CMake embeds the chosen `.dcp` through this |

## Design notes worth knowing before changing anything

**The step is two passes.** All block outputs, then all state updates. The
topological order covers only direct-feedthrough edges, so a strictly-proper
plant is scheduled *before* the controller driving it; fusing the passes feeds
it `u[k-1]` and silently inserts a sample of delay. On fixture 04 the fused
variant diverges by 1.2e-2 against a 5.8e-6 floor. `runtime.h` carries the long
version.

**Validation is exhaustive at load so the step needs none.** Every signal and
state index is proved in range by `ctrl_plan_load`, which is what lets
`ctrl_step()` be straight-line code with a computable WCET. Rejection paths are
exercised: corrupt body → CRC mismatch, truncation → too short, bad magic, and
both version gates. **[V]**

**Summation order is load-bearing.** f32 addition is not associative. Where a
loop in `kernels.c` looks tightenable, it is written to match `exec.rs`
term for term, and that is why.

**The pools are in DTCM; the trace buffer deliberately is not.** 384 B of DTCM
holds the signal and state pools — the hot path. The ~68 KB trace buffer sits in
ordinary cacheable SRAM, which also gives the cache A/B something to measure for
the first time (`caches-off.conf`); the bring-up probe's "caches are free"
result only ever covered a DTCM-resident working set.

## What is not here yet

- **No hardware timer.** Steps run back to back. Stage D asks whether the
  numbers are right and what a step costs; the measurement this prints is what
  sizes the tick budget, so the timer comes after it.
- **No transport.** The plan is linked in, not received. That is stage E, and
  keeping it out means a wrong number here has exactly one possible cause.
- **No I/O bindings.** The format carries the section, the backend emits it
  empty, and the loader *rejects* a non-empty one rather than pretending to bind
  channels it has no HAL for.
- **No fault FSM.** A kernel fault stops the run and reports; there is no
  ARMED/RUNNING/FAULT state machine, no failsafe outputs, and no NaN guard on
  the signal pool. `../AGENTS.md` describes the target.
- **`wcet_estimate_ns` is still 0**, so the loader's WCET rejection is vacuous.
  The check is written and correct; the backend has nothing to stamp yet. The
  `step_ns max` this prints on hardware is the number that closes that loop.
