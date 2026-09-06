#!/usr/bin/env python3
"""Grade a control-runtime trace against the committed f32 reference.

    grade-trace.py <reference.f32.csv> [trace]     # trace defaults to stdin

`trace` is the output of either harness - the device over the console, or
firmware/ctrl/host/ctrl-host - which emit the same format: `T,<hex>,...` rows of
raw little-endian f32 bit patterns, then a `trace_fnv1a64=` digest.

Two different claims come out of this, and they are not interchangeable:

  * max |device - reference|, judged against the project's 5.8e-6 f32 noise
    floor. Anything above that is a bug, not precision loss.
  * the digest, which is the real bit-for-bit check.

The reference CSV cannot settle bit-for-bit on its own. It stores 9 decimal
places, which is not always enough to round-trip an f32 - a value of 1e-8 prints
as 0.000000010 and comes back as something else entirely. That costs nothing for
the error bound, since such values are tiny in absolute terms, but it does mean
an exact comparison against this file would report false mismatches. Compare the
digest against `ctrl-backend --trace-hash` for that.
"""

import struct
import sys

# backend/src/exec.rs: worst f32-vs-f64 divergence across all four fixtures.
NOISE_FLOOR = 5.8e-6


def bits_to_float(token):
    return struct.unpack("<f", struct.pack("<I", int(token, 16)))[0]


def read_trace(stream):
    rows, digest = [], None
    for line in stream:
        line = line.strip()
        if line.startswith("T,"):
            rows.append([bits_to_float(t) for t in line[2:].split(",")])
        elif line.startswith("trace_fnv1a64="):
            digest = line.split("=", 1)[1]
    return rows, digest


def read_reference(path):
    with open(path) as handle:
        header = handle.readline().strip().split(",")
        rows = [[float(v) for v in line.strip().split(",")] for line in handle if line.strip()]
    return header, rows


def main():
    if len(sys.argv) not in (2, 3):
        sys.exit(__doc__)

    header, reference = read_reference(sys.argv[1])
    with open(sys.argv[2]) if len(sys.argv) == 3 else sys.stdin as stream:
        trace, digest = read_trace(stream)

    if not trace:
        sys.exit("no trace rows found (expected lines starting with `T,`)")

    if len(trace) != len(reference):
        print(f"step count differs: trace {len(trace)}, reference {len(reference)}")
    if len(trace[0]) != len(reference[0]):
        sys.exit(f"signal count differs: trace {len(trace[0])}, reference {len(reference[0])}")

    worst, worst_at = 0.0, None
    for k in range(min(len(trace), len(reference))):
        for column, (got, want) in enumerate(zip(trace[k], reference[k])):
            delta = abs(got - want)
            if delta > worst:
                worst, worst_at = delta, (k, column)

    print(f"steps      {len(trace)}")
    print(f"signals    {len(trace[0]) - 1} (plus t)")
    if digest:
        print(f"digest     {digest}")

    if worst_at is None:
        print("max error  0 (identical)")
    else:
        step, column = worst_at
        name = header[column] if column < len(header) else f"col{column}"
        print(f"max error  {worst:.3e} at step {step}, signal `{name}`")

    print(f"floor      {NOISE_FLOOR:.1e}")
    ok = worst <= NOISE_FLOOR
    print("VERDICT    " + ("PASS" if ok else "FAIL - above the f32 noise floor, this is a bug"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
