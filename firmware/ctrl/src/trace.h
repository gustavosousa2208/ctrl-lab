/*
 * Trace digest: one number that answers "did the device compute exactly what
 * the reference computed?".
 *
 * Grading a trace by eye, or by parsing decimal text, keeps running into the
 * same problem: a printed f32 is not the f32. Nine decimal places is not always
 * enough to round-trip a float, so a decimal comparison can only ever support a
 * tolerance claim, and the bar for this stage is bit-for-bit.
 *
 * So the samples are hashed as raw little-endian bits, in emission order, by
 * both sides. Equal digests mean every sample was bit-identical; unequal means
 * the per-sample CSV is worth reading to find where. FNV-1a is used because
 * plan.rs already uses it for plan_id - one hash in the project, not two.
 */

#ifndef CTRL_TRACE_H
#define CTRL_TRACE_H

#include <stdint.h>

struct ctrl_trace_hash {
	uint64_t state;
};

void ctrl_trace_hash_init(struct ctrl_trace_hash *hash);
void ctrl_trace_hash_push(struct ctrl_trace_hash *hash, float value);

static inline uint64_t ctrl_trace_hash_value(const struct ctrl_trace_hash *hash)
{
	return hash->state;
}

/* The raw bits of an f32, for printing a sample without formatting it. */
uint32_t ctrl_f32_bits(float value);

/*
 * The binary trace frame.
 *
 * Hex text costs 9 bytes per sample (8 digits and a separator) to carry 4 bytes
 * of f32. The frame below carries the 4, which is 2.2x less on the wire, and it
 * is transport-independent on purpose: the same bytes go over the ST-Link UART
 * today, a USB CDC endpoint next, and an MCU-to-MCU link in stage E.
 *
 * Layout, little-endian throughout:
 *
 *    offset  size  field
 *         0     4  magic "DCPT"
 *         4     2  frame_version
 *         6     2  signal_count
 *         8     4  step_count
 *        12     8  plan_id            (ties a trace to the plan that made it)
 *        20     4  payload_len        = step_count * (1 + signal_count) * 4
 *        24     4  header_crc32       over bytes 0..23
 *        28     4  reserved (0)
 *        32   ...  payload: per step, t then each signal, as f32
 *       ...     4  payload_crc32
 *       ...     8  digest             (the same FNV-1a64 the text line carries)
 *
 * The reader finds the frame by scanning for the magic and then consumes
 * exactly payload_len bytes, so the frame can sit in the middle of ordinary
 * console text without needing an escape scheme.
 *
 * Why a CRC when USB already has one, and when the digest already proves the
 * samples: because those cover different things. USB's CRC ends at the USB
 * layer and says nothing about a UART hop or an MCU-to-MCU wire. The digest
 * proves the *values* were right but cannot tell a truncated frame from a
 * complete one. header_crc32 makes payload_len safe to trust before allocating
 * against it, which is the field an attacker or a glitch would most like to
 * corrupt.
 */
#define CTRL_TRACE_MAGIC          "DCPT"
#define CTRL_TRACE_FRAME_VERSION  1
#define CTRL_TRACE_HEADER_LEN     32
#define CTRL_TRACE_TRAILER_LEN    12

/*
 * Streaming writer. The frame is emitted as it is produced rather than built in
 * memory: the device has a 501-row trace and no reason to hold a second copy of
 * it, and the host has an unbounded step count. The payload CRC and the sample
 * digest both accumulate as rows go past.
 */
typedef void (*ctrl_trace_sink)(void *ctx, const uint8_t *bytes, uint32_t len);

struct ctrl_trace_writer {
	ctrl_trace_sink sink;
	void *ctx;
	uint32_t crc;
	struct ctrl_trace_hash hash;
	uint16_t signal_count;
};

void ctrl_trace_begin(struct ctrl_trace_writer *writer, ctrl_trace_sink sink, void *ctx,
		      uint64_t plan_id, uint16_t signal_count, uint32_t step_count);

/* One row: the tick's time, then `signal_count` values. */
void ctrl_trace_row(struct ctrl_trace_writer *writer, float time, const float *signals);

/* Writes the trailer and returns the digest, which is the same value the text
 * path prints - the row order fed to the hash is identical.
 */
uint64_t ctrl_trace_end(struct ctrl_trace_writer *writer);

/* Fills a 32-byte frame header, including its own CRC. */
void ctrl_trace_header(uint8_t out[CTRL_TRACE_HEADER_LEN], uint64_t plan_id,
		       uint16_t signal_count, uint32_t step_count);

/* Fills the 12-byte trailer: payload CRC (already finalized) then digest. */
void ctrl_trace_trailer(uint8_t out[CTRL_TRACE_TRAILER_LEN], uint32_t payload_crc,
			uint64_t digest);

/* Little-endian store of one f32 sample, for building a payload row. */
void ctrl_trace_put_f32(uint8_t out[4], float value);

#endif /* CTRL_TRACE_H */
