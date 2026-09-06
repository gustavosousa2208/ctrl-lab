cmake_minimum_required(VERSION 3.20.0)

# Console baud, as a devicetree overlay generated before Zephyr reads the
# devicetree - which is why this block sits above find_package().
#
#   west build ... -- -DCTRL_CONSOLE_BAUD=921600
#
# The board .dts fixes usart3 at 115200, and that line is the trace transport:
# a 501-step run is ~31 KB of console output. Whether raising it helps is an
# empirical question - the bytes are being lost in a buffer, and a higher rate
# fills that buffer faster - so this exists to measure rather than to assume.
# 921600, not the board default of 115200. Measured 2026-09-06 over five
# captures per rate, and the result went the opposite way to the guess: faster
# is CLEANER, not noisier.
#
#   115200  0-2 rows lost, 0-3 damaged, 3.6 s
#   230400  0 lost,         0-1 damaged, 2.2 s
#   460800  0 lost,         0 damaged,   1.4 s
#   921600  0 lost,         0 damaged,   1.0 s
#
# The loss is a time-in-flight problem, not a rate problem: less wall-clock
# spent streaming means fewer windows in which the host can be late. 921600 is
# also the ceiling - macOS rejects every rate above it on this driver.
set(CTRL_CONSOLE_BAUD "921600" CACHE STRING "usart3 console baud rate")
configure_file(console-baud.overlay.in ${CMAKE_CURRENT_BINARY_DIR}/console-baud.overlay @ONLY)
list(APPEND EXTRA_DTC_OVERLAY_FILE ${CMAKE_CURRENT_BINARY_DIR}/console-baud.overlay)

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(ctrl_lab_control_runtime)

# Which committed fixture to embed and grade against.
#   west build ... -- -DCTRL_PLAN=02-feedback-TF
set(CTRL_PLAN "04-2nd-order-system" CACHE STRING "test-projects fixture to run")

set(CTRL_FIXTURES ${CMAKE_CURRENT_LIST_DIR}/../../test-projects)
set(CTRL_PLAN_FILE ${CTRL_FIXTURES}/${CTRL_PLAN}.plan.dcp)
set(CTRL_REF_FILE ${CTRL_FIXTURES}/${CTRL_PLAN}.f32.csv)

foreach(required ${CTRL_PLAN_FILE} ${CTRL_REF_FILE})
  if(NOT EXISTS ${required})
    message(FATAL_ERROR "CTRL_PLAN=${CTRL_PLAN}: missing ${required}")
  endif()
endforeach()

# Embed the plan as bytes. Reading the committed .dcp at configure time rather
# than checking in a generated header means the two can never drift apart.
file(READ ${CTRL_PLAN_FILE} CTRL_PLAN_HEX HEX)
string(REGEX REPLACE "(..)" "0x\\1, " CTRL_PLAN_BYTES ${CTRL_PLAN_HEX})

# The device runs exactly as many ticks as the reference trace has rows, so the
# two are comparable without the grader having to trim either.
file(STRINGS ${CTRL_REF_FILE} CTRL_REF_LINES)
list(LENGTH CTRL_REF_LINES CTRL_REF_COUNT)
math(EXPR CTRL_PLAN_STEPS "${CTRL_REF_COUNT} - 1")

configure_file(plan_blob.h.in ${CMAKE_CURRENT_BINARY_DIR}/include/plan_blob.h @ONLY)
message(STATUS "ctrl: plan ${CTRL_PLAN}, ${CTRL_PLAN_STEPS} steps, console ${CTRL_CONSOLE_BAUD} baud")

target_include_directories(app PRIVATE src ${CMAKE_CURRENT_BINARY_DIR}/include)
target_sources(app PRIVATE
  src/main.c
  src/dcp.c
  src/kernels.c
  src/runtime.c
  src/trace.c
)

# The single most important flag in this project.
#
# GCC defaults to -ffp-contract=fast, which fuses `acc += a * b` into VFMA. The
# fused form does not round the intermediate product, so it is MORE accurate
# than backend/src/exec.rs - and therefore wrong, because the contract for this
# stage is bit-for-bit agreement with that file, not "close enough".
#
# This is not theoretical and it is not loud. Measured on the host harness
# 2026-09-06: contraction changes the trace digest on three of the four
# committed fixtures, while the worst-case error (4.1e-6 on 04-2nd-order-system)
# stays INSIDE the project's 5.8e-6 noise floor. A tolerance-based grading would
# have passed it. Rust does not contract; neither may we.
target_compile_options(app PRIVATE -ffp-contract=off -fno-fast-math)

# Run each control step with interrupts locked:
#   west build ... -- -DCTRL_IRQ_LOCK=y
#
# An attribution tool, not a default. The step is measured to take ~3960 cycles
# with a single ~3930-cycle outlier per run; locking interrupts says whether
# that outlier is an ISR landing inside the step or the step itself varying.
if(CTRL_IRQ_LOCK)
  target_compile_definitions(app PRIVATE CTRL_IRQ_LOCK=1)
  message(STATUS "ctrl: control step runs with interrupts locked")
endif()
