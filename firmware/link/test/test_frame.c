/*
 * Host tests for the link framing (src/link_frame.c).
 *
 * The board cannot produce these cases on demand. A stream that starts
 * mid-packet happens when one board is reset while the other is already
 * sending; a corrupted frame happens rarely and never where you want it; a
 * magic sequence inside float payload happens when the payload happens to
 * contain it. All three are one line here.
 *
 * The model is a byte stream fed into the slot ring exactly the way the DMA
 * fills it: 32 bytes at a time, slots in strict order, link_frame_take() once
 * per completed slot. If this agrees with the board, the framing is right; if
 * it disagrees, the difference is hardware and worth chasing.
 */

#include <stdio.h>
#include <string.h>

#include "link_frame.h"
#include "link_pkt.h"

#define SLOTS 4U

static int failures;

static void check(bool ok, const char *what)
{
	printf("%-58s %s\n", what, ok ? "ok" : "FAIL");
	if (!ok) {
		failures++;
	}
}

/* --- a stream of packets ------------------------------------------------- */

static void make_pkt(struct ctrl_link_pkt *pkt, uint32_t seq)
{
	memset(pkt, 0, sizeof(*pkt));
	pkt->signal_count = CTRL_LINK_SIGNALS;
	pkt->seq = seq;
	pkt->tx_tick = seq * 10U;
	for (unsigned int i = 0; i < CTRL_LINK_SIGNALS; i++) {
		pkt->signals[i] = (float)seq + (float)i * 0.25f;
	}
	link_frame_stamp(pkt);
}

/* The receiver, driven one slot at a time. `lead` is how many bytes of junk
 * precede the first packet - the DMA having been armed mid-stream.
 */
struct rx {
	uint8_t ring[SLOTS][CTRL_LINK_PKT_LEN];
	struct link_frame framer;
	unsigned int slot;	/* next slot the DMA will fill */
	unsigned int fill;	/* bytes already in that slot */
	uint32_t taken;		/* packets recovered */
	uint32_t last_seq;
	bool last_ok;
};

static void rx_init(struct rx *r)
{
	memset(r, 0, sizeof(*r));
	link_frame_init(&r->framer, (const uint8_t (*)[CTRL_LINK_PKT_LEN])r->ring, SLOTS);
}

/* One byte off the wire. Completing a slot is what calls the framer, which is
 * exactly when the driver raises UART_RX_RDY.
 */
static void rx_byte(struct rx *r, uint8_t b)
{
	struct ctrl_link_pkt got;

	r->ring[r->slot][r->fill++] = b;
	if (r->fill < CTRL_LINK_PKT_LEN) {
		return;
	}

	r->last_ok = link_frame_take(&r->framer, r->slot, &got);
	if (r->last_ok) {
		r->taken++;
		r->last_seq = got.seq;
	}

	r->fill = 0;
	r->slot = (r->slot + 1U) % SLOTS;
}

static void rx_bytes(struct rx *r, const uint8_t *bytes, size_t len)
{
	for (size_t i = 0; i < len; i++) {
		rx_byte(r, bytes[i]);
	}
}

static void rx_packets(struct rx *r, uint32_t first, uint32_t count)
{
	for (uint32_t i = 0; i < count; i++) {
		struct ctrl_link_pkt pkt;

		make_pkt(&pkt, first + i);
		rx_bytes(r, (const uint8_t *)&pkt, sizeof(pkt));
	}
}

/* --- tests --------------------------------------------------------------- */

static void test_crc_agrees(void)
{
	check(link_frame_crc_init(), "table CRC-32 agrees with dcp.c bitwise CRC-32");
}

static void test_aligned_stream(void)
{
	struct rx r;

	rx_init(&r);
	rx_packets(&r, 1, 100);

	check(r.taken == 100, "aligned stream: 100 packets in, 100 recovered");
	check(r.last_seq == 100, "aligned stream: last packet is the last one sent");
	check(r.framer.pkt_end == LINK_FRAME_ALIGNED, "aligned stream: boundary never moved");
	check(r.framer.resyncs == 0 && r.framer.bad == 0, "aligned stream: no resync, no bad frame");
}

/* Every possible starting misalignment. This is the case a reset produces and
 * the one most likely to be wrong, so it is swept rather than sampled.
 */
static void test_every_offset(void)
{
	bool all_recovered = true;
	bool all_cheap = true;
	bool all_correct = true;
	bool all_boundary = true;

	for (unsigned int lead = 1; lead < CTRL_LINK_PKT_LEN; lead++) {
		uint8_t junk[CTRL_LINK_PKT_LEN];
		struct rx r;

		/* Junk that is not accidentally a packet, and does not contain
		 * the magic: 0x00 cannot start one.
		 */
		memset(junk, 0, sizeof(junk));

		rx_init(&r);
		rx_bytes(&r, junk, lead);
		rx_packets(&r, 1, 20);

		/* One packet may be lost to the resync itself: the first slot
		 * boundary after the junk can complete before a whole packet
		 * has landed. Everything after must arrive.
		 */
		if (r.taken < 19) {
			all_recovered = false;
		}
		if (r.framer.resyncs > 1) {
			all_cheap = false;
		}

		/* And the last packet sent is NOT the last packet delivered.
		 * This is the cost of running misaligned and the reason
		 * link_dma realigns the buffer grid rather than living with
		 * it: a packet that ends part-way into a slot is not handed
		 * over until that slot fills, so delivery waits for the bytes
		 * of the NEXT packet. On this link that is a whole control
		 * tick of latency, every tick, silently.
		 */
		if (r.last_seq != 19) {
			all_correct = false;
		}
		if (r.framer.pkt_end != lead) {
			all_boundary = false;
		}
	}

	check(all_recovered, "offset stream: every lead 1..31 recovers 19+ of 20");
	check(all_cheap, "offset stream: one resync is enough for any lead");
	check(all_boundary, "offset stream: boundary lands exactly on the lead");
	check(all_correct, "offset stream: delivery lags one packet while misaligned");
}

static void test_corrupt_frame_is_isolated(void)
{
	struct rx r;
	struct ctrl_link_pkt pkt;
	uint8_t bytes[CTRL_LINK_PKT_LEN];

	rx_init(&r);
	rx_packets(&r, 1, 5);

	/* One packet with a flipped payload bit. */
	make_pkt(&pkt, 6);
	memcpy(bytes, &pkt, sizeof(bytes));
	bytes[12] ^= 0x01U;
	rx_bytes(&r, bytes, sizeof(bytes));

	check(!r.last_ok, "corrupt frame: rejected rather than passed on");
	check(r.framer.bad == 1, "corrupt frame: counted once");
	check(r.framer.pkt_end == LINK_FRAME_ALIGNED,
	      "corrupt frame: boundary held, not chased");

	rx_packets(&r, 7, 5);
	check(r.taken == 10 && r.last_seq == 11,
	      "corrupt frame: the frames after it are unaffected");
}

/* The magic is two bytes, so payload will eventually contain it. A false
 * candidate must cost a CRC and nothing else - in particular it must not move
 * the boundary.
 */
static void test_magic_in_payload(void)
{
	struct rx r;
	struct ctrl_link_pkt pkt;

	rx_init(&r);
	rx_packets(&r, 1, 3);

	memset(&pkt, 0, sizeof(pkt));
	pkt.signal_count = CTRL_LINK_SIGNALS;
	pkt.seq = 4;
	pkt.tx_tick = 40;
	/* 0x434C434C in two signals: four magics inside one packet. */
	pkt.signals[0] = 0.0f;
	memcpy(&pkt.signals[1], "\x4c\x43\x4c\x43", 4);
	memcpy(&pkt.signals[2], "\x4c\x43\x4c\x43", 4);
	pkt.signals[3] = 0.0f;
	link_frame_stamp(&pkt);

	rx_bytes(&r, (const uint8_t *)&pkt, sizeof(pkt));

	check(r.last_ok && r.last_seq == 4, "magic in payload: packet still recovered");
	check(r.framer.resyncs == 0, "magic in payload: boundary not moved by a false match");

	rx_packets(&r, 5, 5);
	check(r.taken == 9 && r.last_seq == 9, "magic in payload: stream continues");
}

/* Sequence numbers wrap; the framer must not care. */
static void test_seq_wrap(void)
{
	struct rx r;
	struct ctrl_link_pkt pkt;

	rx_init(&r);
	for (uint32_t i = 0; i < 4; i++) {
		make_pkt(&pkt, 0xfffffffeU + i);
		rx_bytes(&r, (const uint8_t *)&pkt, sizeof(pkt));
	}

	check(r.taken == 4 && r.last_seq == 1U, "seq wrap: four packets across 2^32");
}

int main(void)
{
	if (!link_frame_crc_init()) {
		printf("crc table init disagrees with dcp.c - stopping\n");
		return 1;
	}

	test_crc_agrees();
	test_aligned_stream();
	test_every_offset();
	test_corrupt_frame_is_isolated();
	test_magic_in_payload();
	test_seq_wrap();

	printf("\n%s\n", failures == 0 ? "all ok" : "FAILURES");
	return failures == 0 ? 0 : 1;
}
