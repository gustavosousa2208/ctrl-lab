#!/usr/bin/env python3
"""Grade a two-board closed-loop run against the simulated reference.

    compare-loop.py <loop-reference.f32.csv> <capture> --board <controller.f32.csv>

`capture` is a console capture from the board running the CONTROLLER half. The
reference is a simulation of the whole loop - controller, plant, and an explicit
delay block standing in for the transport - which is the only thing the two-board
run can meaningfully be compared against. See POC-PLAN.md, stage E exit criteria.

Why this is not grade-trace.py: that script compares a device trace against the
simulation of the SAME project, column for column, and reports a digest. Here
the two projects are deliberately different - the board runs half a diagram and
the reference is all of it - so the columns have to be mapped by name and a
bit-for-bit digest is meaningless. What is left is the error, which is the whole
point of the stage: this is where bit-for-bit grading ends and real error begins.

The board's trace frame is positional - it carries no signal names - so
`--board` supplies the controller project's own reference CSV purely for its
header, which IS the board's column order. Naming them from the loop reference
instead would silently line up the wrong pair of signals whenever the two
diagrams differ, which is always: that difference is the point.

The column map defaults to the fixtures this was written for. Override it when
the diagram changes rather than editing the default, so a stale map fails loudly
instead of comparing the wrong pair of signals.
"""

import argparse
import csv
import importlib.util
import math
import pathlib
import sys

# The project's f32 noise floor, from firmware/ctrl/README.md. Anything under it
# is agreement; anything over it is a real disagreement worth chasing.
F32_FLOOR = 5.8e-6

DEFAULT_MAP = "input-2=delay-y,sum-3=sum-3,gain-4=gain-4,step-1=step-1"


def load_grade_trace():
    """Reuse the frame decoder rather than reimplementing it."""
    path = pathlib.Path(__file__).with_name("grade-trace.py")
    spec = importlib.util.spec_from_file_location("grade_trace", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("reference")
    parser.add_argument("capture")
    parser.add_argument("--board", required=True,
                        help="the CONTROLLER project's own f32.csv, for its column order")
    parser.add_argument("--map", default=DEFAULT_MAP,
                        help="comma-separated board_signal=reference_signal pairs")
    parser.add_argument("--quarters", type=int, default=4,
                        help="how many equal slices to report drift over")
    args = parser.parse_args()

    gt = load_grade_trace()
    board = gt.read_binary_frame(pathlib.Path(args.capture).read_bytes())["rows"]

    rows = list(csv.reader(open(args.reference)))
    header, reference = rows[0], [[float(v) for v in r] for r in rows[1:]]
    ref_col = {name: i for i, name in enumerate(header)}

    # The board's frame is positional, so its columns are named by the project
    # it is actually running rather than by the reference it is graded against.
    board_names = next(csv.reader(open(args.board)))

    pairs = []
    for entry in args.map.split(","):
        board_name, _, ref_name = entry.partition("=")
        if board_name not in board_names:
            sys.exit(f"the board project has no signal `{board_name}`; "
                     f"it has {board_names[1:]}")
        if ref_name not in ref_col:
            sys.exit(f"the reference has no signal `{ref_name}`; it has {header[1:]}")
        pairs.append((board_name, ref_name))

    n = min(len(board), len(reference))
    if n == 0:
        sys.exit("no rows in common - did the capture contain a trace frame?")

    print(f"rows        {n} compared "
          f"({len(board)} from the board, {len(reference)} simulated)")
    print(f"floor       {F32_FLOOR:.1e}  (f32 noise floor)\n")
    print(f"{'signal':<22} {'max_abs_error':>14} {'at k':>6} {'rms':>13}   verdict")

    worst = 0.0
    for board_name, ref_name in pairs:
        bcol = board_names.index(board_name)
        rcol = ref_col[ref_name]
        errs = [abs(board[k][bcol] - reference[k][rcol]) for k in range(n)]
        peak = max(errs)
        rms = math.sqrt(sum(e * e for e in errs) / n)
        worst = max(worst, peak)
        label = f"{board_name} -> {ref_name}"
        print(f"{label:<22} {peak:14.3e} {errs.index(peak):6d} {rms:13.3e}   "
              f"{'ABOVE FLOOR' if peak > F32_FLOOR else 'below floor'}")

    # Drift is the thing a steady-state figure hides: two free-running crystals
    # can agree perfectly for a second and separate over a minute.
    first_board, first_ref = pairs[0]
    bcol, rcol = board_names.index(first_board), ref_col[first_ref]
    errs = [abs(board[k][bcol] - reference[k][rcol]) for k in range(n)]
    print(f"\ndrift       max|error| in `{first_board}` per slice of the run")
    for q in range(args.quarters):
        a, b = q * n // args.quarters, (q + 1) * n // args.quarters
        print(f"            k {a:5d}-{b:<5d}  {max(errs[a:b]):.3e}")

    over = sum(1 for e in errs if e > F32_FLOOR)
    print(f"\nabove floor {over} of {n} samples")
    print(f"VERDICT     {'PASS' if worst <= F32_FLOOR else 'FAIL'} - "
          f"worst signal {worst:.3e} against a {F32_FLOOR:.1e} floor")
    return 0 if worst <= F32_FLOOR else 1


if __name__ == "__main__":
    sys.exit(main())
