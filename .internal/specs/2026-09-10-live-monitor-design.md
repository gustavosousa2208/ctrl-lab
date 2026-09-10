# Live MCU monitor — design

A separate view in the ctrl-lab UI that streams what a board is doing, as it
does it. Not the simulation scope, not a control surface: **monitoring only.**

## Why this is bigger than it looks

Of the three parts in "make the H7 change data, read it with the F7, show it in
the UI":

| | state |
| --- | --- |
| H7 emits a time-varying signal | a fixture. No firmware work. |
| F7 reads it over the link | **done** — measured at 4.98e-10 on 2026-09-10 |
| it appears in the UI | **nothing exists** |

The frontend has no serial or device code at all, two Tauri commands, and a
scope wired to `simulationTrace` — data from the *simulator*. The firmware is
record-then-dump: it buffers a whole run and emits one binary frame at the end.
`POC-PLAN.md` lists telemetry transport as an unspecified open decision, and
that gap is this feature.

## Decisions

### 1. A separate view, and read-only

It does not feed the simulation scope. That scope plots a *model*; this plots a
*board*, and the two agreeing is a result rather than a rendering detail — the
whole of stage E is about measuring the gap between them. Merging them would
make the most interesting number in the project invisible.

Monitoring only: no arming, no plan upload, no setpoint changes from the UI.
Every one of those is a control path and wants its own argument.

### 2. Stream from `main()`, never from the control thread

The control step runs in a `K_PRIO_COOP(0)` thread and `main()` blocks on
`k_sem_take(&run_complete, K_FOREVER)` while it does. Replace that with a drain
loop: wake every few milliseconds, emit whatever rows the control thread has
appended since, exit when the run completes.

This is structurally safe rather than safe-by-measurement. `main()` is
preemptible and the control thread is cooperative at priority 0, so **main
cannot preempt the step** — no amount of console I/O in the drain loop can
perturb the thing being measured. Putting the emit in the control thread would
put a ~540 us console write inside a 44 us step.

`completed_steps` is the watermark. It needs to be `volatile`: the control
thread writes it and `main()` reads it, and without that the compiler may hoist
the load out of the drain loop.

### 3. The existing text row format is the stream format

`CTRL_TRACE_TEXT` already emits `T,<hex>,<hex>,...` one row per line, and
`grade-trace.py` already parses it. Newline-delimited rows are self-delimiting,
which a stream needs and the binary DCPT frame cannot give: that frame is a
whole-run container with a length and a trailing CRC, so a reader cannot
interpret it until the run is over. That is exactly the property being removed.

The per-run digest and the DCPT frame stay for grading. Streaming is an
addition, behind its own flag, and the graded path is untouched.

### 4. Transport is the console UART, not USB CDC

usart3 at 921600 through the ST-Link VCP. Measured on 2026-09-06 at 0 rows lost
and 0 damaged over five captures, and it is the channel `console.py` already
reads. A row of 6 signals is ~60 bytes, so 20 Hz is ~1.2 kB/s against a
92 kB/s channel.

USB CDC (`ctrl-lab-1pq`) is the better long-term answer and is a separate
subsystem — a device stack on the control path, and a second thing to blame
when a number looks wrong. Not for the first version.

### 5. Tauri reads the port as a plain file

No new crate. `console.py` already establishes that *"a macOS `/dev/cu.*`
device is a plain character file once stty has set the line discipline"*, and
the same is true from Rust. `stty -f <port> 921600 cs8 -cstopb -parenb raw
-echo clocal -crtscts -ixon -ixoff` then `File::open`.

`clocal -crtscts` is not optional and is worth repeating here because getting
it wrong looks exactly like a firmware bug: macOS defaults the port to hardware
flow control, the ST-Link VCP does not drive RTS/CTS, and the kernel throttles
itself and silently drops bytes mid-line.

Windows needs a different path, as it already does for `console.ps1`. Not now.

## Shape

```
  board ──T,hex rows──> /dev/cu.usbmodem* ──> Tauri reader thread
                                                    │  parsed rows
                                                    ▼
                                            Tauri event stream
                                                    │
                                                    ▼
                                          Monitor panel (live plot)
```

## Not in scope

- **Live streaming of a graded run.** Grading needs the digest over the whole
  trace; that path stays record-then-dump and is not touched.
- **Controlling the board from the UI.** See decision 1.
- **Windows.** The reader is macOS/Linux only for now.
- **The PID staircase.** That is fixtures on top of this, and needs no firmware:
  `POC-PLAN.md` says a discrete PID *is* a second-order discrete transfer
  function, and 1-2-3-2-1 composes from four `step` blocks and sums.
