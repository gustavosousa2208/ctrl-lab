/*
 * ctrl-lab link probe - stage E2.
 *
 * Moves bytes between the two boards and finds out what they cost. No control
 * loop, no plan execution: the only unknown this stage adds is the transport,
 * and mixing it with anything else would make a bad number unattributable.
 *
 *   F767 (initiator)  D1 = PG14 = usart6 TX  ->  PA10 = usart1 RX  (H743)
 *                     D0 = PG9  = usart6 RX  <-  PA9  = usart1 TX
 *                     GND <-> GND
 *
 * Both link UARTs are on APB2 - 108 MHz on the F767, 120 MHz on the H743 -
 * which is the fastest peripheral clock on each part.
 *
 * Two measurements:
 *
 *   ping   one byte out, one byte back, timed on the initiator's clock. This is
 *          the only way to get an accurate delay across two boards that have no
 *          shared timebase: measure the round trip on one clock, and have the
 *          far side report its own turnaround so it can be subtracted.
 *
 *   frame  a real DCPT frame, verified on the far side with the same CRC code
 *          the control runtime uses. Answers whether the link is trustworthy
 *          for stage E3, and what it costs in throughput.
 *
 * What `ping` does NOT measure: uart_poll_out returns once the byte is handed
 * to the transmit register, not once it has left the wire. The last character
 * time is therefore outside the measurement on each hop. At 1 Mbaud that is
 * 10 us per direction, which is large - so the reported delay is a lower bound
 * on the wire time and an accurate measure of software-to-software latency.
 */

#include <zephyr/kernel.h>
#include <zephyr/device.h>
#include <zephyr/drivers/uart.h>
#include <zephyr/sys/printk.h>
#include <zephyr/timing/timing.h>

#include "dcp.h"
#include "trace.h"

#define LINK_NODE DT_ALIAS(link_uart)
#if !DT_NODE_EXISTS(LINK_NODE)
#error "no link-uart alias - add one in boards/<board>.overlay"
#endif

static const struct device *const link = DEVICE_DT_GET(LINK_NODE);

/* Protocol, deliberately one byte wide. Anything richer would need framing of
 * its own, and the DCPT frame already brings framing for the payload.
 */
#define CMD_PING  'P'
#define RSP_PING  'p'
#define CMD_FRAME 'F'
#define CMD_REPORT 'R'
#define RSP_CRC_OK   'Y'
#define RSP_CRC_BAD  'N'

#define PING_COUNT 1000

/* A synthetic trace frame: 200 rows of 4 signals. Big enough that throughput
 * means something, small enough to sit in RAM on both boards.
 */
#define FRAME_ROWS    200
#define FRAME_SIGNALS 4
#define FRAME_BYTES (CTRL_TRACE_HEADER_LEN + \
		     FRAME_ROWS * (1 + FRAME_SIGNALS) * 4 + CTRL_TRACE_TRAILER_LEN)

static uint8_t frame[FRAME_BYTES];

/* Blocking single-byte read. Returns false if nothing arrived within the
 * timeout, which is what distinguishes "the far side is not there" from "the
 * far side is slow".
 */
/* Bulk receive with an absolute deadline.
 *
 * The fast path does NO timing work at all: while bytes keep arriving,
 * uart_poll_in succeeds and nothing else is called. The deadline is only
 * consulted when a poll comes back empty, which is the only time the answer can
 * have changed.
 *
 * That matters more than it looks. The first version called link_get() once per
 * byte, and each call read sys_clock_hw_cycles_per_sec() and k_cycle_get_32()
 * to build a fresh deadline. At 1 Mbaud a byte lands every 10 us and that fits;
 * at 2 Mbaud the budget halves to 5 us and it does not, so the receiver fell
 * behind a blast it could easily keep up with and every frame above 1 Mbaud
 * failed CRC while single-byte pings at 6 Mbaud were flawless. Measured.
 */
#ifdef LINK_RESPONDER
static bool link_recv(uint8_t *out, uint32_t len, uint32_t deadline)
{
	uint32_t got = 0;

	while (got < len) {
		if (uart_poll_in(link, &out[got]) == 0) {
			got++;
			continue;
		}
		if ((int32_t)(k_cycle_get_32() - deadline) >= 0) {
			return false;
		}
	}
	return true;
}

static uint32_t link_deadline(uint32_t ms)
{
	return k_cycle_get_32() + ms * (sys_clock_hw_cycles_per_sec() / 1000U);
}
#endif /* LINK_RESPONDER */

#ifdef LINK_INITIATOR
static bool link_get(uint8_t *out, uint32_t timeout_us)
{
	/* Spin tightly. The first version slept k_busy_wait(1) between polls,
	 * which reads the cycle counter and costs far more than the 1 us it
	 * claims - at 1 Mbaud a byte lands every 10 us and the STM32 USART
	 * receive register holds exactly one, so a slow poll loop overruns it and
	 * drops bytes. That showed up as a frame CRC mismatch and a transfer
	 * running 5x slower than the wire, because every dropped byte then cost a
	 * full receive timeout. Measured, not theorised: 4044 bytes took 210 ms
	 * against a 40 ms floor.
	 *
	 * The deadline is compared as a signed difference so the 32-bit cycle
	 * counter wrapping mid-wait is harmless.
	 */
	const uint32_t per_us = sys_clock_hw_cycles_per_sec() / 1000000U;
	const uint32_t deadline = k_cycle_get_32() + timeout_us * per_us;

	do {
		if (uart_poll_in(link, out) == 0) {
			return true;
		}
	} while ((int32_t)(k_cycle_get_32() - deadline) < 0);

	return false;
}
#endif /* LINK_INITIATOR */

#ifdef LINK_INITIATOR
static void link_put(const uint8_t *bytes, uint32_t len)
{
	for (uint32_t i = 0; i < len; i++) {
		uart_poll_out(link, bytes[i]);
	}
}
#endif

/* --- frame construction, initiator only ---------------------------------- */

#ifdef LINK_INITIATOR
static uint32_t frame_len;

static void frame_sink(void *ctx, const uint8_t *bytes, uint32_t len)
{
	ARG_UNUSED(ctx);

	for (uint32_t i = 0; i < len && frame_len < sizeof(frame); i++) {
		frame[frame_len++] = bytes[i];
	}
}

/* Deterministic contents, so both ends can build the identical frame and any
 * disagreement is the wire rather than the data.
 */
static void build_frame(void)
{
	struct ctrl_trace_writer writer;
	float row[FRAME_SIGNALS];

	frame_len = 0;
	ctrl_trace_begin(&writer, frame_sink, NULL, 0x1234567890abcdefULL,
			 FRAME_SIGNALS, FRAME_ROWS);

	for (uint32_t k = 0; k < FRAME_ROWS; k++) {
		for (int s = 0; s < FRAME_SIGNALS; s++) {
			row[s] = (float)(k * FRAME_SIGNALS + s);
		}
		ctrl_trace_row(&writer, (float)k * 0.001f, row);
	}
	ctrl_trace_end(&writer);
}
#endif /* LINK_INITIATOR */

#ifdef LINK_RESPONDER

/* The H743 end. Never returns: it has to keep answering, and on this part
 * letting Zephyr idle into WFI also takes the core off the SWD debug bus, which
 * is how RTT is read. See firmware/BRINGUP.md.
 */
int main(void)
{
	printk("\nctrl-lab link responder\n");
	printk("board " CONFIG_BOARD_TARGET ", link %s\n", link->name);

	if (!device_is_ready(link)) {
		printk("FAIL: link uart not ready\n");
		return 0;
	}

	timing_init();
	timing_start();

	uint32_t pings = 0, frames = 0, bad = 0, last_payload = 0;
	uint32_t turn_best = UINT32_MAX, turn_worst = 0;
	uint64_t turn_total = 0;

	printk("listening\n");

	while (1) {
		uint8_t cmd;

		if (uart_poll_in(link, &cmd) != 0) {
			continue;
		}

		if (cmd == CMD_PING) {
			timing_t t0 = timing_counter_get();

			uart_poll_out(link, RSP_PING);

			timing_t t1 = timing_counter_get();
			uint32_t turn = (uint32_t)timing_cycles_get(&t0, &t1);

			turn_best = MIN(turn_best, turn);
			turn_worst = MAX(turn_worst, turn);
			turn_total += turn;
			pings++;

			/* Deliberately silent here. printk between the ping phase
			 * and the frame is enough to miss the first bytes of the
			 * frame: the STM32 USART holds one received byte and the
			 * initiator starts sending immediately. That cost a full
			 * baud sweep of CRC mismatches at every rate, including one
			 * that had passed minutes earlier. Stats are printed after
			 * the frame instead, when the link is quiet.
			 */
		} else if (cmd == CMD_FRAME) {
			/* Header first, because payload_len lives in it and must
			 * be CRC-checked before it is trusted as a length.
			 */
			uint8_t header[CTRL_TRACE_HEADER_LEN];
			const uint32_t deadline = link_deadline(2000);
			bool ok = link_recv(header, sizeof(header), deadline);

			uint32_t payload_len = 0;

			if (ok) {
				for (int i = 0; i < 4; i++) {
					payload_len |= (uint32_t)header[20 + i] << (8 * i);
				}
				uint32_t want = 0;

				for (int i = 0; i < 4; i++) {
					want |= (uint32_t)header[24 + i] << (8 * i);
				}
				ok = (ctrl_crc32(header, 24) == want) &&
				     (payload_len + CTRL_TRACE_HEADER_LEN +
				      CTRL_TRACE_TRAILER_LEN <= sizeof(frame));
			}

			/* Buffer the payload, then CRC it.
			 *
			 * Keeping the CRC out of the receive loop is the right
			 * shape - at 6 Mbaud a byte lands every 1.67 us, about 400
			 * cycles, and uart_poll_in plus a bitwise 8-iteration CRC
			 * does not fit in that. But see the bead: bulk transfer is
			 * only verified at 1 Mbaud, and the higher rates fail for a
			 * reason not yet found.
			 */
			if (ok) {
				ok = link_recv(frame, payload_len, deadline);
			}

			uint32_t crc = CTRL_CRC32_INIT;

			if (ok) {
				crc = ctrl_crc32_update(crc, frame, payload_len);
			}

			uint8_t trailer[CTRL_TRACE_TRAILER_LEN];

			if (ok) {
				ok = link_recv(trailer, sizeof(trailer), deadline);
			}

			uint32_t want_crc = 0;

			if (ok) {
				for (int i = 0; i < 4; i++) {
					want_crc |= (uint32_t)trailer[i] << (8 * i);
				}
				ok = (CTRL_CRC32_FINAL(crc) == want_crc);
			}

			frames++;
			if (!ok) {
				bad++;
			}
			uart_poll_out(link, ok ? RSP_CRC_OK : RSP_CRC_BAD);
			last_payload = payload_len;
		} else if (cmd == CMD_REPORT) {
			/* The ONLY place this side prints.
			 *
			 * Printing anywhere else loses bytes, and it took three
			 * sweeps to accept that. The console is not the link, but
			 * printk still takes time, and the receive register holds
			 * exactly one byte - so any print that overlaps an incoming
			 * transfer drops data. It bit twice: once between the ping
			 * phase and the frame, and again between two initiator runs,
			 * because console.py resets the board and the initiator
			 * therefore runs once on flash and once on reset.
			 *
			 * Now the initiator asks for the report after it already has
			 * its verdict, so the link is provably idle.
			 */
			printk("frames %u (bad %u), last payload %u bytes\n", frames, bad,
			       last_payload);
			printk("pings %u, turnaround min=%u mean=%u max=%u cycles\n", pings,
			       pings ? turn_best : 0,
			       pings ? (uint32_t)(turn_total / pings) : 0, turn_worst);
		}
	}
	return 0;
}

#else /* LINK_INITIATOR */

int main(void)
{
	printk("\nctrl-lab link initiator\n");
	printk("board " CONFIG_BOARD_TARGET ", link %s\n", link->name);
	printk("core %u Hz\n", sys_clock_hw_cycles_per_sec());

	if (!device_is_ready(link)) {
		printk("FAIL: link uart not ready\n");
		return 0;
	}

	timing_init();
	timing_start();

	/* Drain anything the responder left in flight from a previous run, so the
	 * first ping is not answered by a stale byte.
	 */
	uint8_t junk;

	while (uart_poll_in(link, &junk) == 0) {
	}

	/* --- ping round trip --- */
	uint32_t best = UINT32_MAX, worst = 0, lost = 0;
	uint64_t total = 0;
	uint32_t counted = 0;

	for (uint32_t i = 0; i < PING_COUNT; i++) {
		uint8_t reply;

		timing_t t0 = timing_counter_get();

		uart_poll_out(link, CMD_PING);

		if (!link_get(&reply, 50000) || reply != RSP_PING) {
			lost++;
			continue;
		}

		timing_t t1 = timing_counter_get();
		uint32_t rtt = (uint32_t)timing_cycles_get(&t0, &t1);

		best = MIN(best, rtt);
		worst = MAX(worst, rtt);
		total += rtt;
		counted++;
	}

	const uint32_t hz = sys_clock_hw_cycles_per_sec();

	if (counted == 0) {
		printk("FAIL: no replies. Check TX/RX are crossed, GND is shared,\n");
		printk("      and that both ends are built with the same LINK_BAUD.\n");
		return 0;
	}

	const uint32_t mean = (uint32_t)(total / counted);

	printk("\nping        %u of %u answered, %u lost\n", counted, PING_COUNT, lost);
	printk("rtt_cycles  min=%u mean=%u max=%u spread=%u\n", best, mean, worst,
	       worst - best);
	printk("rtt_ns      min=%u mean=%u max=%u\n",
	       (uint32_t)((uint64_t)best * 1000000000U / hz),
	       (uint32_t)((uint64_t)mean * 1000000000U / hz),
	       (uint32_t)((uint64_t)worst * 1000000000U / hz));
	printk("one_way_ns  ~%u (half the round trip, before subtracting the\n",
	       (uint32_t)((uint64_t)mean * 1000000000U / hz / 2U));
	printk("            responder turnaround it reports on its own console)\n");

	/* --- bulk frame --- */

	/* Let the responder finish whatever the ping phase left it doing. The
	 * frame arrives as one uninterrupted blast and the far side has a
	 * one-byte receive register, so it must be idle and listening first.
	 */
	k_msleep(50);

	build_frame();
	printk("\nframe       %u bytes, %u rows x %u signals\n", frame_len, FRAME_ROWS,
	       FRAME_SIGNALS);

	timing_t f0 = timing_counter_get();

	uart_poll_out(link, CMD_FRAME);
	link_put(frame, frame_len);

	uint8_t verdict = 0;
	bool answered = link_get(&verdict, 500000);
	timing_t f1 = timing_counter_get();

	if (!answered) {
		printk("FAIL: responder never answered the frame\n");
		return 0;
	}

	const uint32_t cycles = (uint32_t)timing_cycles_get(&f0, &f1);
	const uint32_t us = (uint32_t)((uint64_t)cycles * 1000000U / hz);

	printk("frame_crc   %s\n", verdict == RSP_CRC_OK ? "OK on the far side"
							: "*** MISMATCH ***");
	printk("frame_time  %u cycles, %u us\n", cycles, us);
	printk("throughput  %u KB/s (includes the far side's CRC pass)\n",
	       us ? (uint32_t)((uint64_t)frame_len * 1000U / us) : 0U);

	/* Ask the far side to report now, when nothing is in flight. */
	uart_poll_out(link, CMD_REPORT);

	printk("\ndone\n");
	return 0;
}

#endif
