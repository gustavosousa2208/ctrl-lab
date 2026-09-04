# Zephyr workspace — state and local patches

Survey of the build environment ctrl-lab firmware will use, taken **2026-09-03**
on `remote-macos-gusta-mac` (`~/zephyrproject`). Re-run the checks at the bottom
if anything here looks stale.

The short version: the workspace is sound and `mini_stm32h743` is supported, but
it is **not a neutral Zephyr install** — it is the Atletec EPTS work workspace,
and its `zephyr` tree carries 10 uncommitted local patches. Seven of them cannot
affect an H7 build. Three can.

## What is installed

| | |
| --- | --- |
| Workspace | `~/zephyrproject` (not itself a git repo) |
| Zephyr | **v4.3.0**, `3568e1b6d5c`, released 2025-11-12 |
| west | v1.5.0, in `~/zephyrproject/.venv` (Python 3.14.7) |
| Zephyr SDK | 0.17.4, at `~/zephyr-sdk-0.17.4` — outside the workspace, so it is shared |
| Manifest | stock upstream `zephyr/west.yml`, unmodified |
| `mini_stm32h743` | **present** in `zephyr/boards/weact/` — WeAct Studio MiniSTM32H743 Core Board, 512 KB RAM / 2 MB flash |
| Flashing | `dfu-util` over USB with BOOT0 held (`0483:df11`), or J-Link. No onboard debugger. |
| Repos in workspace | 49 |
| Disk | 30 GB free of 228 GB (86% used) |

`west`, `cmake` and `ninja` are **not** on the non-interactive SSH `PATH` — they
live in the venv and in Homebrew. Activate the venv, or call
`~/zephyrproject/.venv/bin/west` by full path, in any script that runs over SSH.

> zsh gotcha that will waste your time: if *any* glob in a command fails to
> match, zsh aborts the **entire** command with `no matches found` and nothing
> runs. A check like `ls -d ~/zephyrproject/.venv ~/zephyrproject/*/.venv` will
> silently not test the first path. Use `setopt +o nomatch` or avoid globs in
> remote one-liners.

## Local patches in the Zephyr tree

10 tracked files modified, +110/−25, none committed to any branch. Verdict is for
our target, **STM32H743VIT6**:

| File | What it does | Affects H7? |
| --- | --- | --- |
| `cmake/modules/FindZephyr-sdk.cmake` | Quotes `${ZEPHYR_TOOLCHAIN_VARIANT}` in a `STREQUAL`, so an undefined variant is not a CMake parse error | **Yes** — build system, every build |
| `drivers/rtc/rtc_ll_stm32.c` | Moves `stm32_backup_domain_enable_access()` *before* the RTC bus-clock enable; adds a disable on the error path | **Yes** — not SOC-guarded, applies to all STM32 |
| `subsys/zbus/zbus.c` | Adds the channel name to a `LOG_ERR` string | Cosmetic only |
| `drivers/clock_control/clock_stm32_ll_n6.c` | STM32N6 clock control | No — N6-only file |
| `drivers/interrupt_controller/intc_gpio_stm32.c` | EXTI port-index discontinuity (PORTN vs PORTI) | No — inside `CONFIG_SOC_SERIES_STM32N6X` |
| `drivers/timer/stm32_lptim_timer.c` | Enables LPTIM1 EXTI line 52 for WFI wake; adds N6 to two series lists | No — N6-guarded. Adds one unconditional `#include <stm32_ll_exti.h>`, which is harmless |
| `drivers/usb/udc/udc_stm32.c` | Two independent changes: an early return in `HAL_PCD_ResumeCallback` when the device is not suspended, and a **cold-boot hang** fix waiting on `USB33RDY` | **Partly** — the resume-callback guard is *not* SOC-scoped and applies to every STM32. The USB33RDY fix is inside `#elif defined(CONFIG_SOC_SERIES_STM32N6X)`; H7 has its own separate branch and is untouched by it |
| `subsys/usb/device_next/class/Kconfig.msc` | New `USBD_MSC_READ_ONLY` option, `default n` | No — opt-in, off unless selected |
| `subsys/usb/device_next/class/usbd_msc_scsi.{c,h}` | Implements that read-only behavior (`DATA_PROTECT`, `MODE SENSE` write-protect bit) | No — behind the Kconfig above |

**The three to remember:**

- `FindZephyr-sdk.cmake` — a freshly re-cloned Zephyr may fail to configure
  where this one succeeds.
- `rtc_ll_stm32.c` — changes STM32 RTC init ordering for *every* series. Only
  bites us if we enable the RTC, which the PoC does not.
- `udc_stm32.c`'s resume-callback guard — generic, so it is live on H7 the
  moment we use Zephyr's USB device stack. **We do**: the board's console is USB
  CDC ACM, and a verified build resolves `CONFIG_USB_DEVICE_STACK_NEXT=y`. It
  does *not* affect flashing — DFU runs from the ROM bootloader, not this
  driver — but it is in the path of anything that prints. See
  [`BRINGUP.md`](BRINGUP.md).

None of the three blocks the PoC. All three are worth knowing before debugging
something odd.

### Everything else is clean

Six other repos showed as dirty — `modules/hal/{cmsis,espressif,libmetal,st,stm32}`
and `modules/lib/picolibc`. Every one is **`.DS_Store` only**. No vendor HAL is
patched, including `modules/hal/stm32`.

### These patches are fragile

They are uncommitted, on no branch, and unpushed. A `west update`, a `git
checkout`, or a stray `git restore` in that tree destroys all ten with no
recovery path. Two mitigations:

1. Already done — a snapshot is saved **outside** the zephyr tree at
   `~/zephyrproject/.local-patches/zephyr-v4.3.0-local-20260903.patch`, with the
   base commit in `zephyr-BASE.txt` alongside it.
2. Recommended, not done — commit them to a local branch in the zephyr repo
   (`git checkout -b gusta-local-patches`). That is a change to a shared work
   workspace, so it is your call, not ours.

**Do not run `west update` in this workspace** as part of ctrl-lab work. There is
no reason to, and the blast radius lands on someone else's in-flight work.

## This is a shared work workspace

`~/zephyrproject/AGENTS.md` identifies it as the Atletec EPTS workspace, with its
own `HANDOFF.md` protocol tracking which repos are dirty with in-flight work.
Alongside Zephyr it holds `nrf52-bringup-fw`, `stm32n6_bringup_fw`,
`stm32n6-fsbl`, `coletec-dwm`, `atletec-uwb-dwm-bench` and `agent-sidecar`. Two
of those have uncommitted work and one has a stash.

Consequences for us:

- Never touch anything in there outside our own application directory.
- Read that `HANDOFF.md` before working in the workspace; it is the authority on
  which dirt belongs to whom.
- Our firmware is a personal project living in work infrastructure. Keep the
  ctrl-lab source in *this* repo and build it against that workspace, rather than
  adding a directory to it.

### Recommended arrangement

Build `firmware/` as a **freestanding Zephyr application** pointing at the
existing tree via `ZEPHYR_BASE`, rather than creating a second workspace.

- Costs **zero** extra disk. A dedicated workspace means another ~4.2 GB
  (zephyr 1.4 GB + modules 2.8 GB) against 30 GB free on a machine that has
  already been through a disk-optimization pass. The SDK is shared either way.
- We inherit the ten patches above — which, for an H7 target, is seven no-ops
  plus a build-system fix, an RTC ordering change we do not exercise, and a USB
  resume guard that only matters if we use the USB device stack.

Revisit and split into a dedicated workspace if we need a different Zephyr
version, or if the shared tree starts causing us trouble. `~/seesaw-rtos-zephyr`
is precedent that a second workspace is workable.

**This arrangement is verified**, not theoretical: `firmware/bringup/` builds
against this workspace with `ZEPHYR_BASE` pointed at its `zephyr` tree, without
adding anything to it. See [`BRINGUP.md`](BRINGUP.md) for the exact invocation.

## Re-checking this

```bash
# every repo with real modifications (ignoring .DS_Store)
cd ~/zephyrproject && for d in $(find . -maxdepth 5 -name .git -exec dirname {} \;); do
  n=$(cd $d && git status --porcelain | grep -v '\.DS_Store' | wc -l)
  [ "$n" != "0" ] && echo "$d: $n"
done

# the current zephyr patch set
cd ~/zephyrproject/zephyr && git diff --stat && git rev-parse HEAD
```

Note the survey must reach depth 5 — `modules/hal/stm32/.git` is four levels
down, so a `-maxdepth 3` scan misses every vendor HAL.
