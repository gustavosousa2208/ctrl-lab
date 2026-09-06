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
| Driven by a hardware timer | **[V]** yes, from the plan's own `base_ts_ns` |

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

### It runs on a clock now

The step is driven by the plan's own `base_ts_ns`, not by the top of a loop.
Fixture 04 says 50 ms, so a 501-step run takes 25.87 s of wall clock — which is
the point. **[V]**

| | |
| --- | --- |
| tick period | min 10 800 000, mean 10 800 000, max 10 800 017 cycles (nominal 10 800 000) |
| tick jitter | **17 cycles peak-to-peak = 78 ns** on a 50 ms period |
| step | 4081 cycles, 18.9 us |
| awake per tick | 12 343 cycles |
| CPU load | **0.11%** |
| deadlines missed | **0 of 501** |
| trace | bit-for-bit, digest unchanged |

**The step runs in a cooperative thread woken by the timer ISR, not in the ISR
itself.** Floating point in an ISR needs `CONFIG_FPU_SHARING`, because without
it the callee-saved FP registers `s16-s31` are not preserved and an ISR doing FP
silently corrupts the interrupted thread — Cortex-M lazy stacking covers only
`s0-s15`. A thread sidesteps the question, and it is the shape `../AGENTS.md`
asks for. `CONFIG_FPU_SHARING=y` is set regardless, because `main()` and the
control thread both touch floats.

The cost of that choice is visible: of the 12 343 awake cycles per tick, only
4081 are the step. **The remaining ~8300 cycles (38 us) are scheduling** — timer
ISR, semaphore, two context switches, and the tickless SysTick reprogramming.
Running the step directly in the ISR would reclaim most of it, at the price of
the FP hazard above. Worth revisiting if the rate ever needs to go high.

### How fast it can actually go

Measured by overriding the period with `-DCTRL_TICK_NS`, on the 100 kHz kernel
tick that `fast-tick.conf` provides. **[V]**

| requested | delivered | CPU load | deadlines missed |
| --- | --- | --- | --- |
| 1 ms | 1 ms | 5.7% | 0 / 501 |
| 200 us | 200 us | 28.4% | 0 / 501 |
| 100 us | 100 us | 56.8% | 0 / 501 |
| 60 us | 60.0 us | 94.2% | 0 / 501 |
| 50 us | 50.0 us | 101.1% | 0 / 501 |
| 40 us | 40.4 us | 101.5% | **5** |
| 30 us | 46.0 us | 102.2% | **267** |
| 20 us | 140.1 us | 104.0% | **3002** |
| 10 us | — | — | livelocks before printing |

So this design tops out around **16-20 kHz**, and the limit is scheduling
overhead rather than the control step, which is only 18.9 us of a ~57 us budget.
Two things to read off the table:

- **Overload stretches the delivered period.** Past 50 us the loop cannot keep
  up and the effective period grows — 20 us requested arrives as 140 us. The
  requested rate stops being the real one, which is why `cpu_load` is computed
  against the *measured* period.
- **Overload degrades timing, not correctness.** The 20 us run missed 3002
  deadlines and ran 7x slower than asked, and its trace was still **bit-for-bit
  identical**. The scheduler drops *ticks*, not *steps*: each step still runs
  once, in order, on the state the previous one left. A control engineer should
  read that as the sampled-data model being wrong while the arithmetic stays
  right — the worst kind of failure to have in the field, and exactly why the
  deadline counter exists. **[V]**

### Two measurement traps this cost, both worth knowing

**DWT CYCCNT stops while the core sleeps.** Between ticks the idle thread
executes WFI, which gates the core clock, so the DWT-backed timing API simply
stops counting. Measuring a 50 ms period with it reported ~11 900 cycles — which
is not the period at all, it is the time the CPU was *awake*. Tick period now
uses `k_cycle_get_32()`, driven by the system timer, which keeps running across
idle. Both numbers are kept because they answer different questions: awake
cycles are the CPU cost, `k_cycle_get_32` is the period.

**`k_timer` rounds a requested period up to a whole kernel tick.** At the
default 10 kHz that makes 100 us the shortest period expressible, and every
request below it silently becomes 100 us. An earlier sweep of 80/60/50/40 us
therefore measured the same 100 us four times while reporting CPU loads above
100%, because load was being computed against the *requested* period. The
firmware now divides by the measured period and prints a note when the two
disagree.

### It runs on a second board, and agrees to the bit

`firmware/ctrl` also builds and runs on the **WeAct MiniSTM32H743**, executing
`05-plant-only` — a plan containing just the plant from fixture 04, driven by a
step. **[V]** 2026-09-06.

```
plan 05-plant-only (221 bytes): ok
signal_pool @ 0x20000100  in DTCM
trace_fnv1a64=0x6a01864d4f8e0c87        == the backend reference
VERDICT    PASS - bit-for-bit
```

Two things this establishes that one board could not.

**A plant needs no new firmware.** The H743 runs the same binary as the
controller board; only the `.dcp` differs. That is the data-driven claim in
`../AGENTS.md` — *"changing the whole diagram changes the plan, not the
firmware"* — tested rather than asserted, and it is what makes the stage E
two-board loop cheap.

**f32 execution is board-independent, not just architecture-independent.** The
same digests now hold on x86_64, arm64, STM32F767 (Cortex-M7 at 216 MHz) and
STM32H743 (Cortex-M7 at 240 MHz).

| | F767ZI, fixture 04 | H743, plant-only |
| --- | --- | --- |
| step | 3989–4083 cycles, 18.9 us | **598–843 cycles, 2.6 us** |
| tick period vs nominal | 10 800 000 / 10 800 000 | 11 999 999 / 12 000 000 |
| deadlines missed | 0 / 501 | 0 / 501 |

The step figures are not comparable head to head — six blocks against three —
but the H743's per-block speed advantage from the bring-up probe carries over.

**Two anomalies, recorded rather than tidied away:**

- **Tick jitter is 5183 ns on the H743 against 78 ns on the F767** — 66x worse,
  for the same scheduler and the same 10 kHz kernel tick. Not explained. It does
  not affect correctness (the digest matches and no deadline was missed) but it
  matters for stage E, where the two boards' clocks are the thing being measured.
- **`cpu_awake` reported a max of 10 352 907 cycles against a mean of 24 244.**
  That is 86% of a tick spent awake for a step that takes 623 cycles, which is
  not credible; it is far more likely an instrumentation fault in the awake-time
  measurement than a real stall. The mean, the step timings and the deadline
  count are all consistent, so only that one statistic is suspect.

Build it with:

```bash
EXTRA_CONF=rtt.conf bash firmware/scripts/build.sh ctrl mini_stm32h743 -p always \
  -- -DCTRL_PLAN=05-plant-only
bash firmware/scripts/flash.sh ctrl mini_stm32h743     # or west flash --runner jlink
python3 firmware/scripts/rtt-read.py <build>/zephyr/zephyr.elf --out run.bin
python3 firmware/scripts/grade-trace.py test-projects/05-plant-only.f32.csv run.bin \
  --expect-digest 0x6a01864d4f8e0c87
```

`rtt-read.py` exists because `JLinkRTTLogger` could not locate the control block
on this setup with or without `-RTTSearchRanges`. It does not need to be found:
the ELF says where it is, the block says where its buffer is and how much has
been written, and `mem8` reads the bytes. See `firmware/BRINGUP.md` for the
three separate traps RTT presented on this board.

### The trace is a binary frame

Hex text spends 9 bytes per sample (8 digits and a separator) to carry 4 bytes
of f32. The trace is now a framed binary payload instead:

| fixture | binary capture | as hex text |
| --- | --- | --- |
| `01-double-integrator` | 3 887 B | ~8 700 B |
| `02-feedback-TF` | 3 479 B | ~7 500 B |
| `03-TF-test` | 4 688 B | ~11 000 B |
| `04-2nd-order-system` | **15 086 B** | **34 078 B** |

2.26x less on the wire, and five consecutive device captures came back
byte-identical in size and all PASS. **[V]**

The layout is documented in [`src/trace.h`](src/trace.h). Three properties
matter more than the size:

- **Transport-independent.** The same bytes go over the ST-Link UART today, a
  USB CDC endpoint next, and an MCU-to-MCU wire in stage E. Nothing in the frame
  knows which.
- **Self-delimiting.** The reader scans for the `DCPT` magic and then consumes
  exactly `payload_len` bytes, so a frame can sit in the middle of ordinary
  console text with no escaping. `payload_len` is validated by `header_crc32`
  before it is used to slice — a corrupt length is the field a glitch would most
  like to get wrong.
- **Checked, in a way the digest cannot be.** The digest proves the *values*
  were right but cannot distinguish a truncated frame from a complete one. Both
  failure modes are exercised: flipping a payload byte gives "payload CRC
  mismatch", cutting the tail gives "frame truncated: need 14040 bytes, have
  13871". **[V]**

Build `-DCTRL_TRACE_TEXT=y` for the old hex rows, and run the host harness with
`--text` for the same. The grader reads either and says which it found. Text is
worth keeping because it is legible in a terminal with no decoder, which is what
you want when the question is "is the board saying anything at all".

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
| `src/trace.{h,c}` | the binary frame and the sample digest, shared by both harnesses |
| `src/main.c` | Zephyr harness: DTCM check, self-test, run, report |
| `host/main.c` | native harness, same output format |
| `plan_blob.h.in` | CMake embeds the chosen `.dcp` through this |
| `caches-off.conf` | A/B fragment: same runtime, both L1 caches disabled |

Build options: `-DCTRL_PLAN=<fixture>` picks what to run and grade against;
`-DCTRL_CONSOLE_BAUD=<rate>` overrides the 921600 default;
`-DCTRL_TRACE_TEXT=y` emits hex rows instead of a binary frame;
`-DCTRL_FREE_RUN=y` runs steps back to back with no timer (how every
pre-timer measurement was taken, and still the quick way to check the numbers:
under a second against 25.87 s);
`-DCTRL_TICK_NS=<n>` overrides the scheduling period only, leaving the model's
arithmetic alone, which is how the rate table above was measured — pair it with
`EXTRA_CONF=fast-tick.conf` for periods under 100 us; and
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

- ~~**No hardware timer.**~~ Done — see "It runs on a clock now" above.
  `-DCTRL_FREE_RUN=y` keeps the old back-to-back loop for quick checks.
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
