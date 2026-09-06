#!/bin/bash
# Build a ctrl-lab firmware application (WSL or macOS).
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

# WSL and macOS both build; only WSL can flash. Pick the environment from the
# host rather than making the caller remember which script to source.
if [ "$(uname -s)" = "Darwin" ]; then
    . "$(dirname "$0")/mac-env.sh"
else
    . "$(dirname "$0")/wsl-env.sh"
fi

APP="${1:?usage: build.sh <app> <board> [west args]}"
BOARD="${2:?usage: build.sh <app> <board> [west args]}"
shift 2

DEST="$CTRL_LAB_BUILD/$APP/$BOARD${VARIANT:+-$VARIANT}"

# west takes `<west args> -- <cmake args>`, with exactly ONE separator. The
# caller may already have passed a `--`, and EXTRA_CONF adds a CMake argument of
# its own, so the two have to be merged rather than concatenated: appending a
# second `-- -DEXTRA_CONF_FILE=...` produced a command line where CMake silently
# ignored everything after the stray separator, and the build came out with
# neither the fragment nor the caller's defines applied.
WEST_ARGS=()
CMAKE_ARGS=()
SEEN_SEP=0
for arg in "$@"; do
    if [ "$arg" = "--" ] && [ "$SEEN_SEP" -eq 0 ]; then
        SEEN_SEP=1
        continue
    fi
    if [ "$SEEN_SEP" -eq 1 ]; then
        CMAKE_ARGS+=("$arg")
    else
        WEST_ARGS+=("$arg")
    fi
done

if [ -n "$EXTRA_CONF" ]; then
    CMAKE_ARGS+=("-DEXTRA_CONF_FILE=$EXTRA_CONF")
fi

SEP=()
if [ ${#CMAKE_ARGS[@]} -gt 0 ]; then
    SEP=(--)
fi

"$WEST" build -b "$BOARD" -d "$DEST" "$CTRL_LAB_SRC/firmware/$APP" \
    ${WEST_ARGS[@]+"${WEST_ARGS[@]}"} \
    ${SEP[@]+"${SEP[@]}"} \
    ${CMAKE_ARGS[@]+"${CMAKE_ARGS[@]}"}

echo
echo "=== artifacts ==="
ls -la "$DEST/zephyr/zephyr.hex" 2>/dev/null || true
