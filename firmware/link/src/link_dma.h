/*
 * Non-blocking DMA transceiver for the link, stage E3.
 *
 * Both directions are asynchronous: the DMA controller moves the bytes and the
 * CPU is only involved at packet boundaries. Nothing here blocks, spins or
 * locks interrupts, which is the property E3 exists to preserve - see
 * firmware/link/README.md and bead ctrl-lab-b7r.7 for why the E2 polled path
 * could not be carried forward.
 *
 * Threading: link_dma_send() and link_dma_get_latest() are called from the
 * control thread. Everything else runs in the UART callback, which is ISR
 * context. The two meet at a sequence-counted double buffer, so the consumer
 * never blocks the receiver and never reads a half-written packet.
 */

#ifndef CTRL_LINK_DMA_H_
#define CTRL_LINK_DMA_H_

#include <stdbool.h>
#include <stdint.h>
#include <zephyr/device.h>

#include "link_pkt.h"

/* Counters, all free-running. Read them from the control thread; they are
 * written only by the receive callback and only ever increase, so a torn read
 * costs an undercount and nothing else.
 */
struct link_dma_stats {
	uint32_t rx_slots;	/* buffers the DMA filled */
	uint32_t rx_ok;		/* packets that passed magic and CRC */
	uint32_t rx_bad;	/* packets that did not, resync attempted */
	uint32_t resyncs;	/* times the packet boundary moved */
	uint32_t realigns;	/* times the buffer grid was moved to match it */
	uint32_t rx_short;	/* RX_RDY events that were not a whole slot */
	uint32_t rx_errors;	/* UART_RX_STOPPED: overrun, framing, parity */
	uint32_t rx_restarts;	/* times reception had to be re-armed */
	uint32_t tx_done;	/* completed transmits */
	uint32_t tx_rejected;	/* sends refused because one was in flight */
	/* Cost of the receive callback, in cycles. The first call is kept apart
	 * from the rest because it is not the same measurement: it runs with a
	 * cold I-cache and a CRC table nothing has touched yet, and reporting
	 * it as the worst case would describe a startup transient as if it
	 * were the steady state. Both are worth knowing; conflating them is
	 * not.
	 */
	uint32_t rx_first_cycles;
	uint32_t rx_worst_cycles;	/* after the first */
	uint64_t rx_total_cycles;	/* after the first, for the mean */
};

/* Arms reception. Must be called before any packet can arrive; the caller
 * still owns the handshake that decides when the far side starts sending.
 *
 * Returns false, loudly, if the UART is not ready, if the driver rejects the
 * ring, or if the table-driven CRC here disagrees with dcp.c's.
 */
bool link_dma_init(const struct device *uart_dev);

/* Stages a copy of `pkt`, stamps the magic and CRC into it, and hands it to
 * the transmit DMA. Returns immediately.
 *
 * The caller's packet is not modified and need not outlive the call. Returns
 * false if a transmit is still in flight, which at one packet per tick means
 * the far side is not draining or the baud rate is wrong.
 */
bool link_dma_send(const struct ctrl_link_pkt *pkt);

/* Copies the most recently validated packet, and the cycle count at which its
 * last byte landed, out of the latch.
 *
 * Returns false only if nothing has ever been latched. It does NOT report
 * whether the packet is new - the packet's own seq field does that, which is
 * what link_latch_step() uses to tell a fresh sample from a held one.
 */
bool link_dma_get_latest(struct ctrl_link_pkt *out, uint32_t *rx_cycle);

void link_dma_get_stats(struct link_dma_stats *out);

#endif /* CTRL_LINK_DMA_H_ */
