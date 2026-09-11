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

# The link's devicetree, when -DCTRL_LINK=y asks for it. Above find_package()
# for the same reason the console overlay is: Zephyr reads the devicetree in
# there, so anything appended afterwards is appended too late and the only
# symptom is a missing alias much further down the build.
#
# INCLUDED from firmware/link rather than copied. The DMA stream numbers are a
# settled board fact and a second copy is a second place to get them wrong; the
# DTCM selection stays duplicated in boards/ because that really is one line.
if(CTRL_LINK)
  if(BOARD STREQUAL "nucleo_f767zi")
    set(LINK_UART "usart6")
  elseif(BOARD STREQUAL "mini_stm32h743")
    set(LINK_UART "usart1")
  else()
    message(FATAL_ERROR "ctrl: no link UART known for board '${BOARD}'")
  endif()

  # Applied here rather than left to the caller. Without it the build still
  # succeeds and CONFIG_UART_ASYNC_API is simply off, so uart_callback_set
  # fails at runtime on a board that looks correctly configured - which is the
  # kind of quiet wrong that costs an afternoon. ctrl_io_link.c refuses to
  # compile without it as the second half of the same guard.
  list(APPEND EXTRA_CONF_FILE ${CMAKE_CURRENT_LIST_DIR}/link.conf)

  set(CTRL_LINK_BAUD "6000000" CACHE STRING "link UART baud rate")
  set(LINK_BAUD ${CTRL_LINK_BAUD})
  configure_file(../link/link-baud.overlay.in
                 ${CMAKE_CURRENT_BINARY_DIR}/link-baud.overlay @ONLY)
  list(APPEND EXTRA_DTC_OVERLAY_FILE
       ${CMAKE_CURRENT_LIST_DIR}/../link/boards/${BOARD}-link.dtsi
       ${CMAKE_CURRENT_BINARY_DIR}/link-baud.overlay)
endif()

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

# Bind the control core's Input/Output blocks to the MCU-to-MCU link:
#   west build ... -- -DCTRL_LINK=y -DCTRL_LINK_BAUD=6000000
#
# Off by default, and the default is not laziness. Without it an Input block
# reads the `default` its parameter declares, which is exactly what
# backend/src/exec.rs evaluates it to - so a plan carrying io blocks still
# grades bit-for-bit against the reference on a board with nothing wired to it.
# Turning this on is what makes the board's answer depend on another board.
if(CTRL_LINK)
  target_include_directories(app PRIVATE ../link/src)
  target_sources(app PRIVATE
    src/ctrl_io_link.c
    ../link/src/link_dma.c
    ../link/src/link_frame.c
  )
  target_compile_definitions(app PRIVATE CTRL_LINK=1)
  message(STATUS "ctrl: link bound on ${LINK_UART} at ${CTRL_LINK_BAUD} baud")
else()
  target_sources(app PRIVATE src/ctrl_io_none.c)
  message(STATUS "ctrl: no channels bound; Input blocks read their defaults")
endif()

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
# Emit the trace as hex text instead of a binary frame:
#   west build ... -- -DCTRL_TRACE_TEXT=y
#
# The frame is 2.3x smaller on the wire (14 KB vs 33 KB for a 501-step run) and
# is the format the next transports will carry. Text stays available because it
# is readable in a terminal without a decoder, which matters when the question
# is "is the board saying anything at all".
if(CTRL_TRACE_TEXT)
  target_compile_definitions(app PRIVATE CTRL_TRACE_TEXT=1)
  message(STATUS "ctrl: trace emitted as hex text")
endif()

# Run steps back to back instead of on the plan's tick:
#   west build ... -- -DCTRL_FREE_RUN=y
#
# Every measurement taken before the timer existed was in this mode, and it is
# still the quick way to check the numbers: fixture 04 at its real 50 ms tick
# takes 25 s to run, against under a second free-running. It is not a control
# loop and does not pretend to be.
# Override the scheduling period, in nanoseconds:
#   west build ... -- -DCTRL_TICK_NS=10000
#
# Drives the loop at a rate other than the plan's own, which is how the
# missed-deadline path gets exercised: set a period shorter than a step and the
# overrun counter has to notice. The model's arithmetic is untouched, so the
# trace must still match the reference bit-for-bit.
if(CTRL_TICK_NS)
  target_compile_definitions(app PRIVATE CTRL_TICK_NS=${CTRL_TICK_NS}ULL)
  message(STATUS "ctrl: tick overridden to ${CTRL_TICK_NS} ns")
endif()

if(CTRL_FREE_RUN)
  target_compile_definitions(app PRIVATE CTRL_FREE_RUN=1)
  message(STATUS "ctrl: free-running, no timer")
endif()

# Stream the trace over the console as it is produced, instead of only dumping
# it at the end:
#   west build ... -- -DCTRL_STREAM=y [-DCTRL_STREAM_POLL_MS=5]
#
# The rows are the same ones the record-then-dump path emits afterwards - same
# format, same values - so the graded digest and the DCPT frame are unchanged.
# What changes is when they leave, which is what makes a live monitor possible.
#
# The emit happens in main(), never in the control thread. main is preemptible
# and the control thread is K_PRIO_COOP(0), so a console write here structurally
# cannot land inside a step. See .internal/specs/2026-09-10-live-monitor-design.md.
if(CTRL_STREAM)
  if(NOT CTRL_STREAM_POLL_MS)
    set(CTRL_STREAM_POLL_MS 5)
  endif()
  target_compile_definitions(app PRIVATE
    CTRL_STREAM=1 CTRL_STREAM_POLL_MS=${CTRL_STREAM_POLL_MS})
  message(STATUS "ctrl: streaming trace rows, draining every ${CTRL_STREAM_POLL_MS} ms")
endif()

if(CTRL_IRQ_LOCK)
  target_compile_definitions(app PRIVATE CTRL_IRQ_LOCK=1)
  message(STATUS "ctrl: control step runs with interrupts locked")
endif()
