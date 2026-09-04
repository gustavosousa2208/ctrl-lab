# Bring-up on the WeAct MiniSTM32H743

What `mini_stm32h743` gives us, what it does not, and what the other STM32H743
boards in Zephyr are worth copying. Everything below was **built and verified**
against Zephyr v4.3.0 on `remote-macos-gusta-mac` on 2026-09-04. No hardware was
attached — a build proves configuration and linking, not runtime behavior.

`bringup/` is the probe that produced these answers. It is not the control
runtime; it exists to settle the questions stage D depends on.

## Building it

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

## Two things I got wrong earlier, now corrected

Both came from reading a devicetree without following its `#include`s. Recorded
because the same mistake is easy to repeat.

### The board does have a console

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

### DTCM already exists

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

## Caches are on by default

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

## Memory budget

512 KB main SRAM, 128 KB DTCM, 64 KB ITCM, 2 MB flash. The probe uses 3.2% of
RAM and 2.5% of flash, so there is room to be generous.

For stage D's record-then-dump trace: 5000 ticks × 6 signals × 4 B = 120 KB.
That fits DTCM, but **put it in main SRAM instead** — the trace buffer is written
once per tick and read never, so it gains nothing from tightly-coupled memory,
while the signal and state pools are touched several times per tick and do.

## What the sibling boards have that we do not

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

## Telemetry, once SWD is wired

With a J-Link on SWD, **RTT is the better answer than the USB console** for
stage D and beyond: `CONFIG_USE_SEGGER_RTT=y` plus `CONFIG_RTT_CONSOLE=y`. It
needs no USB stack on the device, survives across resets, and does not lose
early output. Not yet tried here — it needs the debug probe attached, so it is a
stage-D task rather than something a build can settle.
