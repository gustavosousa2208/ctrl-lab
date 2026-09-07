cmake_minimum_required(VERSION 3.20.0)

# Which end of the wire this build is:
#   -DLINK_ROLE=initiator   the F767, which measures
#   -DLINK_ROLE=responder   the H743, which echoes
set(LINK_ROLE "initiator" CACHE STRING "initiator or responder")

# Link speed, in baud. Both ends must match. 1/2/3/4/6 Mbaud are exact on both
# boards; above 6 needs 8x oversampling and will not survive jumper wires.
set(LINK_BAUD "1000000" CACHE STRING "link UART baud rate")

if(BOARD STREQUAL "nucleo_f767zi")
  set(LINK_UART "usart6")
elseif(BOARD STREQUAL "mini_stm32h743")
  set(LINK_UART "usart1")
else()
  message(FATAL_ERROR "link: no link UART known for board '${BOARD}'")
endif()

configure_file(link-baud.overlay.in ${CMAKE_CURRENT_BINARY_DIR}/link-baud.overlay @ONLY)
list(APPEND EXTRA_DTC_OVERLAY_FILE ${CMAKE_CURRENT_BINARY_DIR}/link-baud.overlay)

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(ctrl_lab_link)

if(LINK_ROLE STREQUAL "initiator")
  target_compile_definitions(app PRIVATE LINK_INITIATOR=1)
elseif(LINK_ROLE STREQUAL "responder")
  target_compile_definitions(app PRIVATE LINK_RESPONDER=1)
else()
  message(FATAL_ERROR "link: LINK_ROLE must be initiator or responder")
endif()

message(STATUS "link: ${BOARD} as ${LINK_ROLE}, ${LINK_UART} at ${LINK_BAUD} baud")

target_include_directories(app PRIVATE src ../ctrl/src)
target_sources(app PRIVATE src/main.c ../ctrl/src/dcp.c ../ctrl/src/trace.c)

# Same reason as firmware/ctrl: never let the compiler contract a*b+c into an
# FMA. Nothing here is float-heavy, but the CRC and framing code is shared with
# the control core and the two must stay byte-identical.
target_compile_options(app PRIVATE -ffp-contract=off -fno-fast-math)
