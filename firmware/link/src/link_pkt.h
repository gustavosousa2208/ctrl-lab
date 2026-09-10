/*
 * The E3 wire format: one fixed 32-byte packet per control tick.
 *
 * Fixed size is the whole point. The receive path is a DMA controller writing
 * into 32-byte buffers, so "a buffer filled" and "a packet arrived" are the
 * same event and the framing costs nothing at runtime - no length field to
 * parse, no delimiter to scan for, and the buffer swap lands in the idle gap
 * between packets rather than in the middle of one.
 *
 * Everything here has to agree byte for byte with the other end, which is a
 * different SoC. The layout is packed and pinned by a build assertion, and the
 * CRC is the same reflected CRC-32 the control plan format uses (dcp.h), so a
 * frame that survives this check has survived the same arithmetic the plan
 * loader applies.
 */

#ifndef CTRL_LINK_PKT_H_
#define CTRL_LINK_PKT_H_

#include <stddef.h>
#include <stdint.h>

/* Nothing here may reach for a Zephyr header. firmware/link/test builds this
 * file, link_frame.c and dcp.c natively, and the whole value of that is that
 * it is the same source the boards run.
 */
#define CTRL_LINK_PACKED __attribute__((packed))

/* Both parts are little-endian, so this 16-bit value goes out low byte first
 * and reads as 'L','C' on the wire, not 'C','L'. Written as a number rather
 * than a character pair because that is what the resync scan compares.
 */
#define CTRL_LINK_MAGIC 0x434CU

#define CTRL_LINK_PKT_LEN 32U
#define CTRL_LINK_SIGNALS 4U

struct CTRL_LINK_PACKED ctrl_link_pkt {
	uint16_t magic;		/* CTRL_LINK_MAGIC */
	uint16_t signal_count;	/* how many of signals[] carry meaning */
	uint32_t seq;		/* sender's packet counter, wraps freely */
	uint32_t tx_tick;	/* sender's control tick at transmit */
	float signals[CTRL_LINK_SIGNALS];
	uint32_t crc32;		/* over the preceding 28 bytes */
};

_Static_assert(sizeof(struct ctrl_link_pkt) == CTRL_LINK_PKT_LEN,
	       "ctrl_link_pkt must be exactly 32 bytes on both boards");
_Static_assert(offsetof(struct ctrl_link_pkt, crc32) == CTRL_LINK_PKT_LEN - 4U,
	       "crc32 must be the last field");

/* Bytes the CRC covers: everything but the CRC itself. */
#define CTRL_LINK_PKT_CRC_LEN (CTRL_LINK_PKT_LEN - 4U)

#endif /* CTRL_LINK_PKT_H_ */
