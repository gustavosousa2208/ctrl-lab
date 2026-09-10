# Input/Output blocks and io_bindings — design

Bead `ctrl-lab-ri0`. The last feature between here and a closed two-board loop:
a plan has no way to say "this signal comes off the link."

The DCP format has reserved the section since v1. `build_control_plan` emits it
empty, and `ctrl_plan_load` refuses a non-empty one rather than pretending to
bind channels it has no HAL for (`dcp.h:73`, `dcp.c:250`). This is the change
that makes it mean something.

## What exists already

| | state |
| --- | --- |
| `IoBinding { block_index, channel_role, channel_index }` | in `plan.rs:116`, encoded and decoded, round-trip tested |
| `channel_role` | **no assigned meaning anywhere** — a reserved `u16` |
| firmware `struct ctrl_plan` | no io_bindings field; loader rejects non-empty |
| firmware HAL | does not exist |
| kernels | 10, ids 1..10, `KERNEL_SET_VERSION = 1` |

So the container is ready and everything it would carry is missing.

## Decisions

### 1. One generic pair of kernels, not one pair per peripheral

`Input = 11` and `Output = 12`, appended. `KERNEL_SET_VERSION` goes 1 → 2.

The alternative — `LinkIn`/`LinkOut` now, `AdcIn`/`DacOut` at stage F — is
tempting and wrong: it puts the peripheral class in the kernel id, which is
wire-permanent, when the format already has a field for exactly that
(`channel_role`). One pair of kernels plus a role keeps stage F to a new role
constant rather than two new kernels and another version bump.

This also matches what `POC-PLAN.md` asks for in as many words: *"an `Input`
block (no inputs, one output, reads a named channel) and an `Output` block (one
input, no output, writes a named channel)"*.

### 2. `channel_role` gets a small append-only enum

```
1  LINK    the MCU-to-MCU serial link (this stage)
2  ADC     reserved, stage F
3  DAC     reserved, stage F
4  GPIO    reserved
```

Only `LINK` is implemented. The others are written down so the numbering is
settled before two people pick different values for the same thing; a loader
that meets an unimplemented role rejects the plan by name rather than by
falling through.

### 3. `Input` is a source. That is the load-bearing part.

`is_direct_feedthrough: false`, like `constant` and `step`.

Not a detail. In the combined diagram the controller and plant form a loop, and
the only reason it is not an *algebraic* loop is that something in it does not
pass its input straight to its output. On hardware that something is the link.
In the graph it has to be the `Input` block, or validation will report a cycle
in a diagram that runs perfectly well on two boards.

### 4. In simulation, `Input` reads a constant and `Output` discards

`Input` has one parameter, `default` (default `0.0`), and that is its output
value in any PC simulation. `Output` is a unity passthrough, like `Scope`.

The temptation is to make simulation model the link — loop an `Output` back to
the `Input` of the same channel through a one-sample delay, so a split project
simulates as a closed loop. Rejected, because it would make the split projects
*look* like a numerical reference, and they are not one. `POC-PLAN.md` is
explicit about where the reference lives:

> **Exit criteria:** the two-MCU trace matches a stage-C simulation *of the
> delay-augmented diagram*.

The reference is the **combined** diagram — controller and plant in one graph,
with an explicit `delay` block standing in for the transport. `controller.json`
and `plant.json` are deployment artifacts that happen to be simulable, and what
they simulate to is not meaningful. Making that obvious is worth more than
making it convenient.

### 5. The host harness gets the same stub, so bit-exactness survives

`kernels.c` is compiled both into the firmware and into `firmware/ctrl/host/`,
which is what grades traces against the f32 reference. The host has no link, so
`ctrl_io_read` there returns the block's `default` — the same value `exec.rs`
produces for the same block.

That is deliberate: it means a plan containing `Input`/`Output` still grades
bit-for-bit on the host against the backend, and only *diverges by design* when
a real HAL is bound underneath it on a board.

### 6. The control core never sees a peripheral

A new `ctrl_io.h`:

```c
bool ctrl_io_read(uint16_t role, uint16_t index, float *out);
bool ctrl_io_write(uint16_t role, uint16_t index, float value);
```

`kernels.c` calls only these. The link implementation lives beside
`firmware/link/`, the host implementation beside the harness. This is the
boundary the bead calls out: *"the control core knows nothing about peripherals
— only the HAL does."*

### 7. What the loader must check

A binding is refused unless: `block_index` is in range; the block at that index
is `Input` or `Output`; the role is implemented; the direction matches (an
`Input` may not bind a write-only role); and no two bindings name the same
block. An `Input` block with **no** binding is also an error — it would read
whatever the HAL's default is and look like it worked.

## Not in scope, deliberately

- **Graph partitioning.** `POC-PLAN.md`: *"Do not build graph partitioning yet.
  It is a large feature and it is not what this PoC is testing."* The two
  projects are hand-authored.
- **Named channels resolved at load time.** `POC-PLAN.md` says "named"; the
  format carries a `u16` index. Names are a frontend and backend concern that
  can be resolved to indices at compile time; the wire stays numeric.
- **Plan delivery over the link.** Inbound and one-shot, versus telemetry which
  is outbound and continuous. The bead warns against coupling their lifecycles
  and this change touches neither: plans are still linked in at build time.

## The risk this feature walks into

`POC-PLAN.md` guarantees one sample of transport delay by construction, and the
construction it assumes is **SPI, full-duplex** — A transmits `u[k]` and
simultaneously receives `y[k-1]`, with no dependence on baud rate or interrupt
latency. The link that got built is UART, and it has no such symmetry.

From the E3 measurements (`firmware/link/README.md`), one way — control code
decides to send, to control code able to read it — is roughly:

```
  25 689 ns  arming the transmit DMA
  53 333 ns  32 bytes on the wire at 6 Mbaud
  ~10 000 ns  far side's receive interrupt and buffer swap
  ---------
  ~89 000 ns  against a 100 000 ns tick
```

Eleven microseconds of margin, against 5183 ns of tick jitter on the H743 and a
13–19 us control step. So the delay is *usually* one sample and not *provably*
one, which is the worst place to be: the mismatch would be intermittent and
would look like a firmware bug, which is precisely the week `POC-PLAN.md` warns
about losing.

This does not block the io blocks — none of the design above depends on it —
but it should be settled before `ctrl-lab-b7r.4` closes the loop. The options,
in the order they cost:

1. **Halve the tick for the two-board run** (5 kHz, 200 us). 89 us is then
   comfortably one sample and the finding still stands. Free.
2. **Land `ctrl-lab-b7r.8` first** (cyclic DMA), which removes ~39 us of stream
   reprogramming per packet and takes one way to ~50 us. Real work, and the
   right answer eventually.
3. **Stamp and check.** The packet already carries `seq` and `tx_tick`; the
   consumer can assert the delay was exactly one tick and count when it was
   not, turning an intermittent numerical mismatch into a counter. Cheap, and
   worth doing regardless of 1 or 2.
