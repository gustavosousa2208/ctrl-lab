/*
 * Packet framing over a slot ring - the part of the receive path that has no
 * hardware in it.
 *
 * It is separate from link_dma.c for one reason: it is the piece most likely
 * to be wrong, and on a board a framing bug is indistinguishable from a wiring
 * fault. E2 spent four rounds proving that the expensive way (see
 * firmware/link/README.md). Everything here is plain C over a caller-supplied
 * byte ring, so firmware/link/test exercises it natively, including the cases
 * a board will not reproduce on demand - a stream that starts mid-packet, a
 * corrupted frame, a magic that appears inside float payload.
 */

#ifndef CTRL_LINK_FRAME_H_
#define CTRL_LINK_FRAME_H_

#include <stdbool.h>
#include <stdint.h>

#include "link_pkt.h"

/* Where in a slot a packet ends. 32 means packets and slots coincide; less
 * means the stream is offset and a packet straddles two slots. Never 0 -
 * "ends at the start of this slot" and "ends at the end of the previous one"
 * are the same statement, and collapsing them removes a special case from
 * every caller.
 */
#define LINK_FRAME_ALIGNED CTRL_LINK_PKT_LEN

struct link_frame {
	const uint8_t (*ring)[CTRL_LINK_PKT_LEN];
	unsigned int slots;
	uint8_t pkt_end;
	uint32_t resyncs;	/* times pkt_end moved */
	uint32_t bad;		/* slots that yielded no valid packet */
};

/* Builds the CRC table. Must be called once before anything else here; both
 * link_frame_crc32() and link_frame_take() depend on it.
 *
 * Returns false if the table disagrees with dcp.c's bitwise implementation,
 * which would mean this end and the control core no longer compute the same
 * CRC over the same bytes.
 */
bool link_frame_crc_init(void);

uint32_t link_frame_crc32(const uint8_t *bytes, uint32_t len);

/* Fills in magic and crc32. The rest of the packet is the caller's. */
void link_frame_stamp(struct ctrl_link_pkt *pkt);

/* `ring` is `slots` buffers of one packet each, handed to the DMA in strict
 * index order so that consecutive slots form a contiguous byte stream. At
 * least 4 slots: the driver asks for slot n+2 while slot n is being read, and
 * reassembly also needs slot n-1.
 */
void link_frame_init(struct link_frame *f, const uint8_t (*ring)[CTRL_LINK_PKT_LEN],
		     unsigned int slots);

/* Extracts the packet that finished inside slot `idx`, resynchronising first
 * if the one at the current offset does not validate.
 *
 * Returns false if no valid packet could be found in slot `idx` and the one
 * before it, leaving pkt_end untouched: an isolated corrupt frame should not
 * cost the alignment of the frames after it.
 */
bool link_frame_take(struct link_frame *f, unsigned int idx, struct ctrl_link_pkt *out);

/* True when packets and slots coincide.
 *
 * Worth acting on rather than tolerating. A packet that ends part-way into a
 * slot is not handed over until that slot fills, so its delivery waits on the
 * first bytes of the NEXT packet - a whole control tick of latency here, on
 * every tick, and invisible unless something looks for it. The caller owns the
 * buffer grid, so the caller is the one that can move it; link_dma does, by
 * handing the driver a single odd-sized buffer.
 */
static inline bool link_frame_aligned(const struct link_frame *f)
{
	return f->pkt_end == LINK_FRAME_ALIGNED;
}

/* Declares that the buffer grid has been moved and packets now end on slot
 * boundaries again. */
void link_frame_set_aligned(struct link_frame *f);

#endif /* CTRL_LINK_FRAME_H_ */
