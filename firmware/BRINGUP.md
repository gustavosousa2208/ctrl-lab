# Bring-up

Two boards appear here. The project moved to the **NUCLEO-F767ZI** on
2026-09-05; the **WeAct MiniSTM32H743** sections are kept below because they
still hold for that board, and most of the analysis carried over intact.

- [The NUCLEO-F767ZI](#the-nucleo-f767zi) — current target.
- [The WeAct MiniSTM32H743](#the-weact-ministm32h743) — previous target.

## The NUCLEO-F767ZI

Verified 2026-09-05. **Everything in this section is a build result [V]. No
runtime output has been read yet [?]** — the board had not enumerated on the
host at the time of writing.

The switch was cheap, and mostly a win. The board is `nucleo_f767zi` in Zephyr
v4.3.0, and the probe built for it **unmodified except for comments**.

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

### The one regression: no double-precision FPU

`soc/st/stm32/stm32h7x/Kconfig` has

```
select CPU_HAS_FPU_DOUBLE_PRECISION if CPU_CORTEX_M7
```

`soc/st/stm32/stm32f7x/Kconfig` **does not**, though it does select
`CPU_HAS_FPU`, `CPU_HAS_ICACHE` and `CPU_HAS_DCACHE`. The symbol is promptless
in `arch/Kconfig`, so it is select-only and **cannot be turned on from
`prj.conf`**.

The STM32F767 silicon *does* have the FPv5-D16 double unit — this is a Zephyr
packaging gap, not a hardware limit. Either way, on this board as Zephyr builds
it, `float` is hardware and `double` is software-emulated.

That does not affect the control plan, which is f32 end to end. It does mean a
stray `double` in a kernel — a bare `0.1` literal rather than `0.1f`, `sin()`
rather than `sinf()` — becomes a soft-float library call with unbounded WCET. On
this board that is a determinism bug, not merely a slow path. The probe prints
`fpu_dp=` so the fact is visible at boot rather than remembered.

Note this reverses, for the third time, what the project believes about f64 on
the target: soft on the Cortex-M4F originally assumed, hardware on the H743,
soft again here.

### Building and flashing

The toolchain now lives in **WSL** (Ubuntu 24.04); the board's ST-Link
enumerates on **Windows**. Rather than forwarding USB into WSL with `usbipd`,
the flow reaches the other way — WSL's filesystem is visible from Windows at
`\\wsl.localhost`, so Windows flashes a hex that Linux produced. One less moving
part, and no driver juggling.

```bash
# in WSL
bash firmware/scripts/build.sh bringup nucleo_f767zi -p always
```

```powershell
# in Windows PowerShell
firmware\scripts\flash.ps1
firmware\scripts\console.ps1
```

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
uses **1.16% of flash and 1.30% of RAM**, so there is room to be generous.

Stage D's record-then-dump trace — 5000 ticks × 6 signals × 4 B = 120 KB — fits
main SRAM comfortably. Keep it there rather than in DTCM: it is written once per
tick and read never, so it gains nothing from tightly-coupled memory, while the
signal and state pools are touched several times per tick and do.

### Known weakness in the probe

`fake_step()` reads and writes only DTCM, which is **never cached**. So the
current cache A/B measurement can only show I-cache effects on the code path —
it cannot show what D-cache costs, because it never touches cacheable memory.
Before drawing any conclusion from an `ICACHE=n` / `DCACHE=n` comparison, give
the probe a working set in `sram0` as well.

Recorded rather than fixed, deliberately: the first flash should test the
pipeline, not new code.

## The WeAct MiniSTM32H743

Previous target, retained for reference. Everything below was **built and
verified** against Zephyr v4.3.0 on `remote-macos-gusta-mac` on 2026-09-04. No
hardware was attached — a build proves configuration and linking, not runtime
behavior.

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
