#!/usr/bin/env python3
"""Reset the board and capture its console. The macOS/Linux companion to console.ps1.

    console.py [--port /dev/cu.usbmodemXXXX] [--out run.txt]
               [--until done] [--timeout 30]

Resets first, then reads, because the runtime prints everything it has to say in
the first second after boot: open the port too late and the trace is gone. The
port is opened BEFORE the reset is issued for the same reason.

No pyserial. A macOS /dev/cu.* device is a plain character file once stty has set
the line discipline, and stty is already there - one less thing to install on a
machine that just wants to read a board.
"""

import argparse
import glob
import os
import select
import subprocess
import sys
import time

CUBE = "STM32_Programmer_CLI"


def find_port():
    # cu.* rather than tty.*: cu does not block waiting for carrier detect.
    ports = sorted(glob.glob("/dev/cu.usbmodem*")) or sorted(glob.glob("/dev/ttyACM*"))
    if not ports:
        sys.exit("no ST-Link serial port found (looked for /dev/cu.usbmodem*, /dev/ttyACM*)")
    return ports[0]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", default=None)
    parser.add_argument("--out", default=None, help="also write the capture here")
    # Matches firmware/ctrl's CTRL_CONSOLE_BAUD default. The bringup probe still
    # runs at the board default, so read it with --baud 115200.
    parser.add_argument("--baud", type=int, default=921600)
    parser.add_argument("--until", default="done", help="stop once this line appears")
    parser.add_argument("--timeout", type=float, default=30.0)
    parser.add_argument("--no-reset", action="store_true")
    args = parser.parse_args()

    port = args.port or find_port()
    # `clocal -crtscts` is not optional, and getting it wrong looks like a
    # firmware bug. macOS defaults this port to hardware flow control, and the
    # ST-Link VCP does not drive RTS/CTS - so the kernel throttles itself and
    # silently drops bytes mid-line. Measured on hardware 2026-09-06: without
    # these two flags a 501-row trace arrives with one to three mangled rows, in
    # different places every run; with them, 501 rows and zero. -ixon/-ixoff for
    # the same reason, so no data byte is ever read as a flow-control character.
    # macOS accepts only the standard rate ladder on this driver: 115200,
    # 230400, 460800, 921600. 500000, 1000000 and anything above 921600 come
    # back as `tcsetattr: Invalid argument`. Measured 2026-09-06 - so 921600 is
    # the ceiling for the ST-Link VCP path, and a faster link means USB, not a
    # bigger number here.
    stty = subprocess.run(
        ["stty", "-f", port, str(args.baud), "cs8", "-cstopb", "-parenb", "raw", "-echo",
         "clocal", "-crtscts", "-ixon", "-ixoff"],
        capture_output=True,
        text=True,
    )
    if stty.returncode != 0:
        sys.exit(
            f"stty rejected {args.baud} baud on {port}: {stty.stderr.strip()}\n"
            f"macOS supports 115200, 230400, 460800, 921600 on this port."
        )

    # Open before resetting, so nothing printed at boot is lost.
    fd = os.open(port, os.O_RDONLY | os.O_NONBLOCK)

    # Then throw away whatever is already queued.
    #
    # This is not tidiness, it is correctness. Flashing resets the board, so by
    # the time this script runs the device has usually already booted and dumped
    # an entire trace into the OS receive buffer. Without this drain the read
    # loop below returns that stale trace, sees its `done`, and exits - reporting
    # a capture that is faster than the line rate and sometimes has MORE rows
    # than the run has steps, because it spans two boots. Both symptoms were
    # observed before this was added.
    drained = 0
    while True:
        ready, _, _ = select.select([fd], [], [], 0.3)
        if not ready:
            break
        try:
            chunk = os.read(fd, 65536)
        except BlockingIOError:
            break
        if not chunk:
            break
        drained += len(chunk)
    if drained:
        print(f"[drained {drained} stale bytes before reset]", file=sys.stderr)

    if not args.no_reset:
        subprocess.run(
            [CUBE, "-c", "port=SWD", "mode=UR", "-rst"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )

    # select() rather than poll-and-sleep. The ST-Link VCP buffers very little,
    # and a trace is ~30 KB arriving back to back at 115200 - a reader that naps
    # between reads loses characters, which looks exactly like a firmware bug.
    deadline = time.time() + args.timeout
    chunks = []
    seen = 0
    needle = args.until.encode()
    while time.time() < deadline:
        ready, _, _ = select.select([fd], [], [], 0.2)
        if not ready:
            continue
        try:
            data = os.read(fd, 65536)
        except BlockingIOError:
            continue
        if not data:
            continue
        chunks.append(data)
        # Scan only the new tail, so the check stays O(1) per read rather than
        # rescanning a growing buffer.
        if needle in b"".join(chunks[seen:]):
            break
        seen = max(0, len(chunks) - 2)
    os.close(fd)

    text = b"".join(chunks).decode("utf-8", errors="replace")
    sys.stdout.write(text)
    if args.out:
        with open(args.out, "w") as handle:
            handle.write(text)
        print(f"\n[captured {len(text)} bytes to {args.out}]", file=sys.stderr)

    if args.until not in text:
        print(f"\n[warning: never saw `{args.until}` - timed out]", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
