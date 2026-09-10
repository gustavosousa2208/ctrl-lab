# Link probe — stage E2

Moves bytes between the two boards and measures what they cost. No control loop,
no plan execution: the only unknown this stage adds is the transport.

## Wiring

```
F767 D1 (PG14, usart6 TX)  ─────────►  H743 PA10 (usart1 RX)
F767 D0 (PG9,  usart6 RX)  ◄─────────  H743 PA9  (usart1 TX)
F767 GND                   ─────────   H743 GND
```

Both link UARTs are on **APB2**, the fastest peripheral bus on each part —
108 MHz on the F767, 120 MHz on the H743. On the F767 this pair is the Arduino
header's `D0`/`D1`, because `arduino_serial: &usart6`, and the board already
enables it. On the H743 `usart1` is enabled by
[`boards/mini_stm32h743.overlay`](boards/mini_stm32h743.overlay); PB6/PB7 would
also carry `usart1` but PB6 is QuadSPI NCS for the onboard NOR flash.

`gcd(108, 120) = 12`, so **1/2/3/4/6 Mbaud are exact on both ends** — zero baud
error rather than merely small.

## Results

Measured 2026-09-06, 1000 pings and one 4044-byte DCPT frame per rate. **[V]**

| baud | RTT mean | one-way | frame | throughput | % of wire | CRC |
| --- | --- | --- | --- | --- | --- | --- |
| 1 M | 23.27 us | ~11.6 us | 41.2 ms | 98 KB/s | 98% | OK |
| 2 M | 12.50 us | ~6.3 us | 21.0 ms | 192 KB/s | 96% | OK |
| 3 M | 9.43 us | ~4.7 us | 14.2 ms | 284 KB/s | 95% | OK |
| 4 M | 7.88 us | ~3.9 us | 10.8 ms | 372 KB/s | 93% | OK |
| **6 M** | **6.34 us** | **~3.2 us** | **7.5 ms** | **541 KB/s** | 90% | OK |

Zero pings lost at every rate. RTT tracks byte time exactly with a consistent
**2.5–3.5 us of software** across both ends combined, which is the figure stage E
adds to the error budget.

The round trip is timed on one board's clock deliberately: two boards with no
shared timebase cannot measure a one-way delay directly, so the far side reports
its own turnaround for subtraction. Note `uart_poll_out` returns when the byte
reaches the transmit register rather than the wire, so the last character time
per hop sits outside the measurement — these are a lower bound on wire time and
an accurate measure of software-to-software latency.

## Interrupts must be off during a frame

This is the finding, and it took the instrumentation to see.

A polled receive on a UART with a one-byte register cannot tolerate being
preempted: at 1 Mbaud a byte arrives every 10 us, and the H743's 10 kHz kernel
tick ISR is both expensive and variable — the same property the E1 jitter work
measured as 5183 ns of tick jitter. The result was sporadic mid-stream byte
loss, one to three bytes per frame, which looked like a rate problem and was not.

With `irq_lock()` around the frame receive: **five of five runs CRC OK, 41197 us
each, repeatable to the microsecond.** Without it, roughly one run in four
succeeded.

A ~40 ms interrupt lock is fine for a transport probe with nothing else to be
late for, and it is **not** what stage E3 should do — a control loop cannot go
deaf for 40 ms. The real answer there is DMA receive or the H7's USART FIFO,
which take the deadline off the CPU entirely. This build proves the wire and the
framing are sound; it does not propose the mechanism.

## The DMA receive path, and where its buffers are allowed to live

Stage E3 answers the question the section above leaves open: **DMA receive**,
through Zephyr's `CONFIG_UART_ASYNC_API`. Both link UARTs now carry `dmas`
bindings, and the two boards spell the same thing differently.

| | controller | tx | rx |
| --- | --- | --- | --- |
| F767 `usart6` | DMA2, hardwired | stream 6, channel 5 | stream 1, channel 5 |
| H743 `usart1` | DMA1 via DMAMUX1 | channel 0, request 42 | channel 1, request 41 |

On the F767 the stream *is* the request: RM0410 table 28 fixes USART6_TX to
DMA2 streams 6 and 7 and USART6_RX to streams 1 and 2, so the only free choice
is which of each pair. On the H743 the DMAMUX breaks that link — the channel is
a free choice and the request ID (RM0433 table 121) is the fixed part.

**The buffers must be in non-cacheable memory, and the driver enforces it.**
Both parts are Cortex-M7 with D-cache on by default, and `uart_stm32.c` does no
cache maintenance of its own. Instead it checks: `uart_tx`, `uart_rx_enable`
and `uart_rx_buf_rsp` each call `stm32_buf_in_nocache()` and return `-EFAULT`
with *"buffer should be placed in a nocache memory region"* if the buffer is
not there. This applies to the **transmit** buffer as well, so a packet built
on the stack cannot be handed to `uart_tx` — it has to be staged.

Upstream's own `samples/drivers/uart/async_api` sidesteps all of this by
setting `CONFIG_DCACHE=n` on both `nucleo_f746zg` and `nucleo_h753zi`, which
works because `stm32_buf_in_nocache()` is a static inline returning `true` when
D-cache is off. That is the wrong trade here — `firmware/ctrl/README.md`
measured what the D-cache buys on the H743 — so the link uses
`CONFIG_NOCACHE_MEMORY=y` with `CONFIG_ARM_MPU=y` and puts only the ring in the
MPU-backed `nocache` section. The rest of RAM stays cacheable.

Where that section lands, from the linker maps:

| board | `_nocache_ram_start` | what it is |
| --- | --- | --- |
| `nucleo_f767zi` | `0x20020000` | start of SRAM1 |
| `mini_stm32h743` | `0x24000000` | start of AXI SRAM (`sram0`) |

Both are reachable by the DMA controller, which is not automatic on the H743:
DMA1 sits in the D2 domain and cannot see the D1 core-coupled memories at all.
It reaches AXI SRAM through the D2-to-D1 bus matrix, but a buffer in `dtcm`
would simply never be written. That is the exact opposite of the rule for the
control pools in [`firmware/BRINGUP.md`](../BRINGUP.md), which want DTCM — the
link buffers must not follow them there.

## Framing: a slot is a packet, and the grid can be moved

The receive ring is four buffers of 32 bytes, which is exactly one packet, so
"a buffer filled" and "a packet arrived" are the same event. There is no length
field to parse and no delimiter to scan for, and when the stream is in sync the
DMA's buffer swap lands in the idle gap between packets — which matters,
because the driver reloads the DMA in software and the USART holds only one
byte while it does. At 6 Mbaud that is 1.67 us of slack.

**Four slots, and four is the minimum.** The driver holds two buffers at once
and asks for slot *n+2* from inside the interrupt that reports slot *n*.
Reassembling a packet that straddles a boundary also needs slot *n-1*. With
three slots, *n-1* and *n+2* are the same buffer and the DMA overwrites the
bytes being read.

**A stream can start mid-packet**, which is what a reset of one board while the
other is sending produces. The receiver tracks where in a slot a packet *ends*
— 32 when packets and slots coincide, less when they do not — and recovers by
moving that boundary, never by stopping the DMA. Candidates are found by
scanning for the magic in the two-slot window and confirmed by CRC, at most
three CRCs per attempt, because a two-byte magic will occasionally appear
inside float payload.

That recovers correctness. It does **not** recover latency, and the host tests
in [`test/`](test/) are what made that visible: a packet ending part-way into a
slot is not handed over until the slot fills, so its delivery waits for the
first bytes of the *next* packet. On this link that is a whole control tick,
every tick, silently — precisely the kind of error E3 exists to measure and
would instead have absorbed.

So the buffer grid gets moved to match. The DMA's notion of a boundary is only
"this buffer is full", so handing the driver a single odd-sized buffer shifts
every boundary after it. To move the grid forward by *K* bytes the odd buffer
must be *K* modulo 32, and `32 + K` is used rather than `K`: it asks for the
same shift while giving the software reload far more time than a two-microsecond
buffer would. It costs the one packet that lands in it.

The framing is plain C over a byte ring, deliberately separated from the driver
plumbing ([`src/link_frame.c`](src/link_frame.c)), because on a board a framing
bug is indistinguishable from a wiring fault — as the section below cost four
rounds to learn. `bash firmware/link/test/build.sh` runs it natively over every
starting misalignment, a corrupted frame, a packet containing four false
magics, and a sequence-number wrap.

## Four boundary bugs, all found by instrumenting rather than guessing

Every one of these looked like a rate or wiring problem. Each was a gap where
the receiver stopped reading while the sender kept sending.

| symptom reported | actual cause |
| --- | --- |
| `trailer 1/12` | a bitwise CRC over 4000 bytes ran *between* payload and trailer |
| `trailer 11/12` | a 24-byte header CRC ran *before* the payload receive |
| `header 32/32 (crc bad)` | building a deadline after reading `'F'` lost the first header byte, shifting the whole frame by one |
| responder silent | a post-frame drain loop swallowed the initiator's `CMD_REPORT` |

The general rule, learned the expensive way: **on a polled link there is no safe
pause between two parts of one transfer.** Receive everything, then compute.

The responder reports where it stopped — phase, and bytes received against bytes
expected — which is what turned four rounds of guessing into four specific
fixes. Ask for it with `CMD_REPORT`, which the initiator sends once it has its
verdict and the link is provably idle.

## Running it

```bash
# responder first, so it is listening
EXTRA_CONF=rtt.conf bash firmware/scripts/build.sh link mini_stm32h743 -p always \
  -- -DLINK_ROLE=responder -DLINK_BAUD=6000000
west flash --runner jlink -d ~/ctrl-lab-build/link/mini_stm32h743

bash firmware/scripts/build.sh link nucleo_f767zi -p always \
  -- -DLINK_ROLE=initiator -DLINK_BAUD=6000000
bash firmware/scripts/flash.sh link nucleo_f767zi

python3 firmware/scripts/console.py --baud 115200          # initiator results
python3 firmware/scripts/rtt-read.py \
  ~/ctrl-lab-build/link/mini_stm32h743/zephyr/zephyr.elf   # responder diagnostic
```

Both ends must be built with the same `LINK_BAUD`. The initiator's console is
`usart3` at the board default 115200 — deliberately not the link, so
measurements print over a channel that is not the one being measured.
