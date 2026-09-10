/*
 * Consuming the link at tick rate, and measuring the two clocks against each
 * other while doing it.
 *
 * The control thread calls this once per tick with whatever the receive latch
 * currently holds. It answers three questions that a control loop has to
 * distinguish and a byte counter cannot:
 *
 *   - is this sample new, or am I holding the last one? A held sample is a
 *     zero-order hold, which is legitimate for one tick and a fault for six.
 *   - are the two boards' clocks running at the same rate? Nothing here shares
 *     a timebase, so the only way to see drift is to difference the local
 *     arrival times against the sender's own tick count and accumulate.
 *   - is the payload arithmetic still real? A NaN reaching the controller is
 *     not a bad sample, it is a poisoned state.
 *
 * There is no Zephyr in this file and no hardware. The caller supplies the
 * packet and the arrival cycle count; firmware/link/test exercises the
 * arithmetic natively, including the 2^32 wrap that a board would take an hour
 * to reach.
 */

#ifndef CTRL_LINK_LATCH_H_
#define CTRL_LINK_LATCH_H_

#include <stdbool.h>
#include <stdint.h>

#include "link_pkt.h"

enum link_latch_status {
	LATCH_OK = 0,	/* a new sample, arithmetic intact */
	LATCH_SLIP,	/* nothing new; the previous sample is held */
	LATCH_OVERRUN,	/* more than one packet elapsed since the last step */
	LATCH_FAULT,	/* non-finite payload, or too many slips in a row */
};

/* Slips tolerated in a row before the link is called broken. Five ticks is
 * half a millisecond here - long enough that a single late packet is not a
 * fault, short enough that a controller does not integrate half a second of
 * stale input before anyone notices.
 */
#define LINK_LATCH_MAX_SLIPS 5

struct link_latch {
	/* This board's nominal cycles per control tick. The far side's tick
	 * count is converted with it, so the two ends must agree on the tick
	 * RATE; what they are not assumed to agree on is the crystal, which is
	 * exactly what skew_cycles ends up measuring.
	 */
	uint32_t cycles_per_tick;

	bool primed;			/* a previous sample exists */
	uint32_t last_seq;
	uint32_t last_rx_cycle;
	uint32_t last_tx_tick;

	float held[CTRL_LINK_SIGNALS];	/* the zero-order hold */
	uint16_t held_count;

	int64_t skew_cycles;		/* local cycles minus predicted, cumulative */

	uint32_t slips;			/* consecutive */
	uint32_t total_slips;
	uint32_t overruns;
	uint32_t faults;
	uint32_t skew_gaps;		/* intervals too long to attribute */
};

void link_latch_init(struct link_latch *l, uint32_t cycles_per_tick);

/* One control tick's worth of consumption.
 *
 * `pkt` and `rx_cycle` are whatever link_dma_get_latest() returned, and `have`
 * is whether it returned anything at all. A packet whose seq matches the last
 * one is not new, and is treated exactly like no packet: the caller does not
 * have to track that itself.
 *
 * `signals_out` always receives CTRL_LINK_SIGNALS floats - the new sample on
 * LATCH_OK and LATCH_OVERRUN, the held one otherwise - so the control path
 * never has to branch on the status to get a usable input. On LATCH_FAULT it
 * receives the last known-good sample rather than the bad one.
 *
 * Any of the out parameters may be NULL.
 */
enum link_latch_status link_latch_step(struct link_latch *l,
				       const struct ctrl_link_pkt *pkt,
				       uint32_t rx_cycle, bool have,
				       float *signals_out, uint32_t *seq_out,
				       int64_t *skew_cycles_out);

#endif /* CTRL_LINK_LATCH_H_ */
