#!/bin/bash
# Flash a ctrl-lab firmware build from macOS (or Linux).
#
#   flash.sh <app> [board] [variant]
#
# e.g.  flash.sh ctrl
#       flash.sh ctrl nucleo_f767zi caches-off
#
# The companion to flash.ps1, which does the same job from Windows by reaching
# into a WSL build tree. This one is simpler because the build and the board are
# on the same machine: west drives the stm32cubeprogrammer runner directly.
#
# Requires STM32CubeCLT on PATH. It installs to /opt/ST/STM32CubeCLT_*/ and is
# NOT on the default PATH, so this script finds it.
set -e
. "$(dirname "$0")/mac-env.sh" 2>/dev/null || . "$(dirname "$0")/wsl-env.sh"

APP="${1:?usage: flash.sh <app> [board] [variant]}"
BOARD="${2:-nucleo_f767zi}"
VARIANT="${3:-}"

# STM32_Programmer_CLI is what the runner shells out to.
if ! command -v STM32_Programmer_CLI >/dev/null 2>&1; then
    CLT=$(ls -d /opt/ST/STM32CubeCLT_*/STM32CubeProgrammer/bin 2>/dev/null | sort -V | tail -1)
    if [ -z "$CLT" ]; then
        echo "STM32_Programmer_CLI not found - install STM32CubeCLT" >&2
        exit 1
    fi
    export PATH="$CLT:$PATH"
fi

DEST="$CTRL_LAB_BUILD/$APP/$BOARD${VARIANT:+-$VARIANT}"
if [ ! -f "$DEST/zephyr/zephyr.hex" ]; then
    echo "no build at $DEST - run build.sh $APP $BOARD first" >&2
    exit 1
fi

"$WEST" flash -d "$DEST"

echo
echo "Flashed. Read the console with:  python3 firmware/scripts/console.py"
