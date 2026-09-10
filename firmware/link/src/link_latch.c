/* Tick-rate consumption of the link latch. See link_latch.h. */

#include <math.h>
#include <string.h>

#include "link_latch.h"

void link_latch_init(struct link_latch *l, uint32_t cycles_per_tick)
{
	memset(l, 0, sizeof(*l));
	l->cycles_per_tick = cycles_per_tick;
}

static void emit(const struct link_latch *l, float *signals_out, uint32_t *seq_out,
		 int64_t *skew_out)
{
	if (signals_out != NULL) {
		memcpy(signals_out, l->held, sizeof(l->held));
	}
	if (seq_out != NULL) {
		*seq_out = l->last_seq;
	}
	if (skew_out != NULL) {
		*skew_out = l->skew_cycles;
	}
}

/* Everything the sender said happened between the last packet and this one,
 * expressed in this board's cycles, differenced against what this board's own
 * clock counted.
 *
 * All three subtractions are modular on purpose. The cycle counter is 32 bits
 * and wraps every 20 seconds at 216 MHz; the sender's tick counter wraps too.
 * Taking each difference in uint32 arithmetic and only then interpreting it as
 * signed is what makes the wrap a non-event - there is no branch for it here
 * because there does not need to be one, and a test pins that.
 */
static void accumulate_skew(struct link_latch *l, const struct ctrl_link_pkt *pkt,
			    uint32_t rx_cycle)
{
	const uint32_t tick_delta = (uint32_t)(pkt->tx_tick - l->last_tx_tick);

	if (tick_delta == 0U) {
		return;		/* same tick twice; nothing to attribute */
	}

	/* A gap long enough to overflow the conversion cannot be attributed to
	 * drift - at 24000 cycles a tick that is about eighteen seconds, which
	 * is a link that stopped rather than a clock that wandered. Counted and
	 * skipped, so one outage does not poison the accumulator for good.
	 */
	if (tick_delta > UINT32_MAX / l->cycles_per_tick) {
		l->skew_gaps++;
		return;
	}

	const uint32_t rx_delta = (uint32_t)(rx_cycle - l->last_rx_cycle);
	const uint32_t tx_delta = tick_delta * l->cycles_per_tick;

	l->skew_cycles += (int32_t)(rx_delta - tx_delta);
}

static bool payload_is_finite(const struct ctrl_link_pkt *pkt)
{
	const uint16_t count = pkt->signal_count;

	if (count > CTRL_LINK_SIGNALS) {
		return false;
	}

	/* Every slot is checked, not just the declared ones. A signal_count of
	 * two does not make signals[3] harmless: it is copied into the hold and
	 * a later packet declaring four would hand a stale NaN to the
	 * controller as if it were data.
	 */
	for (unsigned int i = 0; i < CTRL_LINK_SIGNALS; i++) {
		if (!isfinite(pkt->signals[i])) {
			return false;
		}
	}
	return true;
}

enum link_latch_status link_latch_step(struct link_latch *l,
				       const struct ctrl_link_pkt *pkt,
				       uint32_t rx_cycle, bool have,
				       float *signals_out, uint32_t *seq_out,
				       int64_t *skew_cycles_out)
{
	/* "Nothing arrived" and "the same packet as last tick" are the same
	 * event to a control loop: there is no new information either way.
	 */
	const bool fresh = have && (!l->primed || pkt->seq != l->last_seq);

	if (!fresh) {
		l->slips++;
		l->total_slips++;
		emit(l, signals_out, seq_out, skew_cycles_out);

		if (l->slips >= LINK_LATCH_MAX_SLIPS) {
			l->faults++;
			return LATCH_FAULT;
		}
		return LATCH_SLIP;
	}

	if (!payload_is_finite(pkt)) {
		/* Deliberately not held. The previous sample is stale but
		 * finite, and a controller can survive stale input; it cannot
		 * survive a NaN, which propagates into the state and never
		 * leaves. The slip counter is left alone - this is a fault in
		 * its own right, not a missing packet.
		 */
		l->faults++;
		emit(l, signals_out, seq_out, skew_cycles_out);
		return LATCH_FAULT;
	}

	const uint32_t seq_delta = (uint32_t)(pkt->seq - l->last_seq);
	bool overrun = false;

	if (l->primed) {
		accumulate_skew(l, pkt, rx_cycle);
		if (seq_delta > 1U) {
			l->overruns++;
			overrun = true;
		}
	}

	memcpy(l->held, pkt->signals, sizeof(l->held));
	l->held_count = pkt->signal_count;
	l->last_seq = pkt->seq;
	l->last_rx_cycle = rx_cycle;
	l->last_tx_tick = pkt->tx_tick;
	l->primed = true;
	l->slips = 0;

	emit(l, signals_out, seq_out, skew_cycles_out);
	return overrun ? LATCH_OVERRUN : LATCH_OK;
}
