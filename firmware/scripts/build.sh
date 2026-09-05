#!/bin/bash
# Build a ctrl-lab firmware application in WSL.
#   build.sh <app-dir-under-firmware/> <board> [extra west args...]
# e.g. build.sh bringup nucleo_f767zi -p always
set -e
. "$(dirname "$0")/wsl-env.sh"

APP="${1:?usage: build.sh <app> <board> [west args]}"
BOARD="${2:?usage: build.sh <app> <board> [west args]}"
shift 2

"$WEST" build \
    -b "$BOARD" \
    -d "$CTRL_LAB_BUILD/$APP/$BOARD" \
    "$CTRL_LAB_SRC/firmware/$APP" \
    "$@"

echo
echo "=== artifacts ==="
ls -la "$CTRL_LAB_BUILD/$APP/$BOARD/zephyr/zephyr.hex" "$CTRL_LAB_BUILD/$APP/$BOARD/zephyr/zephyr.bin" 2>/dev/null || true
