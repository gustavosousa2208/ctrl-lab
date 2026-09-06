#!/bin/bash
# Build the host harness: the firmware control core, compiled natively.
#
#   host/build.sh            -> ./ctrl-host (beside this script)
#
# The flags that matter are the two floating-point ones, and they are the whole
# reason this build is not just `cc *.c`:
#
#   -ffp-contract=off   GCC and Clang both default to contracting `a*b + c`
#                       into a fused multiply-add. FMA keeps the product
#                       un-rounded, so it is MORE accurate than the reference -
#                       and therefore wrong here, because bit-for-bit agreement
#                       with backend/src/exec.rs is the whole test. Rust does
#                       not contract, so neither may we.
#   -fno-fast-math      Belt and braces: nothing in the build should be allowed
#                       to reassociate f32 arithmetic.
#
# The Zephyr build sets the same two in ../CMakeLists.txt. If a trace ever
# disagrees between here and the board, check these first.
set -e
cd "$(dirname "$0")"

CC="${CC:-cc}"

$CC -std=c11 -O2 -Wall -Wextra -Wno-unused-parameter \
    -ffp-contract=off -fno-fast-math \
    -I../src \
    -o ctrl-host \
    main.c ../src/dcp.c ../src/kernels.c ../src/runtime.c ../src/trace.c

echo "built $(pwd)/ctrl-host"
