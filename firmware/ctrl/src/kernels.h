/*
 * The kernel library: the runtime "ISA".
 *
 * Every kernel is pure, deterministic, allocation-free and bounded-WCET, and is
 * reached through a fixed dispatch table indexed by kernel_id. Adding a block
 * type is a new id plus a table entry plus a kernel_set_version bump - never an
 * engine change.
 *
 * `backend/src/exec.rs` is the executable specification for every function
 * here. Where the two could differ, exec.rs wins; the firmware is graded
 * against the traces it produces, to a 5.8e-6 f32 noise floor.
 */

#ifndef CTRL_KERNELS_H
#define CTRL_KERNELS_H

#include <stdint.h>

#include "dcp.h"

/* Largest transfer-function order the packed state space is trusted to run.
 *
 * Mirrors MAX_TF_ORDER in backend/src/exec.rs, where the number is measured
 * rather than chosen: a dense state space stores the expanded denominator, and
 * in f32 an order-3 clustered-pole filter already drifts ~100x past the project
 * noise floor while order 6 diverges outright.
 *
 * Do not raise this to run higher orders. The backend rejects them too, and the
 * documented path is a second-order-section cascade under a NEW kernel id.
 * Raising the constant trades a loud rejection for a quiet wrong answer.
 */
#define CTRL_MAX_TF_ORDER 2

/* What a kernel is given. Mirrors the kernel_ctx in firmware/AGENTS.md.
 *
 * `inputs` holds the gathered upstream signal *values*, in the kernel's declared
 * port order, so a kernel never touches the signal pool or a slot index.
 */
struct ctrl_kernel_ctx {
	const float *params;
	const float *inputs;
	float *state;
	uint16_t state_len;
	uint32_t tick;
	float time;
	float ts;
};

/* Pass 1 computes an output from state and inputs; pass 2 advances state.
 * Splitting them is not a style choice - see runtime.h on why one fused pass is
 * a different and wrong system.
 */
typedef float (*ctrl_kernel_output_fn)(const struct ctrl_kernel_ctx *ctx);
typedef void (*ctrl_kernel_update_fn)(const struct ctrl_kernel_ctx *ctx);

struct ctrl_kernel_desc {
	const char *name;
	ctrl_kernel_output_fn output;
	/* NULL for a stateless kernel: nothing to advance in pass 2. */
	ctrl_kernel_update_fn update;
	uint8_t in_count;
	/* Minimum packed parameter words. The transfer function needs more than
	 * this - its real requirement depends on the order in params[0] - and the
	 * loader checks that separately.
	 */
	uint8_t min_params;
};

/* A kernel that cannot produce a defined result raises a fault instead of
 * inventing one. exec.rs returns Err in the same places; the firmware cannot
 * unwind mid-tick, so the kernel sets a flag and the scheduler reads it after
 * the call - which is also how the block index gets attached, since a kernel
 * does not know its own position.
 */
enum ctrl_kernel_fault {
	CTRL_KERNEL_FAULT_NONE = 0,
	CTRL_KERNEL_FAULT_INVALID_SWITCH_SELECTOR,
};

void ctrl_kernel_fault_clear(void);
enum ctrl_kernel_fault ctrl_kernel_fault_get(void);
float ctrl_kernel_fault_value(void);
const char *ctrl_kernel_fault_str(enum ctrl_kernel_fault fault);

/* NULL for an id this firmware does not implement. */
const struct ctrl_kernel_desc *ctrl_kernel_desc(uint16_t kernel_id);

#endif /* CTRL_KERNELS_H */
