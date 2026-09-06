#!/bin/bash
# Build a ctrl-lab firmware application in WSL.
#
#   build.sh <app-dir-under-firmware/> <board> [extra west args...]
#
# e.g.  build.sh bringup nucleo_f767zi -p always
#
# Optional environment:
#   VARIANT=<name>        put the build in its own tree, so variants coexist
#   EXTRA_CONF=<file>     Kconfig fragment, relative to the application dir
#
# e.g.  VARIANT=caches-off EXTRA_CONF=caches-off.conf build.sh bringup nucleo_f767zi
set -e
. "$(dirname "$0")/wsl-env.sh"

APP="${1:?usage: build.sh <app> <board> [west args]}"
BOARD="${2:?usage: build.sh <app> <board> [west args]}"
shift 2

DEST="$CTRL_LAB_BUILD/$APP/$BOARD${VARIANT:+-$VARIANT}"

EXTRA=()
if [ -n "$EXTRA_CONF" ]; then
    EXTRA=(-- "-DEXTRA_CONF_FILE=$EXTRA_CONF")
fi

"$WEST" build -b "$BOARD" -d "$DEST" "$CTRL_LAB_SRC/firmware/$APP" "$@" "${EXTRA[@]}"

echo
echo "=== artifacts ==="
ls -la "$DEST/zephyr/zephyr.hex" 2>/dev/null || true
