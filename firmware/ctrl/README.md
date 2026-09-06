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
| Bit-exact vs. the f32 reference | **verified on hardware, all four fixtures** |
| Built for `nucleo_f767zi` | **[V]** warning-free, on macOS and WSL |
| Run on the board | **[V]** NUCLEO-F767ZI, 2026-09-06 |
| Driven by a hardware timer | no. Steps run back to back; see below |

## What the board says

All four fixtures load, run, and produce a trace whose digest equals the
reference executor's, **bit for bit**. Measured 2026-09-06 on a NUCLEO-F767ZI
(ST-LINK SN `066BFF485753667187244749`), flashed and read from macOS. **[V]**

| fixture | steps | verdict | min | mean | max | ISR outliers |
| --- | --- | --- | --- | --- | --- | --- |
| `01-double-integrator` | 101 | bit-for-bit | 3108 | 3110 | 3341 | 0 / 100 |
| `02-feedback-TF` | 101 | bit-for-bit | 2736 | 2739 | 3080 | 0 / 100 |
| `03-TF-test` | 101 | bit-for-bit | 3824 | 3835 | 4161 | 0 / 100 |
| `04-2nd-order-system` | 501 | bit-for-bit | 3989 | 3997 | **7782** | 1 / 500 |

Cycles at 216 MHz, so a step is **13-19 us**. The pools land in DTCM at
`0x20000000` and `0x20000100`, exactly as the bring-up probe predicted; the
trace buffer lands at `0x20021bd4`, in cacheable SRAM, on purpose.

### The one big outlier is an interrupt, and that is now proven

Fixture 04's `max` is nearly double its mean. It is reproducible to the cycle
across resets — always step 373, always 7882 — which rules out noise. Building
with `-DCTRL_IRQ_LOCK=y`, which runs each step inside `irq_lock()`, settles it:

| | interrupts on | interrupts locked |
| --- | --- | --- |
| min / mean | 3953 / 3961 | 3649 / 3652 |
| max | **7882** | **3992** |
| spread | 3929 | 343 |
| steps > 1.5x fastest | 1 of 500 | **0 of 500** |

So a single ISR lands inside one step and costs ~3930 cycles (18 us). Only the
501-step fixture is long enough to catch it — the 100-step runs take ~1.9 ms and
see none, which is consistent with a rare periodic event rather than a per-step
cost. The kernel is tickless, so the nominal 10 kHz tick rate is not the
interrupt rate.

The ~300-cycle drop in *every* step under `irq_lock` is **not explained**. It is
systematic and reproducible, and it is not an interrupt, since it moves the
minimum. Recorded rather than guessed at.

**What this means for a real control loop:** the worst *uninterrupted* step is
3992 cycles, 18.5 us. At a 1 kHz tick that is 1.9% of the period, and a single
ISR intrusion adds another 1.8%. Both fit; neither is invisible.

### Caches cost nothing, now for a cacheable working set

`caches-off.conf` versus the default, on fixture 04: **bit-identical and
cycle-identical** — same digest, and min/mean/max/spread of 3989/3997/7782/3793
either way. **[V]**

The bring-up probe found the same thing, but its working set was entirely in
DTCM, which is never cached, so the result did not transfer. This run has ~68 KB
of trace buffer plus the plan structures in ordinary SRAM and still shows no
difference. The likely reason is unchanged: the hot pools are in DTCM, and the
F7's ART accelerator covers instruction fetch from flash independently of L1.
**[I]** for the explanation, **[V]** for the numbers. This closes the open item
carried since bring-up.

### The console was dropping bytes; 921600 fixed it

**Largely solved by moving to 921600 baud** — but the digest is what proved the
firmware was never at fault while it was being solved.

At the board's default 115200, a 501-row trace arrived with rows lost or mangled
on most runs. Raising the console baud fixes it, and the measurement went the
*opposite* way to the intuition that a faster line would overrun a buffer
harder. Five captures per rate:

| baud | rows lost | rows damaged | capture |
| --- | --- | --- | --- |
| 115200 | 0-2 | 0-3 | 3.6 s |
| 230400 | 0 | 0-1 | 2.2 s |
| 460800 | **0** | **0** | 1.4 s |
| 921600 | **0** | **0** | 1.0 s |

The loss is a *time-in-flight* problem, not a rate problem: less wall-clock
spent streaming means fewer windows in which the host can be late. 921600 is
also the ceiling — macOS rejects every rate above it on this driver (500000 and
1000000 included; only the standard ladder is accepted). **[V]**

Throughout all of it the on-device digest was **correct on every single run**,
including the worst-corrupted ones, because it is computed from memory before
anything is printed. That is the whole argument for having it.

Three things were tried before the baud change, and the order is instructive.
Pacing the device output with `k_msleep(1)` per row did nothing and was reverted
— the guess that the ST-Link's buffer was overrunning was simply wrong. Setting
`clocal -crtscts` on the host helped materially, because macOS defaults the port
to hardware flow control the VCP does not drive. And a **measurement bug of my
own** inflated the early numbers: `console.py` did not drain the receive buffer
before resetting, so it was reading a stale trace left over from the flash —
reporting captures faster than the line rate, and occasionally *more* rows than
the run has steps. `console.ps1` had always done this correctly with
`DiscardInBuffer()`; the Python version simply lacked it.

`grade-trace.py` still matches rows to the reference **by their time value
rather than by position** (a dropped row must not shift every comparison after
it), counts and skips damaged rows, and lets the digest decide the verdict. That
is worth keeping now that the link is clean: the next transport is a wire
between two MCUs, and it will not be.

## The two builds

The same sources under `src/` build twice, which is the point.

```bash
# 1. Natively, and grade it against the committed reference. No board needed.
bash firmware/ctrl/host/build.sh
./firmware/ctrl/host/ctrl-host test-projects/04-2nd-order-system.plan.dcp 501 \
  | python3 firmware/scripts/grade-trace.py test-projects/04-2nd-order-system.f32.csv

# 2. On the board. macOS/Linux:
bash firmware/scripts/build.sh ctrl nucleo_f767zi -p always
bash firmware/scripts/flash.sh ctrl
python3 firmware/scripts/console.py --out run.txt
python3 firmware/scripts/grade-trace.py test-projects/04-2nd-order-system.f32.csv run.txt \
  --expect-digest $(cargo run -q --manifest-path backend/Cargo.toml -- \
                      --trace-hash test-projects/04-2nd-order-system.json | grep -o '0x[0-9a-f]*')

#    ...or from Windows, against a WSL build tree:
#    firmware\scripts\flash.ps1 -App ctrl
#    firmware\scripts\console.ps1 > run.txt
```

Flashing needs STM32CubeCLT on macOS (it installs to `/opt/ST/STM32CubeCLT_*/`
and is not on the default `PATH`; `flash.sh` finds it).

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
| `caches-off.conf` | A/B fragment: same runtime, both L1 caches disabled |

Build options: `-DCTRL_PLAN=<fixture>` picks what to run and grade against;
`-DCTRL_IRQ_LOCK=y` runs each step inside `irq_lock()`, an attribution tool for
the timing question above rather than a default.

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

- **No hardware timer.** Steps run back to back. Stage D asked whether the
  numbers are right and what a step costs; both are now answered, so the timer
  is the next thing. The budget it has to fit: 3992 cycles worst-case
  uninterrupted, plus ~3930 for an ISR intrusion.
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
  number to stamp now exists — 3992 cycles, 18.5 us at 216 MHz, worst-case
  uninterrupted on fixture 04 — but it is per-board and per-plan, so the backend
  needs a policy, not just a constant.
