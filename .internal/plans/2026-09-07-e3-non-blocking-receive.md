# E3 Non-Blocking DMA Receive Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use beads-superpowers:subagent-driven-development (recommended) or beads-superpowers:executing-plans to implement this plan task-by-task. Each Task becomes a bead (`bd create -t task --parent <epic-id>`). Steps within tasks use checkbox (`- [ ]`) syntax for human readability.

**Goal:** Implement a non-blocking, zero-CPU-overhead asynchronous DMA receive mechanism for the MCU-to-MCU serial link, eliminating 10 kHz kernel tick preemption byte loss and providing double-buffered sample latching with clock skew tracking for Stage E3.

**Architecture:** Utilize Zephyr's `CONFIG_UART_ASYNC_API` with Devicetree DMA stream mappings on both the STM32F767ZI and STM32H743VI. DMA streams continuously populate a 4-slot circular buffer ring in non-cacheable SRAM. The completion ISR validates CRC32 and latches the newest packet into a double buffer. The real-time control thread consumes the latest sample at tick start, tracking crystal clock drift via modular unsigned cycle arithmetic and detecting phase slips without blocking. Both transmission and reception use asynchronous DMA (`uart_tx` / `uart_rx_enable`).

**Tech Stack:** C (C99/C11), Zephyr RTOS v4.3.0, STM32 DMA / DMAMUX, ARM Cortex-M7 MPU / D-Cache, Zephyr Timing API, Python 3 test scripts.

## Global Constraints
- Target hardware: NUCLEO-F767ZI (`usart6` on `PG14`/`PG9`) and WeAct MiniSTM32H743 (`usart1` on `PA9`/`PA10`).
- Link baud rate: 6 Mbaud exact (zero baud error on both APB2 buses).
- Real-time frame size: 32 bytes fixed (`sizeof(struct ctrl_link_pkt)`).
- Memory constraints: DMA buffers must reside in non-cacheable SRAM (D2 domain on H743, SRAM1 on F767; NEVER in DTCM on H743).
- Real-time safety: No `irq_lock()` during active control or frame transfer; no memory allocation in the control path; float validation with `isfinite()`; all test loops bounded by timeouts.

---

### Task 1: Devicetree and Kconfig DMA Configuration

**Files:**
- Modify: `firmware/link/boards/nucleo_f767zi.overlay`
- Modify: `firmware/link/boards/mini_stm32h743.overlay`
- Modify: `firmware/link/prj.conf`

**Interfaces:**
- Consumes: Existing Zephyr board overlays and link UART definitions.
- Produces: DMA-enabled `link-uart` nodes compatible with `CONFIG_UART_ASYNC_API`.

**Acceptance Criteria:**
- `firmware/link` builds warning-free for `nucleo_f767zi` with `CONFIG_UART_ASYNC_API=y` and `CONFIG_DMA=y`.
- `firmware/link` builds warning-free for `mini_stm32h743` with `CONFIG_UART_ASYNC_API=y`, `CONFIG_DMA=y`, and `CONFIG_DMAMUX_STM32=y`.
- Linker map files confirm `__nocache` memory section is placed in physical non-cacheable SRAM.

- [ ] **Step 1: Update prj.conf with Async UART, DMA, and MPU Kconfigs**

Edit `firmware/link/prj.conf` to add:
```kconfig
CONFIG_SERIAL=y
CONFIG_UART_ASYNC_API=y
CONFIG_DMA=y
CONFIG_ARM_MPU=y
CONFIG_NOCACHE_MEMORY=y
```

- [ ] **Step 2: Add DMA stream bindings to nucleo_f767zi.overlay**

Update `firmware/link/boards/nucleo_f767zi.overlay`:
```dts
&usart6 {
	dmas = <&dma2 6 5 0x400 0x3>,
	       <&dma2 1 5 0x400 0x3>;
	dma-names = "tx", "rx";
};
```

- [ ] **Step 3: Add DMAMUX request bindings to mini_stm32h743.overlay**

Update `firmware/link/boards/mini_stm32h743.overlay`:
```dts
&usart1 {
	dmas = <&dmamux1 0 42 0x400 0x3>,
	       <&dmamux1 1 41 0x400 0x3>;
	dma-names = "tx", "rx";
};
```

- [ ] **Step 4: Build both targets to verify devicetree and DMA bindings**

Run:
```bash
bash firmware/scripts/build.sh link nucleo_f767zi -p always
bash firmware/scripts/build.sh link mini_stm32h743 -p always
```
Expected: Both builds complete with return code 0, and build logs confirm DMA drivers compiled.

- [ ] **Step 5: Commit Devicetree and Kconfig changes**

```bash
git add firmware/link/boards/ firmware/link/prj.conf
git commit -m "Configure UART DMA streams and MPU nocache for F767 and H743 link"
```

---

### Task 2: Per-Tick Packet Layout & Non-Blocking DMA Buffer Ring Transceiver

**Files:**
- Create: `firmware/link/src/link_pkt.h`
- Create: `firmware/link/src/link_dma.h`
- Create: `firmware/link/src/link_dma.c`
- Modify: `firmware/link/CMakeLists.txt`

**Interfaces:**
- Consumes: Zephyr UART Async API (`uart_rx_enable`, `uart_callback_set`, `uart_rx_buf_rsp`, `uart_tx`).
- Produces:
  ```c
  bool link_dma_init(const struct device *uart_dev);
  bool link_dma_send(const struct ctrl_link_pkt *pkt);
  bool link_dma_get_latest(struct ctrl_link_pkt *out, uint32_t *rx_cycle);
  ```

**Acceptance Criteria:**
- `struct ctrl_link_pkt` is packed and exactly 32 bytes (`sizeof(struct ctrl_link_pkt) == 32`).
- 4-slot ring buffer resides in `__nocache` memory.
- `link_dma_init` asserts `device_is_ready()` on UART and reports readiness loudly.
- `UART_RX_RDY` validates magic `0x434C` and CRC32 in $< 500\,\text{ns}$.
- Corrupted frames trigger active realignment scanning for magic offset $K$ with retry cap $N \le 3$.
- `link_dma_send` uses asynchronous `uart_tx()` with zero CPU spinning.

- [ ] **Step 1: Define struct ctrl_link_pkt in link_pkt.h**

Write `firmware/link/src/link_pkt.h`:
```c
#ifndef CTRL_LINK_PKT_H_
#define CTRL_LINK_PKT_H_

#include <stdint.h>

#define CTRL_LINK_MAGIC 0x434CU /* 'C', 'L' */

struct __packed ctrl_link_pkt {
	uint16_t magic;
	uint16_t signal_count;
	uint32_t seq;
	uint32_t tx_tick;
	float signals[4];
	uint32_t crc32;
};

_Static_assert(sizeof(struct ctrl_link_pkt) == 32, "ctrl_link_pkt must be exactly 32 bytes");

#endif /* CTRL_LINK_PKT_H_ */
```

- [ ] **Step 2: Implement link_dma.c with 4-slot circular buffer ring and uart_tx**

Implement `firmware/link/src/link_dma.c`:
- Verify `device_is_ready(uart_dev)`.
- Allocate `static __nocache struct ctrl_link_pkt rx_ring[4];`.
- Handle `UART_RX_BUF_REQUEST` by providing `rx_ring[(buf_idx + 1) % 4]`.
- Handle `UART_RX_RDY`:
  - Check `magic == CTRL_LINK_MAGIC` and `ctrl_crc32`.
  - If valid: stamp arrival cycle `k_cycle_get_32()`, atomic swap latest index.
  - If invalid: scan for `0x434C` at offset $K \in [1, 31]$. If found, realign stream; if not found, increment retry counter and rearm.
- Handle `UART_RX_DISABLED`: auto-rearm `uart_rx_enable()`.
- Implement `link_dma_send()` via `uart_tx(dev, (const uint8_t *)pkt, sizeof(*pkt), SYS_FOREVER_US)`.

- [ ] **Step 3: Update CMakeLists.txt and compile check**

Update `firmware/link/CMakeLists.txt` to include `src/link_dma.c`.
Run:
```bash
bash firmware/scripts/build.sh link nucleo_f767zi
bash firmware/scripts/build.sh link mini_stm32h743
```
Expected: Clean compilation, 0 errors, 0 warnings.

- [ ] **Step 4: Commit DMA transceiver implementation**

```bash
git add firmware/link/src/ firmware/link/CMakeLists.txt
git commit -m "Implement 4-slot DMA buffer ring transceiver and packet framing"
```

---

### Task 3: Real-Time Latch Consumption, Skew Accumulator & Float Validation

**Files:**
- Create: `firmware/link/src/link_latch.h`
- Create: `firmware/link/src/link_latch.c`
- Modify: `firmware/link/CMakeLists.txt`

**Interfaces:**
- Consumes: `link_dma_get_latest()` from Task 2.
- Produces:
  ```c
  enum link_latch_status {
      LATCH_OK = 0,
      LATCH_SLIP,     /* No new packet (ZOH hold) */
      LATCH_OVERRUN,  /* Remote clock faster (>1 packet elapsed) */
      LATCH_FAULT     /* Non-finite float or consecutive misses > 5 */
  };

  enum link_latch_status link_latch_step(float *signals_out,
                                         uint32_t *seq_out,
                                         int64_t *cumulative_skew_cycles);
  ```

**Acceptance Criteria:**
- Normal step returns `LATCH_OK`.
- Zero new packets returns `LATCH_SLIP`, holds prior sample, and increments slip count.
- Step-to-step modular cycle subtraction $(\text{uint32\_t})(t_{rx}[k] - t_{rx}[k-1])$ accurately accumulates into 64-bit integer without overflow discontinuity across $2^{32}$.
- Non-finite float (`NaN`/`Inf`) triggers immediate `LATCH_FAULT`.

- [ ] **Step 1: Implement link_latch.c with delta cycle arithmetic and float checks**

Write `firmware/link/src/link_latch.c`:
- Use `isfinite(signals[i])` on each signal before propagating.
- Track `last_seq` and detect slip/overrun.
- Compute delta cycles:
  ```c
  uint32_t rx_delta = (uint32_t)(rx_cycle - last_rx_cycle);
  uint32_t tx_delta = (uint32_t)(pkt.tx_tick - last_tx_tick) * nominal_cycles_per_tick;
  int32_t step_skew = (int32_t)(rx_delta - tx_delta);
  *cumulative_skew_cycles += step_skew;
  ```
- Trigger `LATCH_FAULT` if `consecutive_slips >= 5`.

- [ ] **Step 2: Add host unit test for modular cycle wrap arithmetic**

Create and run a host verification test in `firmware/link/test/test_latch.c`:
- Feed synthetic timestamps simulating a cycle wrap from `0xFFFFFFE0` to `0x00000030`.
- Verify calculated cycle delta is exactly `80` cycles with zero discontinuity.
Expected: PASS.

- [ ] **Step 3: Commit latch consumer and clock skew implementation**

```bash
git add firmware/link/src/link_latch.* firmware/link/CMakeLists.txt
git commit -m "Implement latch consumer with modular skew tracking and float safety"
```

---

### Task 4: Hardware Link Validation with 10 kHz Kernel Tick Active

**Files:**
- Modify: `firmware/link/src/main.c`
- Modify: `firmware/link/README.md`

**Interfaces:**
- Consumes: DMA transceiver (`link_dma`), packet definitions (`link_pkt.h`), latch (`link_latch`).
- Produces: Measured verification report comparing DMA link vs E2 polled link.

**Acceptance Criteria:**
- 1-byte startup handshake executes successfully prior to packet burst.
- Initiator and responder run with active 10 kHz kernel tick ISR (`sys_clock` ticking, no `irq_lock()`).
- 10,000 packets of 32 bytes transmitted across the link at 6 Mbaud via DMA.
- Packet timeouts bounded to 5 ms; test cleanly aborts on 50 consecutive drops.
- **Zero byte loss** and **100% CRC OK**.
- Measured software-to-software latency remains $\le 4.0\,\mu\text{s}$.
- Results recorded in `firmware/link/README.md`.

- [ ] **Step 1: Update main.c to include startup handshake and DMA transceiver test**

In `firmware/link/src/main.c`:
- Responder: Initialize DMA receiver, wait for `'P'`, reply `'p'`.
- Initiator: Sleep 200 ms, send `'P'`, wait up to 100 ms for `'p'`.
- Transmit 10,000 `ctrl_link_pkt` frames via `link_dma_send()` at 6 Mbaud.
- Maintain a 5 ms timeout per packet; abort on 50 consecutive timeouts.
- Run without `irq_lock()`.

- [ ] **Step 2: Flash and execute test on both boards**

Run:
```bash
# Build & flash responder (mini_stm32h743)
EXTRA_CONF=rtt.conf bash firmware/scripts/build.sh link mini_stm32h743 -p always -- -DLINK_ROLE=responder -DLINK_BAUD=6000000
west flash --runner jlink -d ~/ctrl-lab-build/link/mini_stm32h743

# Build & flash initiator (nucleo_f767zi)
bash firmware/scripts/build.sh link nucleo_f767zi -p always -- -DLINK_ROLE=initiator -DLINK_BAUD=6000000
bash firmware/scripts/flash.sh link nucleo_f767zi

# Capture console output
python3 firmware/scripts/console.py --baud 115200
```
Expected: 10,000 frames received, 0 bad CRC, 0 dropped bytes.

- [ ] **Step 3: Document results in firmware/link/README.md**

Update `firmware/link/README.md` with:
- Measured latency table for DMA receive.
- Proof of determinism with interrupts enabled.
- Conclusion resolving bead `ctrl-lab-b7r.7`.

- [ ] **Step 4: Commit validation results and close bead ctrl-lab-b7r.7**

```bash
git add firmware/link/src/main.c firmware/link/README.md
git commit -m "Verify non-blocking DMA link at 6 Mbaud with interrupts enabled

Closes ctrl-lab-b7r.7: 10,000 frames pass CRC with zero byte loss
while the 10 kHz kernel tick ISR is active. Software latency measured
at <= 4 us, proving the receive path is deterministic for E3."
```

---

## Stress Test Results: E3 Non-Blocking DMA Receive Plan

### Resolved Decisions
- **Asynchronous Transmission (`uart_tx`)**: Upgraded transmit path to DMA alongside receive, eliminating the 53.4 µs CPU blocking loop per packet.
- **Startup Handshake**: Added a 1-byte handshake (`'P'` / `'p'`) after 200 ms stabilization to eliminate DMA arming races between J-Link and ST-Link resets.
- **Timeout Protection**: Added a bounded 5 ms timeout per packet and a 50-packet abort threshold to prevent test hangs on hardware disconnection.
- **Hardware Assertions**: Enforced explicit `CONFIG_DMA=y` and runtime `device_is_ready()` assertions for UART and DMA blocks.

### Changes Made
- Task 1: Added `CONFIG_DMA=y`.
- Task 2: Added `uart_tx()` transmission support to `link_dma`.
- Task 4: Added handshake sequence and bounded timeouts to `main.c`.

### Confidence Assessment
- **Overall**: High
- **Readiness**: All 4 tasks have clear, bite-sized steps and verifiable outcomes.
