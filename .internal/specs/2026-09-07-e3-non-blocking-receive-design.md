# Design Specification: Non-Blocking DMA Receive for Stage E3 Control Loop

**Date:** 2026-09-07  
**Status:** Approved  
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

### 2.3 Memory Placement & Cache Coherency (Cortex-M7)
Both target chips feature Cortex-M7 cores with active L1 data cache (D-cache):
- Because DMA controllers write physical SRAM without passing through the L1 cache pipeline, sharing cached RAM with DMA introduces cache incoherency hazards.
- All DMA reception ping-pong buffers must reside in **non-cacheable memory** (using Zephyr's `__nocache` attribute or dedicated non-cached SRAM / DTCM sections).
- This avoids costly manual cache line flushes/invalidations in the real-time hot path.

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

### 3.2 Continuous Asynchronous Reception Flow

1. **Buffer Provisioning**:
   - Two ping-pong buffers (`rx_buf[0]` and `rx_buf[1]`), each `sizeof(struct ctrl_link_pkt)` in non-cacheable RAM.
   - Armed via `uart_rx_enable(link, rx_buf[0], sizeof(rx_buf[0]), SYS_FOREVER_US)`.
2. **Buffer Request Callback (`UART_RX_BUF_REQUEST`)**:
   - The driver requests the next buffer before the current one finishes.
   - The callback immediately calls `uart_rx_buf_rsp(link, rx_buf[next_idx], sizeof(rx_buf[next_idx]))`.
3. **Completion Callback (`UART_RX_RDY`)**:
   - Triggered when all 32 bytes of a frame land in RAM.
   - Validates `magic == 0x434C` and checks `crc32`.
   - On match: stamps arrival time `rx_cycle = k_cycle_get_32()` and atomically marks the buffer as latest ready (`atomic_set(&latest_ready_idx, current_idx)`).
   - On corruption/framing error: increments `rx_crc_errors` and realigns on magic word boundary.

---

## 4. Real-Time Synchronization & Clock Skew Tracking

### 4.1 Decoupled Crystal Execution
In accordance with Stage E3 objectives, the controller and plant run **unsynchronized on their independent crystals** to expose real clock drift and phase slip.

### 4.2 Latch Consumption in the Control Thread
At the start of each control tick:
1. The control thread inspects `latest_ready_idx`.
2. If a new packet arrived ($S_{\text{curr}} == S_{\text{last}} + 1$):
   - Copy `signals[]` into the plan's input vector.
   - Record normal progress.
3. If no new packet arrived ($S_{\text{curr}} == S_{\text{last}}$):
   - Remote board's tick has not arrived yet due to phase lag.
   - Hold the previous sample value (Zero-Order Hold).
   - Log a missed deadline / phase slip event (`slip_count++`).
4. If multiple packets arrived ($S_{\text{curr}} > S_{\text{last}} + 1$):
   - Remote board is running slightly faster than the local board.
   - Latch newest sample and log `overrun_count++`.

### 4.3 Clock Skew Measurement Formula
Comparing sender tick timestamps against local reception cycle counter values over $N$ steps:
$$\text{Drift}[k] = (t_{\text{rx}}[k] - t_{\text{rx}}[0]) - (t_{\text{tx}}[k] - t_{\text{tx}}[0])$$
This directly computes crystal frequency deviation (PPM) and link transport jitter without introducing artificial time synchronization protocols.

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
