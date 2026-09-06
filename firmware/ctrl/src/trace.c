#include <string.h>

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
