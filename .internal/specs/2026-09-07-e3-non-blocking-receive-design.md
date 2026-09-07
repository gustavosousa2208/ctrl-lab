# Design Specification: Non-Blocking DMA Receive for Stage E3 Control Loop

**Date:** 2026-09-07  
**Status:** Approved (Post Stress-Test)  
**Related Beads:** `ctrl-lab-b7r.7` (Decision), `ctrl-lab-b7r` (Stage E Epic), `ctrl-lab-b7r.4` (Stage E3)

---

## 1. Executive Summary & Problem Context

During Stage E2 link characterization, communication between the NUCLEO-F767ZI (controller) and WeAct MiniSTM32H743 (plant) was proven at baud rates up to 6 Mbaud. However, Stage E2 also uncovered a critical timing hazard:
- Polled UART byte reception without hardware buffering requires unbroken CPU attention.
- When Zephyr's 10 kHz kernel tick ISR fires (every 100 µs, lasting up to several microseconds), it preempts the software polling loop, dropping one to three bytes mid-frame.
- Wrapping the receive in `irq_lock()` restored determinism for a 40 ms transport probe, but holding interrupts disabled for 40 ms (or even tens of microseconds during active control) is unacceptable for Stage E3's real-time control loop.

This specification defines a non-blocking, zero-CPU-overhead receive architecture based on Zephyr's **Asynchronous UART DMA API** (`CONFIG_UART_ASYNC_API`). It enables continuous sample reception, isolates the receive path from kernel tick jitter, and bridges the two independent hardware crystal clock domains with double-buffered sample latching.

---

## 2. Hardware Architecture & DMA Mapping

### 2.1 Peripheral & DMA Assignment

| Role | Board | SoC | UART Node | Pins | Peripheral Bus | DMA Controller | Stream / Channel / Request |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Controller** | NUCLEO-F767ZI | STM32F767ZI | `&usart6` | PG14 (TX), PG9 (RX) | APB2 (108 MHz) | `dma2` | RX: Stream 1, Ch 5<br>TX: Stream 6, Ch 5 |
| **Plant** | MiniSTM32H743 | STM32H743VI | `&usart1` | PA9 (TX), PA10 (RX) | APB2 (120 MHz) | `dmamux1` / `dma1` | RX: DMAMUX Req 41<br>TX: DMAMUX Req 42 |

### 2.2 Devicetree Overlays

#### Controller (`boards/nucleo_f767zi.overlay`)
```dts
&usart6 {
    dmas = <&dma2 6 5 0x400 0x3>,
           <&dma2 1 5 0x400 0x3>;
    dma-names = "tx", "rx";
};
```

#### Plant (`boards/mini_stm32h743.overlay`)
```dts
&usart1 {
    dmas = <&dmamux1 0 42 0x400 0x3>,
           <&dmamux1 1 41 0x400 0x3>;
    dma-names = "tx", "rx";
};
```

### 2.3 Memory Placement, Bus Matrix & Cache Coherency (Cortex-M7)
Both target chips feature Cortex-M7 cores with active L1 data cache (D-cache):
- **STM32H7 Bus Matrix Restriction**: On the STM32H743, peripheral DMA controllers (DMA1/DMA2) **cannot access DTCM** (0x20000000). Directing DMA streams to DTCM triggers a hardware transfer error. DMA buffers on the H743 must be located in **SRAM (D2 domain)** (e.g. `sram1` or `sram2` at `0x30000000`).
- **Cache Incoherency Hazard**: Because DMA controllers write physical SRAM without passing through the CPU L1 cache, sharing cached RAM with DMA introduces cache stale read and dirty writeback corruption.
- **MPU Non-Cacheable Region**: All DMA reception buffers must reside in **non-cacheable memory** backed by MPU configuration:
  - `CONFIG_ARM_MPU=y`
  - `CONFIG_NOCACHE_MEMORY=y`
  - Buffers tagged with Zephyr's `__nocache` attribute.

---

## 3. Communication Framing & Real-Time Reception

### 3.1 Per-Tick Packet Layout (`ctrl_link_pkt`)

Real-time sample exchange transfers a compact, fixed-size 32-byte frame:

```c
struct __packed ctrl_link_pkt {
    uint16_t magic;         /* 0x434C ('C', 'L') frame sync word */
    uint16_t signal_count;  /* Number of active float signals (1..4) */
    uint32_t seq;           /* Monotonically increasing sequence number */
    uint32_t tx_tick;       /* Sender local tick counter at transmit time */
    float    signals[4];    /* Actuator command u[k] or plant output y[k] */
    uint32_t crc32;         /* CRC32 covering header and signals */
};
```

#### Transmission Budget at 6 Mbaud
- Byte duration: $1.67\,\mu\text{s}$
- Frame length: 32 bytes
- On-wire duration: $53.4\,\mu\text{s}$
- In a 1000 µs (1 kHz) or 50,000 µs (20 Hz) control period, wire time is a minor fraction of the cycle budget.

### 3.2 Continuous Asynchronous Reception Flow & 4-Slot Buffer Ring

1. **4-Slot Buffer Ring**:
   - Instead of a bare 2-buffer ping-pong, allocate a 4-slot circular buffer ring (`rx_buf[4]`, 128 bytes total) in non-cacheable RAM to eliminate buffer starvation risks during high-frequency interrupts or rapid bursts.
   - Armed via `uart_rx_enable(link, rx_buf[0], sizeof(rx_buf[0]), SYS_FOREVER_US)`.
2. **Buffer Request Callback (`UART_RX_BUF_REQUEST`)**:
   - The driver requests the next buffer before the current one finishes.
   - The callback immediately responds with the next slot in the ring: `uart_rx_buf_rsp(link, rx_buf[next_idx], sizeof(rx_buf[next_idx]))`.
3. **Completion Callback (`UART_RX_RDY`)**:
   - Triggered when all 32 bytes of a frame land in RAM.
   - Validates `magic == 0x434C` and checks `crc32`.
   - On match: stamps arrival time `rx_cycle = k_cycle_get_32()` and atomically marks the buffer as latest ready (`atomic_set(&latest_ready_idx, current_idx)`).
   - On corruption/framing mismatch: invokes framing realignment (see Section 3.3).
4. **Auto-Recovery on Disabling (`UART_RX_DISABLED`)**:
   - If an overrun or callback starvation disables DMA, the handler immediately logs `dma_starved++` and re-enables DMA reception (`uart_rx_enable`).

### 3.3 Active Framing Realignment on Noise / Wire Glitches
If electrical noise or startup shifts the byte stream:
1. When `UART_RX_RDY` fails magic or CRC, scan the 32-byte corrupted buffer for `0x434C`.
2. If `0x434C` is found at offset $K \in [1, 31]$:
   - Temporarily disable DMA (`uart_rx_disable`).
   - Read the $K$ trailing bytes to re-synchronize the hardware FIFO/stream.
   - Re-enable 32-byte DMA reception.
3. Guard the realignment routine with a retry cap ($N \le 3$); if unaligned after 3 retries, fall back to line idle detection and buffer flush.

---

## 4. Real-Time Synchronization & Clock Skew Tracking

### 4.1 Decoupled Crystal Execution
In accordance with Stage E3 objectives, the controller and plant run **unsynchronized on their independent crystals** to expose real clock drift and phase slip.

### 4.2 Latch Consumption in the Control Thread
At the start of each control tick:
1. The control thread inspects `latest_ready_idx`.
2. If a new packet arrived ($S_{\text{curr}} == S_{\text{last}} + 1$):
   - Check arithmetic safety: verify `isfinite(signals[i])` for each signal. If non-finite (`NaN`/`Inf`), trigger safety `FAULT`.
   - Copy `signals[]` into the plan's input vector.
   - Record normal progress.
3. If no new packet arrived ($S_{\text{curr}} == S_{\text{last}}$):
   - Remote board's tick has not arrived yet due to phase lag.
   - Hold the previous sample value (Zero-Order Hold).
   - Log a missed deadline / phase slip event (`slip_count++`).
4. If multiple packets arrived ($S_{\text{curr}} > S_{\text{last}} + 1$):
   - Remote board is running slightly faster than the local board.
   - Latch newest sample and log `overrun_count++`.

### 4.3 Clock Skew Measurement & 32-Bit Cycle Counter Wrap
On Cortex-M7 running at 216/240 MHz, `k_cycle_get_32()` overflows every $\approx 17.9\text{--}19.9\,\text{s}$. To prevent integer wrap corruption on runs $> 17.8\,\text{s}$:
1. Compute per-step cycle increments via unsigned modular subtraction:
   $$\Delta t_{\text{rx}}[k] = (\text{uint32\_t})(t_{\text{rx}}[k] - t_{\text{rx}}[k-1])$$
   $$\Delta t_{\text{tx}}[k] = (\text{uint32\_t})(t_{\text{tx}}[k] - t_{\text{tx}}[k-1])$$
2. Accumulate drift in a signed 64-bit accumulator:
   $$\text{cumulative\_skew\_cycles} \mathrel{+}= (\text{int64\_t})\Delta t_{\text{rx}}[k] - (\text{int64\_t})\Delta t_{\text{tx\_local}}[k]$$
3. Evaluate packet sequence deltas as signed: `(int32_t)(seq_curr - seq_last)`.

### 4.4 Failsafe & Error Handling
If missed deadlines exceed 5 consecutive ticks, the runtime transitions to `FAULT`, setting actuator outputs to safe values and stopping plan execution.

---

## 5. Verification & Acceptance Plan

### 5.1 Step 1: Standalone Link Characterization (`firmware/link`)
- **Objective**: Reproduce the Stage E2 test with kernel timer interrupts running at 10 kHz, but using the new DMA async receive path without `irq_lock()`.
- **Traffic**: 10,000 packets of 32 bytes sent at 6 Mbaud.
- **Pass Criteria**:
  - Zero dropped bytes.
  - 100% CRC OK across all 10,000 packets.
  - Software latency $\le 4.0\,\mu\text{s}$.

### 5.2 Step 2: Unsynchronized Free-Running Skew Test
- **Objective**: Run F767 and H743 for 1,000 ticks at nominal 20 Hz (50 ms).
- **Pass Criteria**:
  - Measured drift curve is monotonic and corresponds to crystal ppm ratings ($\approx 10\text{--}50\,\text{ppm}$).
  - Slips correlate precisely with cumulative phase drift.

### 5.3 Step 3: Closed-Loop E3 Execution
- Run fixture `test-projects/04-2nd-order-system.plan.dcp` across both boards.
- Grade against reference trace `04-2nd-order-system.f32.csv`.
- Report all committed root metrics:
  - Worst-Case Execution Time (WCET)
  - Tick jitter
  - Transport latency
  - Deadline misses / slips
  - Maximum absolute error ($e_{\text{max}}$) and RMS error.

---

## 6. Stress Test Results: Non-Blocking DMA Receive for Stage E3

### Resolved Decisions
- **Framing Realignment**: Added active offset-search realignment on CRC failure with a bounded retry limit ($N \le 3$) to recover from electrical glitches without falling out of sync.
- **Buffer Starvation Defense**: Sized buffer pool to a 4-slot ring (128 bytes total) and added an explicit `UART_RX_DISABLED` auto-rearm handler.
- **Cycle Counter Wrap**: Switched clock skew tracking from naive difference against $t_0$ to modular unsigned step-to-step delta subtraction and a 64-bit accumulator, preventing wrap corruption after 17.9 s.
- **Hardware Bus Constraints**: Constrained H743 DMA buffers to D2 domain SRAM (`sram1`/`sram2`), explicitly avoiding DTCM which is inaccessible to DMA1/DMA2 on STM32H7.
- **Arithmetic Safety**: Mandated `isfinite()` validation on all deserialized float signals before passing them to the kernel dispatch table, triggering `FAULT` if non-finite.

### Changes Made
- Updated Section 2.3 with STM32H7 bus matrix rules.
- Upgraded Section 3.2 to a 4-slot circular DMA ring.
- Added Section 3.3 for framing realignment.
- Updated Section 4.2 with `isfinite()` validation.
- Updated Section 4.3 with modular delta arithmetic and 64-bit skew accumulation.

### Deferred / Parking Lot
- Slaving the plant clock to the controller tick: Intentionally deferred to a follow-up experiment as required by the E3 roadmap (measuring free-running crystal drift is the primary deliverable of E3).

### Confidence Assessment
- **Overall**: High
- **Areas of Concern**: Ensuring Zephyr's STM32 async UART driver correctly binds DMAMUX channels on the WeAct MiniSTM32H743 without device tree discrepancies (to be validated in Step 1).
