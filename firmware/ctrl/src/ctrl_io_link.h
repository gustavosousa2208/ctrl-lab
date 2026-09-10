/*
 * The link-backed implementation of ctrl_io.h, and the little it exposes
 * beyond that interface.
 *
 * Only main.c needs this: the HAL has to be brought up before a plan is armed,
 * and the run's report wants to say how the link behaved. The control core
 * itself sees nothing but ctrl_io.h.
 */

#ifndef CTRL_IO_LINK_H
#define CTRL_IO_LINK_H

#include <stdbool.h>
#include <stdint.h>

/* Per-tick outcomes, all free-running. None of these is an error the control
 * step reports - a late packet is a held sample, not a fault - so this is the
 * only place a starved link becomes visible.
 */
struct ctrl_io_link_stats {
	uint32_t packets_taken;		/* ticks that got a new inbound sample */
	uint32_t ticks_holding;		/* the latch held the same packet as last tick */
	uint32_t ticks_without_packet;	/* nothing latched at all */
	uint32_t packets_skipped;	/* seq gaps: samples that were never seen */
	uint32_t packets_sent;
	uint32_t packets_dropped;	/* a transmit was still in flight */
};

/* Arms the link. Must run before ctrl_arm(), and returns false loudly if the
 * UART or its DMA is not ready.
 */
bool ctrl_io_link_init(void);

void ctrl_io_link_stats(struct ctrl_io_link_stats *out);

#endif /* CTRL_IO_LINK_H */
