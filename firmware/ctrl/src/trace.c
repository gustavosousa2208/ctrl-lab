#include <string.h>

#include "dcp.h"
#include "trace.h"

uint32_t ctrl_f32_bits(float value)
{
	uint32_t bits;

	memcpy(&bits, &value, sizeof(bits));
	return bits;
}

void ctrl_trace_hash_init(struct ctrl_trace_hash *hash)
{
	hash->state = 0xcbf29ce484222325ULL;
}

/* Little-endian byte order explicitly, not a memcpy of the float, so the digest
 * is defined by the format rather than by the host's endianness. Both sides
 * that compute it today are little-endian; this keeps that from being load
 * bearing.
 */
void ctrl_trace_hash_push(struct ctrl_trace_hash *hash, float value)
{
	const uint32_t bits = ctrl_f32_bits(value);

	for (int i = 0; i < 4; i++) {
		hash->state ^= (bits >> (8 * i)) & 0xffU;
		hash->state *= 0x100000001b3ULL;
	}
}

/* --- binary frame ---------------------------------------------------------
 *
 * Everything is written byte at a time in explicit little-endian order rather
 * than by casting a struct. Struct layout depends on the compiler's padding and
 * the target's alignment rules, and this frame has to be identical coming off a
 * Cortex-M7 and off whatever host builds the reference. The format defines the
 * bytes; the C types are an implementation detail.
 */

static void put_u16(uint8_t *out, uint16_t value)
{
	out[0] = (uint8_t)(value & 0xffU);
	out[1] = (uint8_t)((value >> 8) & 0xffU);
}

static void put_u32(uint8_t *out, uint32_t value)
{
	for (int i = 0; i < 4; i++) {
		out[i] = (uint8_t)((value >> (8 * i)) & 0xffU);
	}
}

static void put_u64(uint8_t *out, uint64_t value)
{
	for (int i = 0; i < 8; i++) {
		out[i] = (uint8_t)((value >> (8 * i)) & 0xffU);
	}
}

void ctrl_trace_put_f32(uint8_t out[4], float value)
{
	put_u32(out, ctrl_f32_bits(value));
}

void ctrl_trace_header(uint8_t out[CTRL_TRACE_HEADER_LEN], uint64_t plan_id,
		       uint16_t signal_count, uint32_t step_count)
{
	memset(out, 0, CTRL_TRACE_HEADER_LEN);
	memcpy(&out[0], CTRL_TRACE_MAGIC, 4);
	put_u16(&out[4], CTRL_TRACE_FRAME_VERSION);
	put_u16(&out[6], signal_count);
	put_u32(&out[8], step_count);
	put_u64(&out[12], plan_id);
	put_u32(&out[20], step_count * (1U + signal_count) * 4U);
	put_u32(&out[24], ctrl_crc32(out, 24));
	/* bytes 28..31 stay reserved and zero */
}

void ctrl_trace_trailer(uint8_t out[CTRL_TRACE_TRAILER_LEN], uint32_t payload_crc,
			uint64_t digest)
{
	put_u32(&out[0], payload_crc);
	put_u64(&out[4], digest);
}

void ctrl_trace_begin(struct ctrl_trace_writer *writer, ctrl_trace_sink sink, void *ctx,
		      uint64_t plan_id, uint16_t signal_count, uint32_t step_count)
{
	uint8_t header[CTRL_TRACE_HEADER_LEN];

	writer->sink = sink;
	writer->ctx = ctx;
	writer->crc = CTRL_CRC32_INIT;
	writer->signal_count = signal_count;
	ctrl_trace_hash_init(&writer->hash);

	ctrl_trace_header(header, plan_id, signal_count, step_count);
	sink(ctx, header, CTRL_TRACE_HEADER_LEN);
}

void ctrl_trace_row(struct ctrl_trace_writer *writer, float time, const float *signals)
{
	uint8_t word[4];

	/* Time first, then signals in slot order - the same sequence the text
	 * rows use, so a frame and a CSV of the same run hash identically.
	 */
	ctrl_trace_put_f32(word, time);
	writer->crc = ctrl_crc32_update(writer->crc, word, 4);
	ctrl_trace_hash_push(&writer->hash, time);
	writer->sink(writer->ctx, word, 4);

	for (uint16_t slot = 0; slot < writer->signal_count; slot++) {
		ctrl_trace_put_f32(word, signals[slot]);
		writer->crc = ctrl_crc32_update(writer->crc, word, 4);
		ctrl_trace_hash_push(&writer->hash, signals[slot]);
		writer->sink(writer->ctx, word, 4);
	}
}

uint64_t ctrl_trace_end(struct ctrl_trace_writer *writer)
{
	uint8_t trailer[CTRL_TRACE_TRAILER_LEN];
	const uint64_t digest = ctrl_trace_hash_value(&writer->hash);

	ctrl_trace_trailer(trailer, CTRL_CRC32_FINAL(writer->crc), digest);
	writer->sink(writer->ctx, trailer, CTRL_TRACE_TRAILER_LEN);
	return digest;
}
