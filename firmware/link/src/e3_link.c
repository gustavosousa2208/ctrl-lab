/*
 * ctrl-lab link validation - stage E3.
 *
 * E2 established that the wire and the framing are sound, and that a polled
 * receive only survives the 10 kHz kernel tick if interrupts are locked for
 * the whole frame. This is the same link with the receive path replaced: DMA
 * in both directions, interrupts enabled throughout, and the tick running.
 *
 * It answers one question - does the transport still lose nothing when nothing
 * is protecting it - and reports the numbers stage E3 needs on top:
 *
 *   loss        every packet is counted at both ends and CRC-checked on
 *               arrival. Zero bad frames and no resyncs after the handshake is
 *               the pass condition.
 *   latency     round trip measured on the initiator's clock, with the wire
 *               time and the responder's own turnaround subtracted, which is
 *               what makes it comparable to E2's ping figure.
 *   skew        the two crystals differenced through the packets themselves.
 *               Nothing here shares a timebase, so this is the only way to see
 *               it - see link_latch.c.
 *
 * Both ends run the SAME loop shape as a control step would: take whatever the
 * latch holds, act, move on. Nothing blocks, nothing spins on the UART, and
 * there is no irq_lock() anywhere in this file. That is the point.
 */

#include <zephyr/kernel.h>
#include <zephyr/device.h>
#include <zephyr/drivers/uart.h>
#include <zephyr/sys/printk.h>
#include <zephyr/timing/timing.h>
#include <string.h>

#include "link_dma.h"
#include "link_frame.h"
#include "link_latch.h"
#include "link_pkt.h"

#define LINK_NODE DT_ALIAS(link_uart)
#if !DT_NODE_EXISTS(LINK_NODE)
#error "no link-uart alias - add one in boards/<board>.overlay"
#endif

static const struct device *const link = DEVICE_DT_GET(LINK_NODE);

/* Taken from the devicetree rather than the CMake variable, so it is the baud
 * the UART was actually configured with and not the one that was requested.
 */
#define LINK_BAUD DT_PROP(LINK_NODE, current_speed)

/* 8N1, so ten bits per byte on the wire. */
#define PKT_WIRE_NS ((uint32_t)(((uint64_t)CTRL_LINK_PKT_LEN * 10ULL * 1000000000ULL) \
			        / (uint64_t)LINK_BAUD))

#define BURST_PACKETS   10000U
#define PKT_TIMEOUT_US  5000U	/* per packet; ~45x the round trip at 6 Mbaud */
#define MAX_CONSEC_DROP 50U	/* abort rather than grind through a dead link */

/* Handshake sequence numbers, kept far away from the burst's 1..10000 so a
 * late handshake echo can never be mistaken for a burst packet.
 */
#define HANDSHAKE_SEQ_BASE 0xf0000000U
#define HANDSHAKE_TRIES    100U

static uint32_t cycles_per_us;

#ifdef LINK_INITIATOR
/* Waits for the latch to hold a packet with this exact seq.
 *
 * A spin, deliberately, and a bounded one - but note what it is NOT doing: it
 * never touches the UART, never disables anything, and the DMA keeps filling
 * the ring the whole time it runs. A late thread here costs latency and not
 * data, which is the entire difference between this stage and E2.
 */
static bool wait_for_seq(uint32_t want, struct ctrl_link_pkt *out, uint32_t *rx_cycle,
			 uint32_t timeout_us)
{
	const uint32_t deadline = k_cycle_get_32() + timeout_us * cycles_per_us;

	do {
		if (link_dma_get_latest(out, rx_cycle) && out->seq == want) {
			return true;
		}
	} while ((int32_t)(k_cycle_get_32() - deadline) < 0);

	return false;
}
#endif /* LINK_INITIATOR */

#ifdef LINK_INITIATOR
/* What the instruments cost, before any of it is blamed on the link.
 *
 * The first decomposed run put 25.5 us into handing a packet to the transmit
 * DMA and 25.7 us into noticing one had arrived - two operations with almost
 * nothing in common, agreeing to three digits. Numbers that similar are rarely
 * about the thing being measured, so this measures the measuring: the DWT
 * counter, read through the timing API, timing the SysTick-backed clock the
 * link path actually calls.
 */
static void measure_instrument_cost(void)
{
	const unsigned int reps = 1000;
	struct ctrl_link_pkt scratch;
	uint32_t scratch_cycle;
	timing_t a, b;

	timing_init();
	timing_start();

	a = timing_counter_get();
	for (unsigned int i = 0; i < reps; i++) {
		(void)k_cycle_get_32();
	}
	b = timing_counter_get();
	printk("cost        k_cycle_get_32 %u cycles\n",
	       (uint32_t)(timing_cycles_get(&a, &b) / reps));

	a = timing_counter_get();
	for (unsigned int i = 0; i < reps; i++) {
		(void)link_dma_get_latest(&scratch, &scratch_cycle);
	}
	b = timing_counter_get();
	printk("cost        link_dma_get_latest %u cycles\n",
	       (uint32_t)(timing_cycles_get(&a, &b) / reps));

	a = timing_counter_get();
	for (unsigned int i = 0; i < reps; i++) {
		(void)sys_clock_tick_get();
	}
	b = timing_counter_get();
	printk("cost        sys_clock_tick_get %u cycles\n",
	       (uint32_t)(timing_cycles_get(&a, &b) / reps));

	/* The CPU half of link_dma_send: the copy into the nocache staging
	 * buffer and the CRC over it. Whatever the send costs beyond this is
	 * the driver arming the transmit DMA, and that subtraction is the
	 * whole point of measuring it.
	 */
	memset(&scratch, 0, sizeof(scratch));
	a = timing_counter_get();
	for (unsigned int i = 0; i < reps; i++) {
		link_frame_stamp(&scratch);
	}
	b = timing_counter_get();
	printk("cost        stage+crc a packet %u cycles\n",
	       (uint32_t)(timing_cycles_get(&a, &b) / reps));

	timing_stop();
}
#endif /* LINK_INITIATOR */

static void print_banner(const char *role)
{
	printk("\nctrl-lab link %s - stage E3, DMA receive\n", role);
	printk("board " CONFIG_BOARD_TARGET ", link %s at %u baud\n", link->name,
	       (unsigned int)LINK_BAUD);
	printk("core %u Hz, kernel tick %u Hz, dcache %s\n",
	       sys_clock_hw_cycles_per_sec(), CONFIG_SYS_CLOCK_TICKS_PER_SEC,
	       IS_ENABLED(CONFIG_DCACHE) ? "on" : "off");
	printk("packet %u bytes, %u ns on the wire\n", (unsigned int)CTRL_LINK_PKT_LEN,
	       PKT_WIRE_NS);
}

static void print_link_stats(const char *who)
{
	struct link_dma_stats s;

	link_dma_get_stats(&s);
	printk("%s slots=%u ok=%u bad=%u short=%u resync=%u realign=%u "
	       "uart_err=%u rearm=%u\n", who, s.rx_slots, s.rx_ok, s.rx_bad,
	       s.rx_short, s.resyncs, s.realigns, s.rx_errors, s.rx_restarts);
	printk("%s tx_done=%u tx_rejected=%u\n", who, s.tx_done, s.tx_rejected);
	printk("%s rx_callback first=%u worst=%u mean=%u cycles (after the first)\n",
	       who, s.rx_first_cycles, s.rx_worst_cycles,
	       s.rx_slots > 1U ? (uint32_t)(s.rx_total_cycles / (s.rx_slots - 1U)) : 0U);
}

#ifdef LINK_RESPONDER

/* The H743 end. Echoes every packet it validates, straight back, as fast as it
 * can - which at one DMA transmit per received packet is the same shape a
 * plant model would have: consume the newest sample, produce one output.
 *
 * Never returns. On this part letting Zephyr idle into WFI also takes the core
 * off the SWD debug bus, which is how RTT is read - see firmware/BRINGUP.md.
 */
int main(void)
{
	print_banner("responder");

	if (!link_dma_init(link)) {
		printk("FAIL: link_dma_init\n");
		return 0;
	}

	cycles_per_us = sys_clock_hw_cycles_per_sec() / 1000000U;

	struct ctrl_link_pkt pkt, reply;
	uint32_t rx_cycle;
	bool primed = false;
	uint32_t last_seq = 0;

	uint32_t echoed = 0, gaps = 0, tx_fail = 0;
	uint32_t turn_best = UINT32_MAX, turn_worst = 0;
	uint64_t turn_total = 0;
	uint32_t quiet_since = k_cycle_get_32();
	bool reported = true;

	printk("listening\n");

	while (1) {
		if (!link_dma_get_latest(&pkt, &rx_cycle) ||
		    (primed && pkt.seq == last_seq)) {
			/* Report once the stream has been quiet for a while.
			 * Deferred rather than printed as it goes: printk is
			 * slow, and although the DMA would keep receiving
			 * through it - which E2's polled loop could not - a
			 * console write in the middle of a measurement still
			 * shows up in the numbers.
			 */
			if (!reported && echoed > 0U &&
			    (uint32_t)(k_cycle_get_32() - quiet_since) >
				    200000U * cycles_per_us) {
				printk("\nechoed %u, seq gaps %u, tx refused %u\n",
				       echoed, gaps, tx_fail);
				printk("turnaround min=%u mean=%u max=%u cycles "
				       "(%u ns worst)\n", turn_best,
				       (uint32_t)(turn_total / echoed), turn_worst,
				       (uint32_t)((uint64_t)turn_worst * 1000000000ULL /
						  sys_clock_hw_cycles_per_sec()));
				print_link_stats("responder");
				reported = true;
			}
			continue;
		}

		if (primed) {
			const uint32_t delta = (uint32_t)(pkt.seq - last_seq);

			/* Only within the burst; a handshake seq sitting far
			 * away from it is not a gap of four billion packets.
			 */
			if (delta > 1U && delta < BURST_PACKETS) {
				gaps += delta - 1U;
			}
		}
		last_seq = pkt.seq;
		primed = true;

		/* Echo. The payload goes back untouched so the initiator can
		 * check the round trip end to end; tx_tick carries THIS
		 * board's kernel tick, which is what lets the far side
		 * difference the two crystals.
		 */
		reply = pkt;
		reply.tx_tick = (uint32_t)sys_clock_tick_get();
		if (!link_dma_send(&reply)) {
			tx_fail++;
		}

		const uint32_t turn = k_cycle_get_32() - rx_cycle;

		turn_best = MIN(turn_best, turn);
		turn_worst = MAX(turn_worst, turn);
		turn_total += turn;
		echoed++;

		quiet_since = k_cycle_get_32();
		reported = false;
	}
}

#else /* LINK_INITIATOR */

int main(void)
{
	print_banner("initiator");

	if (!link_dma_init(link)) {
		printk("FAIL: link_dma_init\n");
		return 0;
	}

	cycles_per_us = sys_clock_hw_cycles_per_sec() / 1000000U;

	measure_instrument_cost();

	struct ctrl_link_pkt pkt, echo;
	uint32_t rx_cycle;

	/* Let the far side finish booting and arm its DMA. Both boards are
	 * reset by their own probe and there is no ordering between them, so
	 * without this the first packets land in a receiver that is not
	 * listening yet - and with a fixed-size framing, a stream that starts
	 * mid-packet costs a resync rather than a byte.
	 */
	k_sleep(K_MSEC(200));

	/* Handshake. One packet out, the same packet back, and the burst does
	 * not start until that has happened: it proves the far side is up, the
	 * baud rates agree, and both framings are aligned before anything is
	 * being counted.
	 */
	bool up = false;

	for (uint32_t try = 0; try < HANDSHAKE_TRIES && !up; try++) {
		memset(&pkt, 0, sizeof(pkt));
		pkt.signal_count = CTRL_LINK_SIGNALS;
		pkt.seq = HANDSHAKE_SEQ_BASE + try;
		pkt.tx_tick = (uint32_t)sys_clock_tick_get();

		if (link_dma_send(&pkt)) {
			up = wait_for_seq(pkt.seq, &echo, &rx_cycle, 10000U);
		} else {
			k_sleep(K_MSEC(1));
		}
	}

	if (!up) {
		printk("\nFAIL: no answer after %u handshake attempts.\n",
		       HANDSHAKE_TRIES);
		printk("      Check TX/RX are crossed, GND is shared, and that\n");
		printk("      both ends are built with the same LINK_BAUD.\n");
		print_link_stats("initiator");
		return 0;
	}

	printk("handshake ok, sending %u packets\n", BURST_PACKETS);

	struct link_latch latch;

	link_latch_init(&latch, sys_clock_hw_cycles_per_sec() /
				CONFIG_SYS_CLOCK_TICKS_PER_SEC);

	uint32_t answered = 0, dropped = 0, consec_drop = 0, payload_bad = 0;
	uint32_t tx_refused = 0, fault = 0, overrun = 0;
	uint32_t rtt_best = UINT32_MAX, rtt_worst = 0;
	uint64_t rtt_total = 0;

	/* The round trip on its own is a number with nowhere to go. Split into
	 * the three pieces that can actually be acted on:
	 *
	 *   setup   handing the packet to the transmit DMA - a copy into the
	 *           nocache staging buffer, a CRC, and the driver reprogramming
	 *           the stream.
	 *   flight  send started to the arrival stamp the receive ISR wrote.
	 *           This is the wire in both directions plus everything the far
	 *           side did, and it is where a surprise would live.
	 *   detect  arrival stamp to this loop noticing, which is the price of
	 *           polling the latch instead of being called by it.
	 */
	uint32_t setup_best = UINT32_MAX, flight_best = UINT32_MAX, detect_best = UINT32_MAX;
	uint64_t setup_total = 0, flight_total = 0, detect_total = 0;
	int64_t skew = 0;
	const uint32_t t_start = k_cycle_get_32();

	for (uint32_t k = 1; k <= BURST_PACKETS; k++) {
		memset(&pkt, 0, sizeof(pkt));
		pkt.signal_count = CTRL_LINK_SIGNALS;
		pkt.seq = k;
		pkt.tx_tick = (uint32_t)sys_clock_tick_get();
		for (unsigned int i = 0; i < CTRL_LINK_SIGNALS; i++) {
			pkt.signals[i] = (float)k + (float)i * 0.25f;
		}

		const uint32_t t0 = k_cycle_get_32();

		if (!link_dma_send(&pkt)) {
			tx_refused++;
			consec_drop++;
			if (consec_drop >= MAX_CONSEC_DROP) {
				break;
			}
			continue;
		}

		const uint32_t t1 = k_cycle_get_32();

		if (!wait_for_seq(k, &echo, &rx_cycle, PKT_TIMEOUT_US)) {
			dropped++;
			if (++consec_drop >= MAX_CONSEC_DROP) {
				break;
			}
			continue;
		}

		const uint32_t t2 = k_cycle_get_32();
		const uint32_t rtt = t2 - t0;

		consec_drop = 0;
		answered++;
		rtt_best = MIN(rtt_best, rtt);
		rtt_worst = MAX(rtt_worst, rtt);
		rtt_total += rtt;

		const uint32_t setup = t1 - t0;
		const uint32_t flight = rx_cycle - t1;
		const uint32_t detect = t2 - rx_cycle;

		setup_best = MIN(setup_best, setup);
		flight_best = MIN(flight_best, flight);
		detect_best = MIN(detect_best, detect);
		setup_total += setup;
		flight_total += flight;
		detect_total += detect;

		/* The payload came back through two DMA controllers and two
		 * CRCs. Checking it here is what makes "zero byte loss" a
		 * statement about the bytes rather than about the frame count.
		 */
		for (unsigned int i = 0; i < CTRL_LINK_SIGNALS; i++) {
			if (echo.signals[i] != pkt.signals[i]) {
				payload_bad++;
				break;
			}
		}

		switch (link_latch_step(&latch, &echo, rx_cycle, true, NULL, NULL, &skew)) {
		case LATCH_OVERRUN:
			overrun++;
			break;
		case LATCH_FAULT:
			fault++;
			break;
		default:
			break;
		}
	}

	const uint32_t elapsed = k_cycle_get_32() - t_start;
	const uint32_t hz = sys_clock_hw_cycles_per_sec();

	printk("\nsent        %u\n", answered + dropped + tx_refused);
	printk("answered    %u\n", answered);
	printk("dropped     %u (timeout %u us), tx refused %u\n", dropped,
	       PKT_TIMEOUT_US, tx_refused);
	printk("payload_bad %u\n", payload_bad);
	printk("latch       overrun %u, fault %u, slips %u\n", overrun, fault,
	       latch.total_slips);

	if (answered > 0U) {
		const uint32_t mean = (uint32_t)(rtt_total / answered);
		const uint32_t wire2 = 2U * PKT_WIRE_NS;

		printk("\nrtt_cycles  min=%u mean=%u max=%u spread=%u\n", rtt_best,
		       mean, rtt_worst, rtt_worst - rtt_best);
		printk("rtt_ns      min=%u mean=%u max=%u\n",
		       (uint32_t)((uint64_t)rtt_best * 1000000000ULL / hz),
		       (uint32_t)((uint64_t)mean * 1000000000ULL / hz),
		       (uint32_t)((uint64_t)rtt_worst * 1000000000ULL / hz));
		printk("wire_ns     %u both directions (%u bytes each way)\n", wire2,
		       (unsigned int)CTRL_LINK_PKT_LEN);
		printk("            min / mean, in ns:\n");
		printk("  setup     %u / %u  handing it to the transmit DMA\n",
		       (uint32_t)((uint64_t)setup_best * 1000000000ULL / hz),
		       (uint32_t)(setup_total * 1000000000ULL / answered / hz));
		printk("  flight    %u / %u  wire both ways plus the far side\n",
		       (uint32_t)((uint64_t)flight_best * 1000000000ULL / hz),
		       (uint32_t)(flight_total * 1000000000ULL / answered / hz));
		printk("  detect    %u / %u  arrival stamp to this loop noticing\n",
		       (uint32_t)((uint64_t)detect_best * 1000000000ULL / hz),
		       (uint32_t)(detect_total * 1000000000ULL / answered / hz));

		/* What is left after the wire is software: two DMA arm/complete
		 * paths, two CRCs, the latch on each side, and the responder's
		 * turnaround, which it reports on its own console. Subtract
		 * that too for the initiator-side figure.
		 */
		const uint32_t best_ns = (uint32_t)((uint64_t)rtt_best * 1000000000ULL / hz);

		printk("soft_ns     ~%u round trip after the wire (subtract the\n",
		       best_ns > wire2 ? best_ns - wire2 : 0U);
		printk("            responder turnaround from its own console)\n");
		printk("\nelapsed     %u us for %u packets, %u us each\n",
		       (uint32_t)((uint64_t)elapsed * 1000000ULL / hz), answered,
		       (uint32_t)((uint64_t)elapsed * 1000000ULL / hz / answered));
		printk("skew        %lld cycles accumulated over the burst\n",
		       (long long)skew);
		printk("            (%lld ns; positive means this board's clock ran long)\n",
		       (long long)(skew * 1000000000LL / (int64_t)hz));
		printk("skew_gaps   %u intervals too long to attribute\n", latch.skew_gaps);
	}

	print_link_stats("initiator");
	printk("\ndone\n");
	return 0;
}

#endif /* LINK_RESPONDER */
