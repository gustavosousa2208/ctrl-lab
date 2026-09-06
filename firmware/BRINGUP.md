# Bring-up

Two boards appear here, and as of 2026-09-06 **both are running**. The
NUCLEO-F767ZI is the controller; the WeAct MiniSTM32H743 was brought up for the
stage E two-board HIL loop and is the plant.

- [The NUCLEO-F767ZI](#the-nucleo-f767zi) — controller.
- [The WeAct MiniSTM32H743](#the-weact-ministm32h743) — plant, running since
  2026-09-06.

## The NUCLEO-F767ZI

**Running on hardware since 2026-09-06 [V]** — flashed over the onboard ST-Link
and read back over its virtual COM port. Everything below is a measurement or a
generated artifact unless tagged otherwise.

The switch was cheap, and mostly a win. The board is `nucleo_f767zi` in Zephyr
v4.3.0, and the probe built for it **unmodified except for comments**.

### What the board actually says

The full boot output, reproducible across resets:

```
*** Booting Zephyr OS build v4.3.0 ***
ctrl-lab bringup probe
board nucleo_f767zi/stm32f767xx, SoC stm32f767xx
DTCM is 128 KB at 0x20000000
signal_pool @ 0x20000100  in DTCM
state_pool  @ 0x20000000  in DTCM
icache=1 dcache=1 fpu_dp=1
core 216000000 Hz, kernel tick 10000 Hz
DWT enable: naive=counts barriers-only=counts
DWT: lsr_at_entry=0x00000001 (was already unlocked) ctrl=0x40000001 cyccnt=running
63 dependent f32 MACs: first=1670 best=1653 worst=1670 cycles (spread 17)
at 1 kHz a control step may use 216000 cycles
```

The probe's three questions, answered:

1. **The pools are in DTCM.** `0x20000000` and `0x20000100`, checked at runtime
   against the devicetree rather than inferred from `nm`. This is the first time
   the project has had that as anything but linker output.
2. **The cycle counter works**, at the full 216 MHz core clock — so a control
   step can be timed.
3. **The caches cost nothing measurable**, which was not the expected answer.
   See below.

The workload is 63 dependent f32 multiply-accumulates: **1653–1670 cycles**,
about 26 cycles each, with a **spread of 17 cycles (~1%)** over 100 runs.
Identical to the cycle across resets and across both cache variants. At 1 kHz
the budget is 216 000 cycles, so this stand-in uses **0.77%** of it.

### The caches cost nothing here, and that is a property of the build

The H743 analysis called the caches "the determinism hazard on this part" and
expected the A/B to show it. Measured, the caches-off variant
(`bringup/caches-off.conf`, `CONFIG_ICACHE=n` + `CONFIG_DCACHE=n`, confirmed
absent from the generated `.config`) is **bit-identical**: `first=1670
best=1653 worst=1670`, spread 17, the same numbers to the cycle.

Two reasons, both structural rather than luck:

- **The hot pools are in DTCM, which is never cached.** D-cache has nothing to
  do with this working set by construction — that is the point of putting them
  there.
- **`CONFIG_STM32_FLASH_PREFETCH=y` in both variants.** The F7's ART accelerator
  sits in front of flash *independently* of the Cortex-M7 L1 caches, so a tight
  loop's instruction fetch is already covered with `ICACHE=n`. This is a real
  difference from the H743, where ART is not the same mechanism.

**Do not carry this conclusion into the control runtime.** It holds for a
workload that never touches cacheable memory. Once the trace buffer and plan
structures live in `sram0`, D-cache is back in the picture and the A/B has to be
re-run. The probe cannot answer that question — see the known weakness below.

### What the board gives us for free

Checked against the board `.dts` and the SoC Kconfig, not assumed:

| | WeAct H743 | nucleo_f767zi |
| --- | --- | --- |
| DTCM | needed our overlay | **already in the board's `chosen` block** — no overlay at all |
| Console | USB CDC ACM | **`usart3` (PD8/PD9) on the ST-Link VCP** |
| Flashing | DFU with BOOT0 held, or J-Link | **`stm32cubeprogrammer`**, listed first in `board.cmake` |
| ADC / DAC / PWM | none enabled | `adc1_in0_pa0`, `dac_out1_pa4`, `tim1_ch3_pe13` |
| Caches | on by default | **also on by default** — the analysis carries over |

The DTCM result is identical to the H743's, and for the same reason:

```
DTCM:  512 B   128 KB   0.39%
```

`stm32f765.dtsi` (included by `stm32f767.dtsi`) declares `dtcm: memory@20000000`
at 128 KB, and unlike the WeAct board `nucleo_f767zi.dts` already selects it:

```dts
chosen {
    zephyr,console = &usart3;
    zephyr,dtcm    = &dtcm;
    ...
};
```

So the four-line overlay that was the whole point of the H743 bring-up is not
needed here. `firmware/bringup/boards/mini_stm32h743.overlay` is retained for
that board; there is deliberately no `nucleo_f767zi.overlay`.

**The silent-failure hazard is now closed in code rather than in prose.** The
old advice was to go read the linker's DTCM line, which is easy to skip. The
probe now fails the *build* if nothing chooses `zephyr,dtcm`, and prints an
explicit in-DTCM / **NOT IN DTCM** verdict per pool at boot, computed from
`DT_REG_ADDR(DT_CHOSEN(zephyr_dtcm))`. "The section exists" and "the data landed
in it" are different claims, and only the second one matters.

### Double precision is present — and the near-miss that says why to check

**`fpu_dp=1` on the board.** Both f32 and f64 run in hardware here, as on the
H743. The control plan stays f32 for footprint, not for speed.

This entry exists because it was first recorded as the opposite, and the mistake
is worth more than the fact. `soc/st/stm32/stm32h7x/Kconfig` selects the symbol
where you would look for it:

```
select CPU_HAS_FPU_DOUBLE_PRECISION if CPU_CORTEX_M7
```

`soc/st/stm32/stm32f7x/Kconfig` does **not** — it selects only `CPU_HAS_FPU`,
`CPU_HAS_ICACHE` and `CPU_HAS_DCACHE`. Reading those two files side by side, the
conclusion "the F7 has no double-precision FPU in Zephyr" is the obvious one,
and it is wrong. The series `Kconfig.defconfig` ends with

```
rsource "Kconfig.defconfig.stm32f7*"
```

and `Kconfig.defconfig.stm32f767xx:11` supplies
`CPU_HAS_FPU_DOUBLE_PRECISION` per die.

This is the **third** time this project has been caught by exactly one thing:
concluding from a Kconfig or devicetree file without following what it pulls in.
The H743 notes below record the same error twice, on `#include`. Here it was an
`rsource` glob. The rule that would have caught all three: **never conclude from
a source file when a generated artifact can be read instead.** `zephyr/.config`
in the build directory is the ground truth for Kconfig, and it said
`CONFIG_CPU_HAS_FPU_DOUBLE_PRECISION=y` the whole time.

The probe prints `fpu_dp=` so this is checked at boot rather than remembered.

Even with the unit present, a stray `double` in a kernel — a bare `0.1` rather
than `0.1f`, `sin()` rather than `sinf()` — still costs f64 work where f32 was
intended, and silently breaks bit-exact agreement with the f32 reference
executor. Worth catching in review; just not the WCET cliff it would have been.

### Building and flashing

The toolchain now lives in **WSL** (Ubuntu 24.04); the board's ST-Link
enumerates on **Windows**. Rather than forwarding USB into WSL with `usbipd`,
the flow reaches the other way — WSL's filesystem is visible from Windows at
`\\wsl.localhost`, so Windows flashes a hex that Linux produced. One less moving
part, and no driver juggling.

```bash
# in WSL
bash firmware/scripts/build.sh bringup nucleo_f767zi -p always

# a Kconfig variant, in its own build tree so both survive
VARIANT=caches-off EXTRA_CONF=caches-off.conf \
    bash firmware/scripts/build.sh bringup nucleo_f767zi -p always
```

```powershell
# in Windows PowerShell
firmware\scripts\flash.ps1
firmware\scripts\flash.ps1 -Variant caches-off
firmware\scripts\console.ps1
```

Two things that are easy to get wrong, both handled by the scripts:

- **`console.ps1` resets the board after opening the port**, and must. The probe
  prints once at boot and then idles, so attaching to the port afterwards shows
  an empty screen — the output has already gone. Open first, then reset.
- **Reset with `mode=UR`, not `mode=HOTPLUG`.** Once `main()` returns the core
  idles in WFI and a hotplug attach fails outright with `Error: Unable to read
  device id from ROM table`. Under Reset holds NRST while connecting and always
  attaches. This is also why `flash.ps1` connects under reset.

`firmware/scripts/wsl-env.sh` holds the environment. Two things are not on the
default `PATH` and cost time to rediscover: **`cmake` and `ninja` live in the
workspace venv**, and **`dtc` ships inside the Zephyr SDK's host tools**.

Source stays on the Windows drive so edits from either side are live; the build
tree goes on the WSL native filesystem, where it is much faster than 9p.

### The WSL workspace is also not ours

Same caveat as the Mac, different owner. `~/zephyrproject` in WSL belongs to the
**imxrt1176-evkb bring-up** work — its `.west/config` manifest points at
`imxrt1176-evkb-bringup/west.yml`. **Do not run `west update` in it.**

It is Zephyr **v4.3.0 at `3568e1b6d5c`** with SDK **0.17.4** — the same commit
and the same SDK as the Mac's workspace, so builds from the two machines are
directly comparable.

It carries two uncommitted patches. Both were checked; **neither affects us**:

| File | What it does | Affects us? |
| --- | --- | --- |
| `arch/arm/core/cortex_m/prep_c.c` | boot-progress trace words written to `0x40c04044`, an i.MX RT SRC GPR | **No** — every hunk is inside `#ifdef CONFIG_MCUBOOT`, and we do not build MCUboot |
| `soc/nxp/imxrt/imxrt11xx/soc.c` | i.MX RT11xx SoC init | No — NXP-only file |

The first deserved the check rather than a glance: `prep_c.c` is in the build
path of *every* Cortex-M target, ours included. It is the `#ifdef` that makes it
a no-op, not the filename.

### Memory budget

2 MB flash, 384 KB SRAM (`sram0`, covering SRAM1+SRAM2), 128 KB DTCM. The probe
uses **1.18% of flash and 1.30% of RAM**, and 512 B of DTCM (0.39%), so there is
room to be generous.

Stage D's record-then-dump trace — 5000 ticks × 6 signals × 4 B = 120 KB — fits
main SRAM comfortably. Keep it there rather than in DTCM: it is written once per
tick and read never, so it gains nothing from tightly-coupled memory, while the
signal and state pools are touched several times per tick and do.

### Known weakness in the probe

`fake_step()` reads and writes only DTCM, which is **never cached**. So the
cache A/B can only ever show I-cache effects on the code path — it cannot show
what D-cache costs, because it never touches cacheable memory.

This was written down as a caveat before the measurement and then confirmed by
it: the A/B came back bit-identical, which is the result you would predict from
the caveat alone. **Give the probe a working set in `sram0` before treating
"caches are free" as a fact about the control runtime.**

### One unexplained anomaly: the cycle counter, once

On the very first flash, the probe reported `WARNING: DWT cycle counter is not
running` and measured `first=0 best=0 worst=0`. Every run since has counted
correctly. **It has not reproduced**, and two plausible causes were tested and
rejected — recorded here so nobody spends the time again:

| Hypothesis | Test | Result |
| --- | --- | --- |
| Cortex-M7 DWT needs its CoreSight Lock Access Register unlocked with `0xC5ACCE55` | print `DWT->LSR` *before* unlocking | **Rejected.** `lsr_at_entry=0x00000001` — LAR present, already unlocked. The unlock never fires |
| The `TRCENA` write needs a barrier before DWT registers accept writes | run the naive sequence and a barriers-only sequence, cold, and compare | **Rejected.** `naive=counts barriers-only=counts` — the tutorial sequence works fine |

A third possibility — that the failed `mode=HOTPLUG` attach immediately before
that run left the debug subsystem wedged — was also tested by provoking the same
failed attach and resetting. The counter kept working. Rejected.

So the cause is unknown. What changed is that the failure can no longer be
quiet:

- `dwt_try_enable()` verifies with **two reads of `CYCCNT` compared against each
  other**, not one read compared against zero. The single-read check is itself a
  race — the enabling write can still be in flight — and it was observed
  reporting `DEAD` alongside a plausible nonzero measurement, which is how the
  race was noticed.
- `DWT_CTRL.NOCYCCNT` is checked, so a core without the counter is distinguished
  from one that has it and is not counting.
- The LAR unlock is kept even though it does not fire here. It costs two reads
  at boot and is correct on a part that does lock it.

**This matters for stage D**: a dead cycle counter reports zero, and zero looks
like a fast control step rather than a broken measurement. Never accept a timing
of 0.

Recorded rather than fixed, deliberately: the first flash should test the
pipeline, not new code.

## The WeAct MiniSTM32H743

**Running on hardware since 2026-09-06 [V]** — flashed over SWD with a J-Link,
console over SEGGER RTT. Brought up as the plant end of the stage E two-board
loop. The analysis below it was written on 2026-09-04 without hardware; the
runtime results are in the next section and the older analysis held up.

### What the board says

```
board mini_stm32h743/stm32h743xx, SoC stm32h743xx
DTCM is 128 KB at 0x20000000
signal_pool @ 0x20000100  in DTCM
state_pool  @ 0x20000000  in DTCM
icache=1 dcache=1 fpu_dp=1
core 240000000 Hz, kernel tick 10000 Hz
DWT: lsr_at_entry=0x00000000 (was already unlocked) ctrl=0x40000001 cyccnt=running
63 dependent f32 MACs: first=768 best=760 worst=768 cycles (spread 8)
```

Every question the probe asks, answered on this part: **the overlay works and
the pools are in DTCM**, `fpu_dp=1` so f64 is hardware here too, and the cycle
counter runs at the full 240 MHz. As on the F767, the DWT Lock Access Register
was **already unlocked** at entry, and both enable sequences work.

### It is much faster than the F767, and that was not expected

Same source, same SDK, same 63 dependent f32 MACs:

| | F767ZI @216 MHz | H743 @240 MHz |
| --- | --- | --- |
| cycles | 1653–1670 (spread 17) | **760–768 (spread 8)** |
| wall clock | 7.65–7.73 us | **3.17–3.20 us** |

**2.2x fewer cycles, 2.4x faster in wall clock.** For identical code on two
Cortex-M7 cores, that has to be instruction fetch: the H7 reads flash 256 bits
at a time over AXI, against the F7's 128-bit ART path. **[V]** for the numbers,
**[I]** for the reason — it has not been isolated, and a controller that must
hit a deadline on both boards should be sized against the F767.

### Caches are NOT free here, unlike on the F767

| | caches on | caches off | delta |
| --- | --- | --- | --- |
| F767ZI | 1653–1670 | 1653–1670 | **bit-identical** |
| H743 | 760–768, spread 8 | 775–785, spread 10 | **~15 cycles, ~2%** |

The F767 section below predicted this: *"a difference from the H743, where ART
is not the same mechanism."* Confirmed. The working set is entirely in DTCM on
both boards, so this is instruction fetch, and the H7 has no ART accelerator to
make L1 redundant. Small, but no longer zero — and it means the F767's "caches
cost nothing" result must not be generalised to this part. **[V]**

### RTT on this board: three traps, all of them measured

The console is USB CDC ACM by default, which loses everything printed before
enumeration and puts a USB device stack on the control path. RTT avoids both,
and cost three separate failures to get working. `firmware/bringup/rtt.conf`
carries the fixes and the reasoning; the short version:

1. **The D-cache eats RTT.** The Cortex-M7 writes the RTT buffer into cache and
   the debug probe reads physical SRAM through the AHB-AP, so it sees nothing.
   The symptom is precise and misleading: the `"SEGGER RTT"` magic string is
   visible at `_SEGGER_RTT` — one cache line happened to be evicted — while
   `MaxNumUpBuffers` immediately after it still reads `0`, so the host reports
   *"RTT Control Block not found"* for a block that is plainly in memory.
   Fix: `CONFIG_SEGGER_RTT_SECTION_DTCM=y`. DTCM is never cached, which is the
   same reason the signal and state pools live there.
2. **Idle kills the debug port.** RTT is read over SWD, so it only works while
   the debug port is alive. When `main()` returns, Zephyr idles into WFI and the
   H743 takes the core domain off the debug bus: J-Link reports *"DAP initialized
   successfully"* then *"Can not attach to CPU"*, and OpenOCD reads
   *"Cortex-M PARTNO 0x0"*. The board looks bricked and is not — but the only way
   back in is BOOT0 plus a power cycle. `CONFIG_STM32_ENABLE_DEBUG_SLEEP_STOP=y`
   is the documented fix and **did not hold on this part**; the probe therefore
   ends in a busy loop under `#ifdef CONFIG_RTT_CONSOLE` instead. With that in
   place the board stays attachable and can be reflashed with no button presses.
3. **`JLinkRTTLogger` never found the block**, with or without
   `-RTTSearchRanges`. Reading the control block and its buffer directly with
   `mem32`/`mem8` works and is what produced the transcript above: the CB gives
   `pUp` and `WrOff`, and the bytes are just there.

Recovery, if the board ever does stop responding: hold BOOT0, power-cycle, keep
it held about a second, release. That runs the ROM bootloader, which never
sleeps, so the core stays attachable. Then flash normally.

### Disk footprint

Measured on the Mac, 2026-09-04. The point of the table is that **most of this
is already installed** — the marginal cost of doing ctrl-lab work there is the
last section, about 1.7 GB.

| | Size | Notes |
| --- | --- | --- |
| **Zephyr side** | | |
| `zephyrproject/zephyr` | 1.4 GB | the tree itself |
| `zephyrproject/modules` | 2.8 GB | every HAL for every vendor |
| `zephyrproject/.venv` | 1.8 GB | west and its Python dependencies |
| `zephyrproject/tools`, `bootloader` | 35 MB | |
| Zephyr SDK, `arm-zephyr-eabi` | 1.1 GB | the only architecture we need |
| Zephyr SDK, `xtensa-…` | 188 MB | ESP32; already installed, unused by us |
| **Host toolchains** | | |
| Xcode Command Line Tools | 1.9 GB | also Tauri's macOS prerequisite |
| Rust (Homebrew) | 437 MB | |
| Bun | 461 MB | |
| **ctrl-lab itself** | | |
| Checkout, fully built | 933 MB | of which: |
| ⤷ `frontend/src-tauri/target` | 709 MB | Tauri pulls ~400 crates |
| ⤷ `backend/target` | 128 MB | |
| ⤷ `frontend/node_modules` | 92 MB | |
| ⤷ `.git` | <1 MB | the repo is tiny |

**Totals.** A machine with nothing installed needs roughly **12 GB** for all of
it. Trimming to what this project actually uses — ARM-only SDK, no Xtensa —
brings the Zephyr side to about 7 GB.

Two things worth knowing before you clone a second workspace:

- The **`modules` directory is the expensive part at 2.8 GB**, and almost all of
  it is HALs for vendors we will never build. `west update` accepts a group
  filter, so a ctrl-lab-only workspace could pull far less. That is the argument
  for a separate workspace if disk ever gets tight — not the zephyr tree itself.
- `frontend/src-tauri/target` at 709 MB dwarfs everything else in the checkout.
  `cargo clean` there reclaims it, at the cost of a ~50 s rebuild.

### Building it

```bash
export ZEPHYR_BASE=~/zephyrproject/zephyr
export ZEPHYR_SDK_INSTALL_DIR=~/zephyr-sdk-0.17.4
export PATH=/opt/homebrew/bin:$PATH          # cmake, ninja
cd firmware/bringup
~/zephyrproject/.venv/bin/west build -p always -b mini_stm32h743 .
```

`west` is in the workspace venv and not on the non-interactive SSH `PATH` — call
it by full path or activate the venv first. See
[`ZEPHYR-WORKSPACE.md`](ZEPHYR-WORKSPACE.md).

Flashing, once boards exist: `west flash --runner jlink` (the board declares
`--device=STM32H743VI`), or `dfu-util` over USB with BOOT0 held. With SWD wired
up, J-Link is the better path — it also unlocks RTT, see Telemetry below.

### Two things I got wrong earlier, now corrected

Both came from reading a devicetree without following its `#include`s. Recorded
because the same mistake is easy to repeat.

#### The board does have a console

An earlier note in `POC-PLAN.md` said there was no console and no UART, based on
the `chosen` block at the top of `mini_stm32h743.dts`. That block genuinely only
sets `zephyr,sram`, `zephyr,flash` and `zephyr,display` — but **line 161** pulls
in `boards/common/usb/cdc_acm_serial.dtsi`, which adds:

```dts
zephyr,console   = &board_cdc_acm_uart;
zephyr,shell-uart = &board_cdc_acm_uart;
```

So the console is **USB CDC ACM over the board's USB-C port**, and a verified
build resolves `CONFIG_CONSOLE=y`, `CONFIG_UART_CONSOLE=y` and
`CONFIG_USB_DEVICE_STACK_NEXT=y` with no work from us. `printk` reaches a host
serial terminal out of the box.

One consequence worth flagging: because the console rides the USB device stack,
the `udc_stm32.c` patch in the shared workspace — the non-SOC-guarded
`HAL_PCD_ResumeCallback` guard — **is in our build path**. It was previously
filed as "only matters if we use USB CDC". We do.

Two caveats for stage D:

- Output produced before the host enumerates the CDC device is lost. Fine for a
  record-then-dump design, fatal for early boot debugging.
- The USB stack is not something you want running on the control path. Dump
  after the run, not during it.

#### DTCM already exists

The other earlier claim was that `stm32h743.dtsi` defines no DTCM region and we
would need to write one. Wrong for the same reason: `stm32h743.dtsi` includes
`stm32h742.dtsi`, which already declares

```dts
dtcm: memory@20000000 { compatible = "zephyr,memory-region", "arm,dtcm";
                        reg = <0x20000000 DT_SIZE_K(128)>; };
itcm: memory@0        { compatible = "zephyr,memory-region", "arm,itcm";
                        reg = <0x0 DT_SIZE_K(64)>; };
```

**128 KB of DTCM and 64 KB of ITCM.** The board simply never selects them. The
whole fix is a four-line overlay (`bringup/boards/mini_stm32h743.overlay`):

```dts
/ { chosen { zephyr,dtcm = &dtcm; }; };
```

That is the pattern copied from `boards/witte/linum` — the only STM32 board in
the tree that chooses DTCM. Choosing it creates the `__dtcm_*` linker sections,
after which `__dtcm_bss_section` places data in tightly-coupled memory.

Verified, not assumed:

```
DTCM:  512 B   128 KB   0.39%
20000100 b signal_pool
20000000 b state_pool
```

Both pools land at `0x20000000+`. **Watch for the silent failure:** without the
overlay, `__dtcm_bss_section` still compiles and links — the data just falls back
to ordinary SRAM. Check the linker's DTCM usage line, not that the build passed.

### Caches are on by default

`soc/st/stm32/stm32h7x/Kconfig` selects `CPU_HAS_ICACHE` and `CPU_HAS_DCACHE`
for Cortex-M7, and `arch/Kconfig` defaults both `CONFIG_ICACHE` and
`CONFIG_DCACHE` to `y`. A verified build confirms it:

```
CONFIG_ICACHE=y
CONFIG_DCACHE=y
CONFIG_CACHE_MANAGEMENT=y
CONFIG_DCACHE_LINE_SIZE=32
```

This is the determinism hazard on this part, and it is new relative to a
Cortex-M4: execution time becomes history-dependent, so jitter stops being a
function of the code path alone. Two mitigations, in order:

1. **DTCM for the hot pools** — zero wait states, never cached, so the signal and
   state pools sidestep the question entirely. Already wired up above.
2. **Measure the cost.** Turning both caches off is a two-line `prj.conf`
   change, and that variant is verified to build:
   ```
   CONFIG_ICACHE=n
   CONFIG_DCACHE=n
   ```
   Run the probe both ways and compare the reported spread. Do not guess which
   is better — a fully cached build may well have lower jitter *and* lower mean
   once the working set is hot. The point is to have the number.

### Memory budget

512 KB main SRAM, 128 KB DTCM, 64 KB ITCM, 2 MB flash. The probe uses 3.2% of
RAM and 2.5% of flash, so there is room to be generous.

For stage D's record-then-dump trace: 5000 ticks × 6 signals × 4 B = 120 KB.
That fits DTCM, but **put it in main SRAM instead** — the trace buffer is written
once per tick and read never, so it gains nothing from tightly-coupled memory,
while the signal and state pools are touched several times per tick and do.

### What the sibling boards have that we do not

Four boards in Zephyr v4.3.0 sit on an STM32H743: `st/nucleo_h743zi`,
`fanke/fk743m5_xih6`, `google/icetower`, and ours.

| | mini_stm32h743 (ours) | nucleo_h743zi |
| --- | --- | --- |
| Console | USB CDC ACM | `usart3`, hardware UART |
| Runners | `dfu-util`, `jlink` | `stm32cubeprogrammer`, `openocd`, `jlink`, `pyocd` |
| ADC / DAC / PWM | none enabled | `adc1`, `adc3`, `dac1`, `tim12` PWM |
| Ethernet, CAN, I2C | none enabled | enabled |

Worth copying, in the order it becomes needed:

- **Nothing, for stage D.** The USB console and the DTCM overlay cover it. This
  was the surprise: the board needed less work than expected.
- **A hardware UART**, if the USB console proves awkward — losing pre-enumeration
  output, or wanting the USB stack off the control path entirely. `fk743m5_xih6`
  is the closer model here than the Nucleo: same class of cheap board, console on
  `usart1` at `pa9`/`pa10` with `current-speed = <115200>`. Adapt its `&usart1`
  block, then set `zephyr,console` in our overlay to override the CDC choice.
- **ADC and DAC**, at stage F. `nucleo_h743zi` is the reference: `adc1_inp15_pa3`,
  `adc3_inp5_pf3`, `dac1_out1_pa4`. Check every pin against the WeAct schematic
  before copying — the two boards break out different pins, and the Nucleo's
  choices are driven by its Arduino headers.
- **More flash runners**, if you have an ST-Link rather than a J-Link. These live
  in `board.cmake`, which is board-level and not overridable from an application
  overlay, so it means either carrying a local board definition or invoking
  `openocd`/`pyocd` by hand.

### Telemetry, once SWD is wired

With a J-Link on SWD, **RTT is the better answer than the USB console** for
stage D and beyond: `CONFIG_USE_SEGGER_RTT=y` plus `CONFIG_RTT_CONSOLE=y`. It
needs no USB stack on the device, survives across resets, and does not lose
early output. Not yet tried here — it needs the debug probe attached, so it is a
stage-D task rather than something a build can settle.
