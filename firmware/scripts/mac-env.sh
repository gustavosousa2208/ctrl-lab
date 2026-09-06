# Environment for building ctrl-lab firmware on macOS.
#
# The companion to wsl-env.sh. WSL remains the primary firmware environment
# because the board lives on that machine, but the Mac can build: it carries a
# Zephyr workspace at the same commit (v4.3.0, 3568e1b6d5c) and the same SDK
# version (0.17.4), so a compile here is a real check on the WSL build rather
# than an approximation.
#
# The Mac cannot flash. The ST-Link enumerates on Windows; see BRINGUP.md.
#
# As in WSL, the workspace is NOT ours - it belongs to the Atletec EPTS work and
# carries uncommitted patches. We build against it as a freestanding application
# and never write into it. Do not run `west update` in it.

export ZEPHYR_BASE="$HOME/zephyrproject/zephyr"
export ZEPHYR_SDK_INSTALL_DIR="$HOME/zephyr-sdk-0.17.4"

# The macOS SDK has no sysroots/ directory - unlike the Linux one it ships no
# host tools, so dtc and gperf come from Homebrew instead. Both are present;
# if a fresh Mac is missing them: brew install dtc gperf
export PATH="$HOME/zephyrproject/.venv/bin:/opt/homebrew/bin:$PATH"

WEST="$HOME/zephyrproject/.venv/bin/west"

# Unlike WSL, the source and the build tree are both native here, so the build
# goes beside the checkout rather than on a separate filesystem.
CTRL_LAB_SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CTRL_LAB_BUILD="$HOME/ctrl-lab-build"
