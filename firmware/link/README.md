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
