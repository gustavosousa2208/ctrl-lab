#!/bin/bash
# Build and run the link framing tests natively.
#
#   test/build.sh          -> ./test-frame, then runs it
#
# Same floating-point flags as firmware/ctrl/host/build.sh, for the same
# reason: the packet carries f32 signals and nothing in this build may be
# allowed to reassociate or contract them.
#
# dcp.c is here for one function - ctrl_crc32_update, which link_frame builds
# its table from - and it drags in kernels.c for the plan loader it also
# contains. Linking the real file rather than a copy of the polynomial is the
# entire point: a divergence between the two ends would show up here.
set -e
cd "$(dirname "$0")"

CC="${CC:-cc}"

$CC -std=c11 -O2 -Wall -Wextra -Wno-unused-parameter \
    -ffp-contract=off -fno-fast-math \
    -I../src -I../../ctrl/src \
    -o test-frame \
    test_frame.c ../src/link_frame.c ../../ctrl/src/dcp.c ../../ctrl/src/kernels.c

echo "built $(pwd)/test-frame"
echo
./test-frame
