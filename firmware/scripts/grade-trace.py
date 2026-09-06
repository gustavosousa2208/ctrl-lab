#!/usr/bin/env python3
"""Grade a control-runtime trace against the committed f32 reference.

    grade-trace.py <reference.f32.csv> [trace]     # trace defaults to stdin

`trace` is the output of either harness - the device over the console, or
firmware/ctrl/host/ctrl-host - which emit the same format: `T,<hex>,...` rows of
raw little-endian f32 bit patterns, then a `trace_fnv1a64=` digest.

Two different claims come out of this, and they are not interchangeable:

  * the digest, which is the real bit-for-bit check and the verdict this script
    reports. Pass --expect-digest with the value from `ctrl-backend
    --trace-hash` to have it checked here.
  * max |device - reference| over the rows that arrived, judged against the
    project's 5.8e-6 f32 noise floor. This locates a disagreement; it cannot
    prove agreement.

The reference CSV cannot settle bit-for-bit on its own. It stores 9 decimal
places, which is not always enough to round-trip an f32 - a value of 1e-8 prints
as 0.000000010 and comes back as something else entirely.

Neither can the console. Measured on hardware 2026-09-06: a 501-row trace off
the NUCLEO-F767ZI arrives with zero to three rows mangled by dropped bytes, in
different places every run, and occasionally one row short. That is the ST-Link
VCP, not the firmware - the digest is computed on-device from memory, before any
of it is printed, and it has been correct on every run including the corrupted
ones. So rows are matched to the reference by their time value rather than by
position (a dropped row must not shift everything after it), damaged rows are
counted and skipped, and the digest decides the verdict.
"""

import argparse
import io
import struct
import sys
import zlib

# backend/src/exec.rs: worst f32-vs-f64 divergence across all four fixtures.
NOISE_FLOOR = 5.8e-6


def bits_to_float(token):
    return struct.unpack("<f", struct.pack("<I", int(token, 16)))[0]


TRACE_MAGIC = b"DCPT"
TRACE_HEADER_LEN = 32
TRACE_FRAME_VERSION = 1


def read_binary_frame(raw):
    """Decode a DCPT trace frame out of a raw capture, or None if absent.

    The frame is located by scanning for its magic and then consuming exactly
    `payload_len` bytes, so it can sit in the middle of ordinary console text
    without any escaping. `payload_len` is only trusted after `header_crc32`
    checks out - a corrupt length is the one field worth validating before it is
    used to slice.

    See firmware/ctrl/src/trace.h for the layout.
    """
    at = raw.find(TRACE_MAGIC)
    while at >= 0:
        header = raw[at:at + TRACE_HEADER_LEN]
        if len(header) < TRACE_HEADER_LEN:
            return None
        version, signal_count = struct.unpack_from("<HH", header, 4)
        step_count, plan_id, payload_len, header_crc = struct.unpack_from("<IQII", header, 8)
        if version == TRACE_FRAME_VERSION and zlib.crc32(header[:24]) & 0xFFFFFFFF == header_crc:
            break
        # A false positive on the magic - the bytes "DCPT" can occur inside a
        # payload. Keep looking.
        at = raw.find(TRACE_MAGIC, at + 1)
    else:
        return None
    if at < 0:
        return None

    body = raw[at + TRACE_HEADER_LEN:]
    if len(body) < payload_len + 12:
        return {"error": f"frame truncated: need {payload_len + 12} bytes, have {len(body)}"}

    payload = body[:payload_len]
    payload_crc, digest = struct.unpack_from("<IQ", body, payload_len)
    if zlib.crc32(payload) & 0xFFFFFFFF != payload_crc:
        return {"error": "frame payload CRC mismatch"}

    stride = 1 + signal_count
    if payload_len != step_count * stride * 4:
        return {"error": "frame payload_len disagrees with step/signal counts"}

    values = struct.unpack(f"<{step_count * stride}f", payload)
    rows = [list(values[k * stride:(k + 1) * stride]) for k in range(step_count)]
    return {"rows": rows, "digest": f"0x{digest:016x}", "plan_id": f"0x{plan_id:016x}"}


def read_trace(stream, width):
    """Rows that parse cleanly, plus the digest and the count of damaged rows.

    `width` is the expected field count including t. A row that does not have
    exactly that many 8-digit hex fields lost bytes in transport and is dropped
    rather than compared - comparing it would report a firmware bug that is
    really a serial-line bug.
    """
    rows, digest, damaged = [], None, 0
    for line in stream:
        line = line.strip()
        if line.startswith("T,"):
            fields = line[2:].split(",")
            if len(fields) != width or not all(len(f) == 8 for f in fields):
                damaged += 1
                continue
            try:
                rows.append([bits_to_float(f) for f in fields])
            except ValueError:
                damaged += 1
        elif line.startswith("trace_fnv1a64="):
            digest = line.split("=", 1)[1]
    return rows, digest, damaged


def read_reference(path):
    with open(path) as handle:
        header = handle.readline().strip().split(",")
        rows = [[float(v) for v in line.strip().split(",")] for line in handle if line.strip()]
    return header, rows


def main():
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("reference", help="test-projects/NN-*.f32.csv")
    parser.add_argument("trace", nargs="?", help="device or host capture (default: stdin)")
    parser.add_argument(
        "--expect-digest", help="value from `ctrl-backend --trace-hash`; decides the verdict"
    )
    args = parser.parse_args()

    header, reference = read_reference(args.reference)
    width = len(reference[0])

    raw = open(args.trace, "rb").read() if args.trace else sys.stdin.buffer.read()

    # A binary frame if there is one, hex text otherwise. Both harnesses can
    # emit either, and a capture may contain text around the frame.
    frame = read_binary_frame(raw)
    if frame and "error" in frame:
        sys.exit(f"binary trace frame: {frame['error']}")

    if frame:
        trace, digest, damaged = frame["rows"], frame["digest"], 0
        source = "binary frame"
    else:
        text = raw.decode("utf-8", errors="replace")
        trace, digest, damaged = read_trace(io.StringIO(text), width)
        source = "hex text"

    if not trace:
        sys.exit("no trace found (looked for a DCPT binary frame and for `T,` hex rows)")

    # Match by time, not position: a row lost in transport must not shift every
    # comparison after it by one step.
    by_time = {round(row[0], 6): (k, row) for k, row in enumerate(reference)}

    worst, worst_at, unmatched = 0.0, None, 0
    for row in trace:
        found = by_time.get(round(row[0], 6))
        if found is None:
            unmatched += 1
            continue
        step, want_row = found
        for column, (got, want) in enumerate(zip(row, want_row)):
            delta = abs(got - want)
            if delta > worst:
                worst, worst_at = delta, (step, column)

    compared = len(trace) - unmatched
    print(f"format     {source}")
    print(f"rows       {compared} of {len(reference)} compared", end="")
    if damaged or unmatched:
        print(f"  ({damaged} damaged in transport, {unmatched} unmatched)")
    else:
        print()
    print(f"signals    {width - 1} (plus t)")

    if worst_at is None:
        print("max error  n/a (nothing compared)")
    else:
        step, column = worst_at
        name = header[column] if column < len(header) else f"col{column}"
        print(f"max error  {worst:.3e} at step {step}, signal `{name}`  (floor {NOISE_FLOOR:.1e})")

    numeric_ok = worst_at is not None and worst <= NOISE_FLOOR

    if digest:
        print(f"digest     {digest}", end="")
        if args.expect_digest:
            match = digest.lower() == args.expect_digest.lower()
            print("  == expected" if match else f"  != expected {args.expect_digest}")
            print("VERDICT    " + ("PASS - bit-for-bit" if match else "FAIL - digest mismatch"))
            return 0 if match else 1
        print("  (pass --expect-digest to check it)")

    print("VERDICT    " + ("PASS" if numeric_ok else "FAIL - above the f32 noise floor"))
    return 0 if numeric_ok else 1


if __name__ == "__main__":
    sys.exit(main())
