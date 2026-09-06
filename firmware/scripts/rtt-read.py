#!/usr/bin/env python3
"""Read an RTT up-buffer straight out of target memory with J-Link.

    rtt-read.py <elf> [--out FILE] [--device STM32H743VI] [--speed 1000]

JLinkRTTLogger could not find the control block on this setup, with or without
-RTTSearchRanges. It does not need to be found: the ELF says where it is, the
control block says where its buffer is and how much has been written, and
`mem8` reads the bytes. That is all RTT is.

Writes raw bytes, because the trace is a binary DCPT frame with console text
around it. Pipe the result to grade-trace.py, which finds the frame by its magic.
"""

import argparse
import re
import struct
import subprocess
import sys
import tempfile

CB_LAYOUT = "acID[16] MaxNumUp MaxNumDown | aUp[0]: sName pBuffer SizeOfBuffer WrOff RdOff Flags"


def jlink(device, speed, commands):
    with tempfile.NamedTemporaryFile("w", suffix=".jlink", delete=False) as handle:
        handle.write(f"si SWD\nspeed {speed}\ndevice {device}\nconnect\n")
        handle.write("\n".join(commands) + "\nq\n")
        path = handle.name
    result = subprocess.run(["JLinkExe", "-nogui", "1", "-CommandFile", path],
                            capture_output=True, text=True)
    if "Cannot connect to target" in result.stdout:
        sys.exit("J-Link could not attach. If the board idled into WFI the core "
                 "drops off the debug bus; hold BOOT0 and power-cycle, then retry.")
    return result.stdout


def read_bytes(device, speed, addr, length):
    """mem8 in chunks - J-Link truncates very large single reads."""
    data = bytearray()
    step = 0x400
    for offset in range(0, length, step):
        n = min(step, length - offset)
        out = jlink(device, speed, [f"mem8 0x{addr + offset:08X} 0x{n:X}"])
        for line in out.splitlines():
            m = re.match(r"^[0-9A-F]{8} = (.*)$", line.strip())
            if not m:
                continue
            for token in m.group(1).split():
                if len(token) == 2:
                    data.append(int(token, 16))
    return bytes(data[:length])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("elf")
    parser.add_argument("--out", default=None)
    parser.add_argument("--device", default="STM32H743VI")
    parser.add_argument("--speed", default="1000")
    parser.add_argument("--nm", default=None, help="path to arm-zephyr-eabi-nm")
    args = parser.parse_args()

    nm = args.nm or "arm-zephyr-eabi-nm"
    symbols = subprocess.run([nm, args.elf], capture_output=True, text=True).stdout
    cb = next((int(l.split()[0], 16) for l in symbols.splitlines()
               if l.split()[-1] == "_SEGGER_RTT"), None)
    if cb is None:
        sys.exit("no _SEGGER_RTT symbol in the ELF - is CONFIG_USE_SEGGER_RTT set?")

    header = read_bytes(args.device, args.speed, cb, 0x30)
    if header[:10] != b"SEGGER RTT":
        sys.exit(f"no RTT control block at 0x{cb:08X} (read {header[:10]!r}). "
                 "If the magic is present but the counts are zero, the D-cache is "
                 "hiding it: set CONFIG_SEGGER_RTT_SECTION_DTCM=y.")

    max_up, _, _, buf, size, wroff = struct.unpack_from("<IIIIII", header, 0x10)
    print(f"CB 0x{cb:08X}  up-buffers {max_up}  buffer 0x{buf:08X}  "
          f"size {size}  written {wroff}", file=sys.stderr)
    if wroff == 0:
        sys.exit("buffer is empty - the target has printed nothing yet")
    if wroff > size:
        sys.exit(f"WrOff {wroff} exceeds buffer size {size}; refusing to read")

    payload = read_bytes(args.device, args.speed, buf, wroff)
    if args.out:
        open(args.out, "wb").write(payload)
        print(f"wrote {len(payload)} bytes to {args.out}", file=sys.stderr)
    else:
        sys.stdout.buffer.write(payload)


if __name__ == "__main__":
    main()
