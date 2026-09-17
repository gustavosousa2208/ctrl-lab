#!/bin/bash
# Build and run the control-core loader tests natively.
#
#   test/build.sh          -> ./test-io-bindings, then runs it
#
# Same floating-point flags as host/build.sh. Nothing here does float
# arithmetic, but dcp.c is shared with the graded path and must be compiled the
# same way in both places.
set -e
cd "$(dirname "$0")"

CC="${CC:-cc}"

# -lm because kernels.c calls fmodf. macOS resolves the math functions out of
# libSystem without being asked; glibc does not, so every one of these linked
# fine here and failed on Linux with `undefined reference to fmodf`. It is a
# no-op on macOS, which is why the omission survived this long.
FIXTURES=../../../test-projects

$CC -std=c11 -O2 -Wall -Wextra -Wno-unused-parameter \
    -ffp-contract=off -fno-fast-math \
    -I../src \
    -o test-io-bindings \
    test_io_bindings.c ../src/dcp.c ../src/kernels.c -lm

echo "built $(pwd)/test-io-bindings"
echo
./test-io-bindings "$FIXTURES/06-link-io.plan.dcp" "$FIXTURES/04-2nd-order-system.plan.dcp"
