/*
 * Plan decoding and validation. Load-time only; nothing here runs in a step.
 *
 * Two passes: read the byte stream into the fixed-size struct, then validate
 * the decoded plan as a whole. They are separate because some checks need
 * fields that arrive later in the stream than the record they constrain - a
 * transfer function's parameter length depends on the order word, and the
 * params section is encoded after every block record.
 *
 * The validation is deliberately exhaustive about slot indices. Every check
 * here is one the control step then does not have to do, which is what makes
 * ctrl_step() straight-line code with a computable WCET.
 */

#include <string.h>

#include "dcp.h"
#include "kernels.h"

#define DCP_HEADER_LEN 52
static const uint8_t DCP_MAGIC[4] = { 'D', 'C', 'P', '1' };

const char *ctrl_load_result_str(enum ctrl_load_result result)
{
	switch (result) {
	case CTRL_LOAD_OK:
		return "ok";
	case CTRL_LOAD_TOO_SHORT:
		return "stream shorter than the structure it declares";
	case CTRL_LOAD_BAD_MAGIC:
		return "not a DCP stream";
	case CTRL_LOAD_BAD_FORMAT_VERSION:
		return "container format version not implemented";
	case CTRL_LOAD_BAD_KERNEL_SET_VERSION:
		return "plan needs a newer kernel set than this firmware has";
	case CTRL_LOAD_CHECKSUM_MISMATCH:
		return "crc32 mismatch";
	case CTRL_LOAD_PLAN_ID_MISMATCH:
		return "plan_id mismatch";
	case CTRL_LOAD_UNKNOWN_KERNEL:
		return "unknown kernel id";
	case CTRL_LOAD_MALFORMED:
		return "internally inconsistent plan";
	case CTRL_LOAD_TOO_LARGE:
		return "plan exceeds a static pool capacity";
	case CTRL_LOAD_SLOT_OUT_OF_RANGE:
		return "signal or state slot outside the declared pools";
	case CTRL_LOAD_ARITY_MISMATCH:
		return "block input count disagrees with its kernel";
	case CTRL_LOAD_PARAMS_TOO_SHORT:
		return "packed parameters too short for the kernel";
	case CTRL_LOAD_IO_BINDINGS_UNSUPPORTED:
		return "plan carries io_bindings, which this runtime cannot bind";
	case CTRL_LOAD_UNSUPPORTED_RATE_DIV:
		return "rate_div != 1, and the scheduler runs one rate";
	case CTRL_LOAD_WCET_EXCEEDS_PERIOD:
		return "wcet_estimate_ns exceeds the tick period";
	}
	return "unknown";
}

/* --- integrity ----------------------------------------------------------- */

/* Bitwise CRC-32 (reflected, polynomial 0xEDB88320). Matches plan.rs exactly,
 * table-free: a plan is a few hundred bytes and this runs once at load, so a
 * 1 KiB table would cost more than it saves.
 */
uint32_t ctrl_crc32(const uint8_t *bytes, uint32_t len)
{
	uint32_t crc = 0xffffffffU;

	for (uint32_t i = 0; i < len; i++) {
		crc ^= bytes[i];
		for (int bit = 0; bit < 8; bit++) {
			const uint32_t mask = -(crc & 1U);

			crc = (crc >> 1) ^ (0xEDB88320U & mask);
		}
	}
	return ~crc;
}

uint64_t ctrl_fnv1a64(const uint8_t *bytes, uint32_t len)
{
	uint64_t hash = 0xcbf29ce484222325ULL;

	for (uint32_t i = 0; i < len; i++) {
		hash ^= bytes[i];
		hash *= 0x100000001b3ULL;
	}
	return hash;
}

/* --- cursor -------------------------------------------------------------- */

/* Byte-at-a-time little-endian reads. The stream has no alignment guarantees -
 * the params section starts wherever the block records happen to end - and on
 * Cortex-M7 an unaligned VLDR faults, so nothing here casts a pointer.
 */
struct cursor {
	const uint8_t *bytes;
	uint32_t len;
	uint32_t position;
	bool overrun;
};

static bool take(struct cursor *c, uint32_t n, const uint8_t **out)
{
	if (c->overrun || c->position + n > c->len || c->position + n < c->position) {
		c->overrun = true;
		return false;
	}
	*out = &c->bytes[c->position];
	c->position += n;
	return true;
}

static uint16_t take_u16(struct cursor *c)
{
	const uint8_t *b;

	if (!take(c, 2, &b)) {
		return 0;
	}
	return (uint16_t)((uint32_t)b[0] | ((uint32_t)b[1] << 8));
}

static uint32_t take_u32(struct cursor *c)
{
	const uint8_t *b;

	if (!take(c, 4, &b)) {
		return 0;
	}
	return (uint32_t)b[0] | ((uint32_t)b[1] << 8) | ((uint32_t)b[2] << 16) |
	       ((uint32_t)b[3] << 24);
}

static uint64_t take_u64(struct cursor *c)
{
	const uint32_t low = take_u32(c);
	const uint32_t high = take_u32(c);

	return (uint64_t)low | ((uint64_t)high << 32);
}

static float take_f32(struct cursor *c)
{
	const uint32_t bits = take_u32(c);
	float value;

	/* memcpy, not a union or a cast: the only aliasing-safe way to reinterpret
	 * the bits, and GCC folds it to a single VMOV.
	 */
	memcpy(&value, &bits, sizeof(value));
	return value;
}

/* --- decode -------------------------------------------------------------- */

static enum ctrl_load_result decode(struct ctrl_plan *plan, const uint8_t *bytes, uint32_t len)
{
	if (len < DCP_HEADER_LEN) {
		return CTRL_LOAD_TOO_SHORT;
	}
	if (memcmp(bytes, DCP_MAGIC, sizeof(DCP_MAGIC)) != 0) {
		return CTRL_LOAD_BAD_MAGIC;
	}

	struct cursor header = { bytes + 4, DCP_HEADER_LEN - 4, 0, false };

	plan->format_version = take_u16(&header);
	plan->kernel_set_version = take_u16(&header);
	plan->plan_id = take_u64(&header);
	plan->base_ts_ns = take_u64(&header);
	plan->n_blocks = take_u32(&header);
	plan->signal_count = take_u32(&header);
	const uint32_t signal_pool_bytes = take_u32(&header);
	const uint32_t state_pool_bytes = take_u32(&header);

	plan->wcet_estimate_ns = take_u64(&header);
	const uint32_t crc = take_u32(&header);

	plan->state_len = state_pool_bytes / 4;

	/* Version gates come before integrity: a plan from a newer backend is a
	 * more useful diagnosis than the checksum failure it might also produce.
	 */
	if (plan->format_version != CTRL_DCP_FORMAT_VERSION) {
		return CTRL_LOAD_BAD_FORMAT_VERSION;
	}
	if (plan->kernel_set_version > CTRL_DCP_KERNEL_SET_VERSION) {
		return CTRL_LOAD_BAD_KERNEL_SET_VERSION;
	}

	const uint8_t *body = bytes + DCP_HEADER_LEN;
	const uint32_t body_len = len - DCP_HEADER_LEN;

	if (ctrl_crc32(body, body_len) != crc) {
		return CTRL_LOAD_CHECKSUM_MISMATCH;
	}
	if (ctrl_fnv1a64(body, body_len) != plan->plan_id) {
		return CTRL_LOAD_PLAN_ID_MISMATCH;
	}

	/* The pool sizes are what get statically allocated, so they are checked
	 * before anything is written into the fixed-size arrays below.
	 */
	if (plan->n_blocks > CTRL_MAX_BLOCKS || plan->signal_count > CTRL_MAX_SIGNALS ||
	    plan->state_len > CTRL_MAX_STATE) {
		return CTRL_LOAD_TOO_LARGE;
	}
	if (signal_pool_bytes != plan->signal_count * 4U) {
		return CTRL_LOAD_MALFORMED;
	}

	struct cursor c = { body, body_len, 0, false };

	for (uint32_t i = 0; i < plan->n_blocks; i++) {
		struct ctrl_block *block = &plan->blocks[i];

		block->kernel_id = take_u16(&c);
		block->rate_div = take_u16(&c);
		block->param_offset = take_u32(&c);
		block->param_len = take_u16(&c);
		block->state_offset = take_u32(&c);
		block->state_len = take_u16(&c);
		block->output_signal = take_u32(&c);
		block->in_count = take_u16(&c);

		if (block->in_count > CTRL_MAX_INPUTS) {
			return CTRL_LOAD_ARITY_MISMATCH;
		}
		for (uint16_t k = 0; k < block->in_count; k++) {
			block->inputs[k] = take_u32(&c);
		}
	}

	plan->param_count = take_u32(&c);
	if (plan->param_count > CTRL_MAX_PARAMS) {
		return CTRL_LOAD_TOO_LARGE;
	}
	for (uint32_t i = 0; i < plan->param_count; i++) {
		plan->params[i] = take_f32(&c);
	}

	/* v1 emits no io_bindings and this runtime has no HAL channels to bind
	 * them to, so a non-empty section is refused rather than ignored.
	 */
	if (take_u32(&c) != 0) {
		return CTRL_LOAD_IO_BINDINGS_UNSUPPORTED;
	}

	if (c.overrun) {
		return CTRL_LOAD_TOO_SHORT;
	}

	/* Meta (model name, generated_at, backend version) follows. It is
	 * non-executable provenance; the runtime does not retain it.
	 */
	return CTRL_LOAD_OK;
}

/* --- validate ------------------------------------------------------------ */

static enum ctrl_load_result validate(const struct ctrl_plan *plan)
{
	if (plan->base_ts_ns == 0) {
		return CTRL_LOAD_MALFORMED;
	}

	/* The designed real-time gate: a plan whose own estimate does not fit in
	 * its period is refused before it can miss a deadline.
	 *
	 * This is currently VACUOUS - build_control_plan() hardcodes
	 * wcet_estimate_ns to 0 (PROJECT_STATUS.md, "Open decisions"). It is
	 * written now so that stamping a real number at the backend is the only
	 * remaining step, and so the check is not forgotten once it can bite.
	 */
	if (plan->wcet_estimate_ns > plan->base_ts_ns) {
		return CTRL_LOAD_WCET_EXCEEDS_PERIOD;
	}

	for (uint32_t i = 0; i < plan->n_blocks; i++) {
		const struct ctrl_block *block = &plan->blocks[i];
		const struct ctrl_kernel_desc *desc = ctrl_kernel_desc(block->kernel_id);

		if (desc == NULL) {
			return CTRL_LOAD_UNKNOWN_KERNEL;
		}
		if (block->rate_div != 1) {
			return CTRL_LOAD_UNSUPPORTED_RATE_DIV;
		}
		if (block->in_count != desc->in_count) {
			return CTRL_LOAD_ARITY_MISMATCH;
		}

		if (block->output_signal >= plan->signal_count) {
			return CTRL_LOAD_SLOT_OUT_OF_RANGE;
		}
		for (uint16_t k = 0; k < block->in_count; k++) {
			if (block->inputs[k] >= plan->signal_count) {
				return CTRL_LOAD_SLOT_OUT_OF_RANGE;
			}
		}

		if ((uint64_t)block->state_offset + block->state_len > plan->state_len) {
			return CTRL_LOAD_SLOT_OUT_OF_RANGE;
		}
		if ((uint64_t)block->param_offset + block->param_len > plan->param_count) {
			return CTRL_LOAD_SLOT_OUT_OF_RANGE;
		}
		if (block->param_len < desc->min_params) {
			return CTRL_LOAD_PARAMS_TOO_SHORT;
		}

		const float *params = &plan->params[block->param_offset];

		/* Per-kernel structural checks. These pair a block's state length with
		 * what its kernel will actually index, so no kernel has to re-derive it.
		 */
		switch (block->kernel_id) {
		case CTRL_KERNEL_INTEGRATOR:
			if (block->state_len != 1) {
				return CTRL_LOAD_MALFORMED;
			}
			break;

		case CTRL_KERNEL_DELAY:
			/* params[1] is the delay in steps, and it is the state length. */
			if ((float)block->state_len != params[1]) {
				return CTRL_LOAD_MALFORMED;
			}
			break;

		case CTRL_KERNEL_TRANSFER_FUNCTION: {
			const int order = (int)params[0];

			/* The order cap is enforced on both sides. The backend rejects
			 * order > 2 at parse time; this is the second half of that, so a
			 * hand-built or future plan cannot smuggle one past the f32
			 * precision limit documented in kernels.h.
			 */
			if (order < 1 || order > CTRL_MAX_TF_ORDER) {
				return CTRL_LOAD_MALFORMED;
			}
			if (block->param_len < 1 + order * order + 2 * order + 1) {
				return CTRL_LOAD_PARAMS_TOO_SHORT;
			}
			if (block->state_len != order) {
				return CTRL_LOAD_MALFORMED;
			}
			break;
		}

		default:
			if (block->state_len != 0) {
				return CTRL_LOAD_MALFORMED;
			}
			break;
		}
	}

	return CTRL_LOAD_OK;
}

enum ctrl_load_result ctrl_plan_load(struct ctrl_plan *plan, const uint8_t *bytes, uint32_t len)
{
	memset(plan, 0, sizeof(*plan));

	const enum ctrl_load_result decoded = decode(plan, bytes, len);

	if (decoded != CTRL_LOAD_OK) {
		return decoded;
	}
	return validate(plan);
}
