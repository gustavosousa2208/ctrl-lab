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

#endif /* CTRL_TRACE_H */
