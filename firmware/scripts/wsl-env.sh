# Environment for building ctrl-lab firmware inside WSL (Ubuntu 24.04).
#
# The Zephyr workspace here is NOT ours - it belongs to the imxrt1176-evkb
# bring-up work (see the manifest in ~/zephyrproject/.west/config). We build
# against it as a freestanding application and never write into it. In
# particular: do not run `west update` in it.
#
# Nothing needed here is on the default PATH: cmake and ninja live in the
# workspace venv, dtc in the SDK's host tools.

export ZEPHYR_BASE="$HOME/zephyrproject/zephyr"
export ZEPHYR_SDK_INSTALL_DIR="$HOME/zephyrproject/.toolchains/zephyr-sdk-0.17.4"
export PATH="$HOME/zephyrproject/.venv/bin:$ZEPHYR_SDK_INSTALL_DIR/sysroots/x86_64-pokysdk-linux/usr/bin:$PATH"

WEST="$HOME/zephyrproject/.venv/bin/west"

# Source lives on the Windows drive so Windows-side edits are live; the build
# tree goes on the WSL native filesystem, where it is far faster than 9p.
CTRL_LAB_SRC="/mnt/c/Users/gusta/source/ctrl-lab"
CTRL_LAB_BUILD="$HOME/ctrl-lab-build"
