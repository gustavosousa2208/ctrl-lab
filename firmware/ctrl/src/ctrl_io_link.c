/*
 * The control core's channels, carried over the MCU-to-MCU link.
 *
 * This is the file that finally connects `firmware/ctrl` to `firmware/link`,
 * and it is the only one in the control application that knows the link
 * exists. Everything above it sees `ctrl_io_read` and `ctrl_io_write`.
 *
 * The shape is set by one requirement: a tick's worth of I/O must be ONE
 * packet in each direction, sampled and published at one instant.
 *
 *   begin_tick   takes a single snapshot of the latched inbound packet. Every
 *                read for the rest of the tick is served from that copy, so two
 *                Input blocks on one channel cannot straddle the interrupt that
 *                publishes the next packet and see different samples.
 *   write        fills a slot in the outgoing packet. Nothing goes on the wire.
 *   end_tick     sends it, once. A packet per Output block would put two on the
 *                wire for a two-channel plan and halve the rate the link can
 *                sustain.
 *
 * Latency is why `end_tick` is called between the two passes rather than after
 * them - see runtime.c. The measured one-way cost of this link is about 89 us
 * against a 100 us tick (firmware/link/README.md), so a pass of scheduling is
 * not a rounding error here.
 */

#include <zephyr/kernel.h>
#include <zephyr/device.h>
#include <string.h>

#include "ctrl_io.h"
#include "ctrl_io_link.h"
#include "dcp.h"
#include "link_dma.h"
#include "link_pkt.h"

#define LINK_NODE DT_ALIAS(link_uart)
#if !DT_NODE_EXISTS(LINK_NODE)
#error "CTRL_LINK needs a link-uart alias - see firmware/link/boards/"
#endif

/* Without the async API this file compiles, links, and then fails at runtime on
 * a board that looks correctly configured. CMakeLists appends link.conf for
 * exactly this reason; the check is here as well because a build that loses the
 * fragment should stop rather than produce a deaf binary.
 */
#if !defined(CONFIG_UART_ASYNC_API)
#error "CTRL_LINK needs CONFIG_UART_ASYNC_API - see firmware/ctrl/link.conf"
#endif

static const struct device *const link_dev = DEVICE_DT_GET(LINK_NODE);

/* This tick's inbound packet, and whether one was ever latched. `fresh` says
 * the snapshot is new since last tick; without it a stalled link would serve
 * the same sample for ever and read as a working one.
 */
static struct ctrl_link_pkt inbound;
static bool inbound_valid;
static bool inbound_fresh;
static uint32_t inbound_last_seq;

/* This tick's outbound packet, and whether anything was written into it. */
static struct ctrl_link_pkt outbound;
static bool outbound_dirty;
static uint32_t outbound_seq;

/* Counted rather than reported: a control step cannot fail because a packet was
 * late, and the run's telemetry is where a starved link should show up. Read
 * them with ctrl_io_link_stats().
 */
static struct ctrl_io_link_stats stats;

bool ctrl_io_link_init(void)
{
	memset(&inbound, 0, sizeof(inbound));
	memset(&outbound, 0, sizeof(outbound));
	memset(&stats, 0, sizeof(stats));
	inbound_valid = false;
	inbound_fresh = false;
	outbound_dirty = false;
	outbound_seq = 0;

	return link_dma_init(link_dev);
}

void ctrl_io_begin_tick(void)
{
	struct ctrl_link_pkt latest;
	uint32_t rx_cycle;

	inbound_fresh = false;

	if (!link_dma_get_latest(&latest, &rx_cycle)) {
		stats.ticks_without_packet++;
		return;
	}

	/* The same packet as last tick is not a new sample. Saying so lets a
	 * held value be counted as a hold rather than silently pass for data.
	 */
	if (inbound_valid && latest.seq == inbound_last_seq) {
		stats.ticks_holding++;
		return;
	}

	if (inbound_valid) {
		const uint32_t gap = (uint32_t)(latest.seq - inbound_last_seq);

		if (gap > 1U) {
			stats.packets_skipped += gap - 1U;
		}
	}

	inbound = latest;
	inbound_last_seq = latest.seq;
	inbound_valid = true;
	inbound_fresh = true;
	stats.packets_taken++;
}

void ctrl_io_end_tick(void)
{
	if (!outbound_dirty) {
		return;
	}
	outbound_dirty = false;

	outbound.signal_count = CTRL_LINK_SIGNALS;
	outbound.seq = ++outbound_seq;
	outbound.tx_tick = (uint32_t)sys_clock_tick_get();

	if (link_dma_send(&outbound)) {
		stats.packets_sent++;
	} else {
		/* The previous transmit is still in flight, which at one packet
		 * per tick means the far side is not draining or the tick is
		 * faster than the link. The sample is dropped rather than
		 * queued: a control loop wants the newest value, not a backlog.
		 */
		stats.packets_dropped++;
	}
}

bool ctrl_io_read(uint16_t role, uint16_t index, float *out)
{
	if (role != CTRL_CHANNEL_LINK || index >= CTRL_LINK_SIGNALS) {
		return false;
	}
	if (!inbound_fresh) {
		/* No new sample. Writing nothing is what makes the Input block
		 * hold its last value - see ctrl_io.h.
		 */
		return false;
	}

	*out = inbound.signals[index];
	return true;
}

bool ctrl_io_write(uint16_t role, uint16_t index, float value)
{
	if (role != CTRL_CHANNEL_LINK || index >= CTRL_LINK_SIGNALS) {
		return false;
	}

	outbound.signals[index] = value;
	outbound_dirty = true;
	return true;
}

void ctrl_io_link_stats(struct ctrl_io_link_stats *out)
{
	*out = stats;
}
