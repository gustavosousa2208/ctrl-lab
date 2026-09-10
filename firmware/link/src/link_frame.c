/* Packet framing over a slot ring. See link_frame.h for what this is and why
 * it is not in link_dma.c.
 */

#include <string.h>

#include "dcp.h"
#include "link_frame.h"

/* --- CRC ----------------------------------------------------------------- */

/* dcp.c computes the CRC a bit at a time, which is right for a plan loaded
 * once at startup and wrong here: 28 bytes cost roughly 1100 cycles that way -
 * about 5 us on either board - and this runs in an ISR ten thousand times a
 * second.
 *
 * The table is therefore built FROM ctrl_crc32_update() rather than being a
 * second copy of the polynomial. Agreement with the control core is then
 * structural rather than asserted, and asserted anyway, once, below.
 */
static uint32_t crc_table[256];

uint32_t link_frame_crc32(const uint8_t *bytes, uint32_t len)
{
	uint32_t crc = CTRL_CRC32_INIT;

	for (uint32_t i = 0; i < len; i++) {
		crc = (crc >> 8) ^ crc_table[(crc ^ bytes[i]) & 0xffU];
	}
	return CTRL_CRC32_FINAL(crc);
}

bool link_frame_crc_init(void)
{
	/* The vector is a packet-sized run of the payload this actually
	 * carries - float bit patterns included, since 1.0f and -2.0f put
	 * bytes in it that a text vector never would.
	 */
	static const uint8_t vector[CTRL_LINK_PKT_CRC_LEN] = {
		0x4c, 0x43, 0x04, 0x00, 0xef, 0xbe, 0xad, 0xde,
		0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3f,
		0x00, 0x00, 0x00, 0xc0, 0xff, 0xff, 0x7f, 0x7f,
		0x00, 0x00, 0x00, 0x00,
	};

	for (uint32_t i = 0; i < 256U; i++) {
		const uint8_t byte = (uint8_t)i;

		/* Starting from 0 reduces the update to "fold this one byte
		 * through the polynomial", which is exactly a table entry.
		 */
		crc_table[i] = ctrl_crc32_update(0U, &byte, 1U);
	}

	return link_frame_crc32(vector, sizeof(vector)) ==
	       ctrl_crc32(vector, sizeof(vector));
}

void link_frame_stamp(struct ctrl_link_pkt *pkt)
{
	pkt->magic = CTRL_LINK_MAGIC;
	pkt->crc32 = link_frame_crc32((const uint8_t *)pkt, CTRL_LINK_PKT_CRC_LEN);
}

/* --- framing ------------------------------------------------------------- */

static bool pkt_valid(const uint8_t *bytes)
{
	uint16_t magic;
	uint32_t crc;

	/* Copied out rather than read through a cast: `bytes` points into a
	 * byte buffer at an arbitrary offset, which keeps both the alignment
	 * and the aliasing questions from arising at all.
	 */
	memcpy(&magic, bytes, sizeof(magic));
	if (magic != CTRL_LINK_MAGIC) {
		return false;
	}

	memcpy(&crc, bytes + CTRL_LINK_PKT_CRC_LEN, sizeof(crc));
	return link_frame_crc32(bytes, CTRL_LINK_PKT_CRC_LEN) == crc;
}

/* Copies out the packet that finished inside slot `idx`.
 *
 * It ends `end` bytes into this slot, so it begins (32 - end) bytes before the
 * end of the previous one. With end == 32 that reduces to the slot itself and
 * the previous slot is not touched - which is also the startup case, where
 * nothing has been received into it yet.
 */
static void gather(const struct link_frame *f, uint8_t *out, unsigned int idx, uint8_t end)
{
	const unsigned int prev = (idx + f->slots - 1U) % f->slots;
	const uint32_t head = CTRL_LINK_PKT_LEN - end;

	if (head != 0U) {
		memcpy(out, &f->ring[prev][end], head);
	}
	memcpy(out + head, &f->ring[idx][0], end);
}

/* Looks for a packet boundary somewhere other than where we thought it was.
 *
 * The search window is the previous slot followed by this one, and a candidate
 * `end` puts the packet at window offset `end` - so end == 32 is the aligned
 * case sitting at the top of the window, and end == 1 is a packet almost
 * entirely in the previous slot. Every candidate lies wholly inside the 64
 * bytes, which is why the window is two slots and not one.
 *
 * Only candidates whose magic matches are worth a CRC, and only the first few
 * of those are tried. The magic is two bytes and will occasionally appear
 * inside float payload, so the work has to be bounded by construction rather
 * than by hope.
 */
#define RESYNC_MAX_CRC 3

static bool resync(struct link_frame *f, uint8_t *out, unsigned int idx)
{
	const unsigned int prev = (idx + f->slots - 1U) % f->slots;
	uint8_t window[2U * CTRL_LINK_PKT_LEN];
	unsigned int tried = 0;

	memcpy(window, &f->ring[prev][0], CTRL_LINK_PKT_LEN);
	memcpy(window + CTRL_LINK_PKT_LEN, &f->ring[idx][0], CTRL_LINK_PKT_LEN);

	for (unsigned int end = 1U; end <= LINK_FRAME_ALIGNED; end++) {
		uint16_t magic;

		if (end == f->pkt_end) {
			continue;	/* just failed, in the caller */
		}

		memcpy(&magic, &window[end], sizeof(magic));
		if (magic != CTRL_LINK_MAGIC) {
			continue;
		}
		if (++tried > RESYNC_MAX_CRC) {
			break;
		}
		if (!pkt_valid(&window[end])) {
			continue;
		}

		memcpy(out, &window[end], CTRL_LINK_PKT_LEN);
		f->pkt_end = (uint8_t)end;
		f->resyncs++;
		return true;
	}
	return false;
}

void link_frame_init(struct link_frame *f, const uint8_t (*ring)[CTRL_LINK_PKT_LEN],
		     unsigned int slots)
{
	f->ring = ring;
	f->slots = slots;
	f->pkt_end = LINK_FRAME_ALIGNED;
	f->resyncs = 0;
	f->bad = 0;
}

void link_frame_set_aligned(struct link_frame *f)
{
	f->pkt_end = LINK_FRAME_ALIGNED;
}

bool link_frame_take(struct link_frame *f, unsigned int idx, struct ctrl_link_pkt *out)
{
	uint8_t *bytes = (uint8_t *)out;

	gather(f, bytes, idx, f->pkt_end);
	if (pkt_valid(bytes)) {
		return true;
	}

	f->bad++;
	return resync(f, bytes, idx);
}
