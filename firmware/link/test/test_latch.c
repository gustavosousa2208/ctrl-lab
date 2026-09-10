/*
 * Host tests for the latch consumer (src/link_latch.c).
 *
 * The interesting cases are the ones a board reaches slowly or not at all: the
 * cycle counter wrapping past 2^32 takes twenty seconds at 216 MHz and the
 * sender's tick counter takes five days, a clock drifting far enough to see
 * takes minutes, and a NaN in the payload takes a fault upstream that nobody
 * wants to arrange twice. All of them are a line here.
 */

#include <math.h>
#include <stdio.h>
#include <string.h>

#include "link_latch.h"
#include "link_pkt.h"

/* The F767, which is the end that consumes: 216 MHz, 10 kHz control tick. */
#define CYCLES_PER_TICK 21600U

static int failures;

static void check(bool ok, const char *what)
{
	printf("%-58s %s\n", what, ok ? "ok" : "FAIL");
	if (!ok) {
		failures++;
	}
}

static void make_pkt(struct ctrl_link_pkt *pkt, uint32_t seq, uint32_t tx_tick)
{
	memset(pkt, 0, sizeof(*pkt));
	pkt->magic = CTRL_LINK_MAGIC;
	pkt->signal_count = CTRL_LINK_SIGNALS;
	pkt->seq = seq;
	pkt->tx_tick = tx_tick;
	for (unsigned int i = 0; i < CTRL_LINK_SIGNALS; i++) {
		pkt->signals[i] = (float)seq + (float)i;
	}
}

/* --- tests --------------------------------------------------------------- */

static void test_steady_stream(void)
{
	struct link_latch l;
	struct ctrl_link_pkt pkt;
	enum link_latch_status st = LATCH_FAULT;
	float sig[CTRL_LINK_SIGNALS];
	int64_t skew = -1;
	uint32_t cycle = 1000U;
	bool all_ok = true;

	link_latch_init(&l, CYCLES_PER_TICK);

	for (uint32_t k = 1; k <= 100U; k++) {
		make_pkt(&pkt, k, k);
		st = link_latch_step(&l, &pkt, cycle, true, sig, NULL, &skew);
		if (st != LATCH_OK) {
			all_ok = false;
		}
		cycle += CYCLES_PER_TICK;
	}

	check(all_ok, "steady stream: every step reports OK");
	check(sig[0] == 100.0f, "steady stream: the newest sample is the one emitted");
	check(skew == 0, "steady stream: clocks in step means zero accumulated skew");
	check(l.total_slips == 0 && l.overruns == 0, "steady stream: no slips, no overruns");
}

/* The clocks are never actually equal. One cycle per tick either way is a
 * 46 ppm crystal difference, which is ordinary, and the point of the
 * accumulator is that it adds up into something visible rather than hiding in
 * the rounding.
 */
static void test_drift_accumulates(void)
{
	struct link_latch l;
	struct ctrl_link_pkt pkt;
	float sig[CTRL_LINK_SIGNALS];
	int64_t skew = 0;
	uint32_t cycle = 0U;

	link_latch_init(&l, CYCLES_PER_TICK);

	for (uint32_t k = 1; k <= 1000U; k++) {
		make_pkt(&pkt, k, k);
		link_latch_step(&l, &pkt, cycle, true, sig, NULL, &skew);
		cycle += CYCLES_PER_TICK + 1U;	/* local clock one cycle fast */
	}

	/* 999 differenced intervals, one cycle each. */
	check(skew == 999, "drift: one cycle per tick accumulates to 999 over 1000");

	link_latch_init(&l, CYCLES_PER_TICK);
	cycle = 0U;
	for (uint32_t k = 1; k <= 1000U; k++) {
		make_pkt(&pkt, k, k);
		link_latch_step(&l, &pkt, cycle, true, sig, NULL, &skew);
		cycle += CYCLES_PER_TICK - 1U;	/* and the other way */
	}
	check(skew == -999, "drift: the sign follows which clock is faster");
}

/* The case the plan calls out by name: the 32-bit cycle counter rolling over
 * mid-measurement must not produce a four-billion-cycle step.
 */
static void test_cycle_wrap(void)
{
	struct link_latch l;
	struct ctrl_link_pkt pkt;
	float sig[CTRL_LINK_SIGNALS];
	int64_t skew = 0;

	link_latch_init(&l, 80U);	/* so a tick IS the 80 cycles under test */

	make_pkt(&pkt, 1, 1);
	link_latch_step(&l, &pkt, 0xffffffe0U, true, sig, NULL, &skew);

	make_pkt(&pkt, 2, 2);
	link_latch_step(&l, &pkt, 0x00000030U, true, sig, NULL, &skew);

	check(skew == 0, "cycle wrap: 0xffffffe0 -> 0x00000030 is 80 cycles, not 2^32");

	/* And once more with the local clock genuinely two cycles long, to be
	 * sure the zero above is arithmetic rather than a swallowed result.
	 */
	link_latch_init(&l, 80U);
	make_pkt(&pkt, 1, 1);
	link_latch_step(&l, &pkt, 0xffffffe0U, true, sig, NULL, &skew);
	make_pkt(&pkt, 2, 2);
	link_latch_step(&l, &pkt, 0x00000032U, true, sig, NULL, &skew);
	check(skew == 2, "cycle wrap: a real two-cycle error still reads as two");
}

static void test_tick_wrap(void)
{
	struct link_latch l;
	struct ctrl_link_pkt pkt;
	float sig[CTRL_LINK_SIGNALS];
	int64_t skew = 0;

	link_latch_init(&l, CYCLES_PER_TICK);

	make_pkt(&pkt, 1, 0xffffffffU);
	link_latch_step(&l, &pkt, 0U, true, sig, NULL, &skew);

	make_pkt(&pkt, 2, 0x00000000U);
	link_latch_step(&l, &pkt, CYCLES_PER_TICK, true, sig, NULL, &skew);

	check(skew == 0, "tick wrap: the sender's tick counter may roll over too");
	check(l.skew_gaps == 0, "tick wrap: not mistaken for an eighteen-second gap");
}

static void test_slip_holds_then_faults(void)
{
	struct link_latch l;
	struct ctrl_link_pkt pkt;
	float sig[CTRL_LINK_SIGNALS];
	enum link_latch_status st;
	bool held_every_time = true;

	link_latch_init(&l, CYCLES_PER_TICK);

	make_pkt(&pkt, 7, 7);
	link_latch_step(&l, &pkt, 1000U, true, sig, NULL, NULL);

	/* The same packet again is not a new packet. */
	for (int i = 0; i < LINK_LATCH_MAX_SLIPS - 1; i++) {
		memset(sig, 0, sizeof(sig));
		st = link_latch_step(&l, &pkt, 1000U, true, sig, NULL, NULL);
		if (st != LATCH_SLIP || sig[1] != 8.0f) {
			held_every_time = false;
		}
	}

	check(held_every_time, "slip: a repeated packet holds the previous sample");
	check(l.total_slips == LINK_LATCH_MAX_SLIPS - 1, "slip: counted, one per tick");

	st = link_latch_step(&l, &pkt, 1000U, true, sig, NULL, NULL);
	check(st == LATCH_FAULT, "slip: the fifth consecutive slip is a fault");

	/* And a fresh packet clears it. */
	make_pkt(&pkt, 8, 8);
	st = link_latch_step(&l, &pkt, 1000U + CYCLES_PER_TICK, true, sig, NULL, NULL);
	check(st == LATCH_OK && l.slips == 0, "slip: a new packet resets the run");
}

static void test_nothing_at_all(void)
{
	struct link_latch l;
	struct ctrl_link_pkt pkt;
	float sig[CTRL_LINK_SIGNALS];
	enum link_latch_status st;

	link_latch_init(&l, CYCLES_PER_TICK);
	memset(&pkt, 0, sizeof(pkt));

	st = link_latch_step(&l, &pkt, 0U, false, sig, NULL, NULL);
	check(st == LATCH_SLIP, "no packet ever: slips rather than reporting a sample");
	check(sig[0] == 0.0f, "no packet ever: the hold is zero, not garbage");
}

static void test_overrun(void)
{
	struct link_latch l;
	struct ctrl_link_pkt pkt;
	float sig[CTRL_LINK_SIGNALS];
	enum link_latch_status st;

	link_latch_init(&l, CYCLES_PER_TICK);

	make_pkt(&pkt, 1, 1);
	link_latch_step(&l, &pkt, 0U, true, sig, NULL, NULL);

	/* Three packets sent, one step taken: two were missed. */
	make_pkt(&pkt, 4, 4);
	st = link_latch_step(&l, &pkt, 3U * CYCLES_PER_TICK, true, sig, NULL, NULL);

	check(st == LATCH_OVERRUN, "overrun: a seq jump greater than one is reported");
	check(sig[0] == 4.0f, "overrun: the sample is still taken, not discarded");
	check(l.overruns == 1, "overrun: counted");
}

static void test_non_finite_payload(void)
{
	struct link_latch l;
	struct ctrl_link_pkt pkt;
	float sig[CTRL_LINK_SIGNALS];
	enum link_latch_status st;
	bool all_faulted = true;

	const float poison[] = { NAN, INFINITY, -INFINITY };

	for (unsigned int p = 0; p < 3U; p++) {
		link_latch_init(&l, CYCLES_PER_TICK);

		make_pkt(&pkt, 1, 1);
		link_latch_step(&l, &pkt, 0U, true, sig, NULL, NULL);

		make_pkt(&pkt, 2, 2);
		pkt.signals[2] = poison[p];
		memset(sig, 0, sizeof(sig));
		st = link_latch_step(&l, &pkt, CYCLES_PER_TICK, true, sig, NULL, NULL);

		if (st != LATCH_FAULT || sig[0] != 1.0f || !isfinite(sig[2])) {
			all_faulted = false;
		}
	}

	check(all_faulted, "non-finite: NaN and both infinities fault, hold stays finite");

	/* A slot past signal_count is still poison: it reaches the hold. */
	link_latch_init(&l, CYCLES_PER_TICK);
	make_pkt(&pkt, 1, 1);
	pkt.signal_count = 2;
	pkt.signals[3] = NAN;
	st = link_latch_step(&l, &pkt, 0U, true, sig, NULL, NULL);
	check(st == LATCH_FAULT, "non-finite: an undeclared slot is checked too");

	/* And a signal_count that cannot be honoured is a fault, not a read
	 * past the end of the array.
	 */
	link_latch_init(&l, CYCLES_PER_TICK);
	make_pkt(&pkt, 1, 1);
	pkt.signal_count = CTRL_LINK_SIGNALS + 1U;
	st = link_latch_step(&l, &pkt, 0U, true, sig, NULL, NULL);
	check(st == LATCH_FAULT, "non-finite: an impossible signal_count is refused");
}

/* An outage long enough to overflow the tick conversion must be skipped, not
 * accumulated - one dead link should not poison the skew figure for good.
 */
static void test_long_gap_not_attributed(void)
{
	struct link_latch l;
	struct ctrl_link_pkt pkt;
	float sig[CTRL_LINK_SIGNALS];
	int64_t skew = 0;

	link_latch_init(&l, CYCLES_PER_TICK);

	make_pkt(&pkt, 1, 1);
	link_latch_step(&l, &pkt, 0U, true, sig, NULL, &skew);

	make_pkt(&pkt, 2, 1U + (UINT32_MAX / CYCLES_PER_TICK) + 1U);
	link_latch_step(&l, &pkt, 12345U, true, sig, NULL, &skew);

	check(skew == 0, "long gap: not folded into the accumulator");
	check(l.skew_gaps == 1, "long gap: counted so it is not silent");

	/* And the link keeps working afterwards. */
	make_pkt(&pkt, 3, 3U + (UINT32_MAX / CYCLES_PER_TICK));
	link_latch_step(&l, &pkt, 12345U + CYCLES_PER_TICK + 5U, true, sig, NULL, &skew);
	check(skew == 5, "long gap: measurement resumes from the next interval");
}

int main(void)
{
	test_steady_stream();
	test_drift_accumulates();
	test_cycle_wrap();
	test_tick_wrap();
	test_slip_holds_then_faults();
	test_nothing_at_all();
	test_overrun();
	test_non_finite_payload();
	test_long_gap_not_attributed();

	printf("\n%s\n", failures == 0 ? "all ok" : "FAILURES");
	return failures == 0 ? 0 : 1;
}
