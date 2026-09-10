/*
 * Non-blocking DMA transceiver for the link. See link_dma.h for the contract,
 * link_frame.h for the framing, and firmware/link/README.md for the board
 * wiring and the cache rules.
 *
 * The shape of this file is set by three constraints, none of them obvious:
 *
 *   1. The STM32 async UART driver refuses buffers it cannot prove are
 *      non-cacheable. uart_tx, uart_rx_enable and uart_rx_buf_rsp all call
 *      stm32_buf_in_nocache() and return -EFAULT otherwise - transmit buffers
 *      included. So the ring AND the transmit staging buffer live in the
 *      __nocache section, and link_dma_send() copies rather than passing the
 *      caller's pointer through.
 *
 *   2. The driver hands buffers back one at a time and asks for the next from
 *      inside the completion ISR. It holds two at once - the one filling and
 *      the one queued - and asks for slot n+2 immediately after reporting slot
 *      n. Reassembling a packet also needs slot n-1, so the ring needs four:
 *      with three, n-1 and n+2 are the same buffer and the DMA would overwrite
 *      the bytes being read. That is the reason for four, not throughput.
 *
 *   3. A slot is exactly one packet long, so when the stream is in sync the
 *      buffer swap happens in the idle gap between packets. That matters
 *      because the driver reloads the DMA in software: between the
 *      transfer-complete interrupt and dma_start() the peripheral is holding
 *      bytes in a one-deep register, and at 6 Mbaud that is 1.67 us of slack.
 *
 * When the stream is not in sync, nothing above is lost - link_frame realigns
 * by moving the packet boundary, never by stopping the DMA.
 */

#include <string.h>

#include <zephyr/kernel.h>
#include <zephyr/device.h>
#include <zephyr/drivers/uart.h>
#include <zephyr/sys/barrier.h>
#include <zephyr/sys/printk.h>
#include <zephyr/linker/section_tags.h>

#include "link_dma.h"
#include "link_frame.h"
#include "link_pkt.h"

#define RING_SLOTS 4U

/* The DMA's memory. One contiguous array deliberately: slots are handed to the
 * driver in strict order, so consecutive slots form a contiguous byte stream
 * and a packet straddling a boundary is just a pair of copies.
 */
static __nocache uint8_t rx_ring[RING_SLOTS][CTRL_LINK_PKT_LEN] __aligned(4);
static __nocache struct ctrl_link_pkt tx_stage;

/* Used once per realignment, never for a packet. See realign_arm(). */
static __nocache uint8_t realign_buf[2U * CTRL_LINK_PKT_LEN] __aligned(4);

static const struct device *link_dev;

/* Receive state. Touched only by the UART callback, which is ISR context and
 * never preempts itself, so none of it needs protection.
 */
static struct link_frame framer;
static uint8_t next_slot;	/* slot to hand out on the next buffer request */
static struct link_dma_stats stats;

/* Realignment of the buffer grid. See realign_arm() for what this is for; the
 * state exists only so that the several slots between wanting a realignment
 * and getting one do not each ask for another.
 */
enum realign_state {
	REALIGN_IDLE = 0,
	REALIGN_WANTED,		/* hand out the odd buffer at the next request */
	REALIGN_QUEUED,		/* handed out, waiting for it to fill */
};

static enum realign_state realign;
static uint16_t realign_len;
static uint8_t realign_tries;

/* Four attempts, then stop asking. Framing is already correct without this -
 * only latency suffers - so a link that will not align is worth a counter and
 * not worth an unbounded stream of odd buffers.
 */
#define REALIGN_MAX_TRIES 4

/* The latch. The callback writes one of two packets and then publishes by
 * incrementing latch_seq; a reader that sees the same latch_seq before and
 * after its copy knows the copy was not overwritten underneath it. The
 * published slot for sequence S is S & 1.
 */
static struct ctrl_link_pkt latch[2];
static uint32_t latch_cycle[2];
static volatile uint32_t latch_seq;

/* Transmit in flight. Set by the thread, cleared by the callback; only one
 * side ever performs each transition, so no atomic is needed.
 */
static volatile bool tx_busy;

/* --- the latch ----------------------------------------------------------- */

static void latch_publish(const struct ctrl_link_pkt *pkt, uint32_t cycle)
{
	const uint32_t seq = latch_seq;
	const uint32_t slot = (seq + 1U) & 1U;

	latch[slot] = *pkt;
	latch_cycle[slot] = cycle;

	/* The packet must be visible before the sequence that advertises it. */
	barrier_dmem_fence_full();
	latch_seq = seq + 1U;
}

bool link_dma_get_latest(struct ctrl_link_pkt *out, uint32_t *rx_cycle)
{
	/* Bounded, not a spin: a retry costs one more 32-byte copy and can only
	 * be forced by a packet arriving mid-copy. At one packet per 100 us
	 * against a copy of a few dozen cycles, three attempts is already
	 * unreachable - it is a cap, not a strategy.
	 */
	for (int attempt = 0; attempt < 3; attempt++) {
		const uint32_t seq = latch_seq;

		if (seq == 0U) {
			return false;	/* nothing has ever arrived */
		}

		*out = latch[seq & 1U];
		if (rx_cycle != NULL) {
			*rx_cycle = latch_cycle[seq & 1U];
		}

		barrier_dmem_fence_full();
		if (latch_seq == seq) {
			return true;
		}
	}
	return false;
}

/* --- receive ------------------------------------------------------------- */

/* Moves the buffer grid so that slots end where packets end.
 *
 * Reception is left running throughout. The trick is that the DMA's idea of a
 * boundary is just "this buffer is full", so handing it one buffer of an odd
 * size shifts every boundary after it. To move the grid forward by `end` bytes
 * the odd buffer must be `end` modulo 32 - and 32 + end is chosen over end
 * itself deliberately: a short buffer at 6 Mbaud would fill in a couple of
 * microseconds and the driver reloads the DMA in software, so the longer form
 * asks for the same shift with more time to service it.
 *
 * The cost is one packet, discarded with the odd buffer.
 */
static void realign_arm(void)
{
	if (realign != REALIGN_IDLE || link_frame_aligned(&framer)) {
		return;
	}
	if (realign_tries >= REALIGN_MAX_TRIES) {
		return;
	}

	realign_len = (uint16_t)(CTRL_LINK_PKT_LEN + framer.pkt_end);
	realign = REALIGN_WANTED;
	realign_tries++;
}

static void rx_ready(const uint8_t *buf)
{
	/* Stamp first. Everything below is bookkeeping whose cost should not
	 * land in the arrival time the skew accumulator differences.
	 */
	const uint32_t cycle = k_cycle_get_32();
	const unsigned int idx = (unsigned int)((buf - &rx_ring[0][0]) / CTRL_LINK_PKT_LEN);
	struct ctrl_link_pkt pkt;
	uint32_t cost;

	stats.rx_slots++;

	if (link_frame_take(&framer, idx, &pkt)) {
		stats.rx_ok++;
		latch_publish(&pkt, cycle);
	}

	/* Checked on every packet, not only after a resync: reception can be
	 * re-armed mid-stream, and the grid it comes back on is whatever the
	 * far side happened to be sending at the time.
	 */
	realign_arm();

	/* One extra clock read, kept because the receive callback's cost is a
	 * number this stage has to report rather than assume.
	 */
	cost = k_cycle_get_32() - cycle;
	if (cost > stats.rx_worst_cycles) {
		stats.rx_worst_cycles = cost;
	}
}

static void rx_arm(void)
{
	const int rc = uart_rx_enable(link_dev, rx_ring[next_slot],
				      CTRL_LINK_PKT_LEN, SYS_FOREVER_US);

	if (rc != 0) {
		printk("link_dma: uart_rx_enable failed (%d)\n", rc);
		return;
	}
	next_slot = (uint8_t)((next_slot + 1U) % RING_SLOTS);
}

static void link_cb(const struct device *dev, struct uart_event *evt, void *user_data)
{
	ARG_UNUSED(dev);
	ARG_UNUSED(user_data);

	switch (evt->type) {
	case UART_TX_DONE:
		stats.tx_done++;
		tx_busy = false;
		break;

	case UART_TX_ABORTED:
		tx_busy = false;
		break;

	case UART_RX_RDY:
		/* A slot is one packet, and SYS_FOREVER_US stops the driver
		 * delivering a partial one on an idle line, so this fires once
		 * per 32 bytes with offset 0. It is checked anyway: a short
		 * event would mean the driver's contract changed underneath
		 * us, and reassembling the wrong bytes silently would be worse
		 * than dropping them.
		 */
		if (evt->data.rx.buf == realign_buf) {
			/* The odd buffer has filled, so every boundary from
			 * here on is a packet boundary. Its contents are a
			 * packet and a bit, and are deliberately dropped: the
			 * point of it was the shift, not the payload.
			 */
			link_frame_set_aligned(&framer);
			realign = REALIGN_IDLE;
			stats.realigns++;
		} else if (evt->data.rx.len == CTRL_LINK_PKT_LEN &&
			   evt->data.rx.offset == 0U) {
			rx_ready(evt->data.rx.buf);
		} else {
			stats.rx_short++;
		}
		break;

	case UART_RX_BUF_REQUEST:
		/* Must always be answered. If the driver finishes a buffer
		 * with nothing queued behind it, reception stops entirely.
		 */
		if (realign == REALIGN_WANTED) {
			if (uart_rx_buf_rsp(link_dev, realign_buf, realign_len) == 0) {
				realign = REALIGN_QUEUED;
				break;
			}
			realign = REALIGN_IDLE;	/* refused; try again later */
		}

		if (uart_rx_buf_rsp(link_dev, rx_ring[next_slot],
				    CTRL_LINK_PKT_LEN) == 0) {
			next_slot = (uint8_t)((next_slot + 1U) % RING_SLOTS);
		}
		break;

	case UART_RX_BUF_RELEASED:
		/* Slots are tracked by index, so there is nothing to free. */
		break;

	case UART_RX_STOPPED:
		stats.rx_errors++;
		break;

	case UART_RX_DISABLED:
		/* Re-arm rather than stay deaf. The packet boundary is
		 * deliberately kept: the next packets will either validate
		 * against it or drive a resync, and resetting it would throw
		 * away an alignment that is probably still correct.
		 */
		stats.rx_restarts++;
		rx_arm();
		break;

	default:
		break;
	}
}

/* --- transmit ------------------------------------------------------------ */

bool link_dma_send(const struct ctrl_link_pkt *pkt)
{
	if (tx_busy) {
		stats.tx_rejected++;
		return false;
	}

	memcpy(&tx_stage, pkt, sizeof(tx_stage));
	link_frame_stamp(&tx_stage);

	/* Claimed before the transfer starts, so the completion interrupt
	 * cannot clear a flag that was never set.
	 */
	tx_busy = true;

	const int rc = uart_tx(link_dev, (const uint8_t *)&tx_stage,
			       sizeof(tx_stage), SYS_FOREVER_US);

	if (rc != 0) {
		tx_busy = false;
		stats.tx_rejected++;
		return false;
	}
	return true;
}

/* --- init ---------------------------------------------------------------- */

void link_dma_get_stats(struct link_dma_stats *out)
{
	*out = stats;
	out->rx_bad = framer.bad;
	out->resyncs = framer.resyncs;
}

bool link_dma_init(const struct device *uart_dev)
{
	/* Deliberately loud. A link that comes up deaf looks exactly like a
	 * wiring fault from the far side, and E2 lost time to that once.
	 */
	if (!device_is_ready(uart_dev)) {
		printk("link_dma: FAIL uart %s not ready\n",
		       uart_dev != NULL ? uart_dev->name : "(null)");
		return false;
	}
	link_dev = uart_dev;

	if (!link_frame_crc_init()) {
		printk("link_dma: FAIL table crc32 disagrees with dcp.c\n");
		return false;
	}

	link_frame_init(&framer, (const uint8_t (*)[CTRL_LINK_PKT_LEN])rx_ring, RING_SLOTS);
	next_slot = 0U;
	realign = REALIGN_IDLE;
	realign_tries = 0U;
	latch_seq = 0U;
	tx_busy = false;
	memset(&stats, 0, sizeof(stats));
	memset(rx_ring, 0, sizeof(rx_ring));

	const int rc = uart_callback_set(link_dev, link_cb, NULL);

	if (rc != 0) {
		printk("link_dma: FAIL uart_callback_set (%d) - is the async "
		       "API built for this instance?\n", rc);
		return false;
	}

	rx_arm();
	return true;
}
