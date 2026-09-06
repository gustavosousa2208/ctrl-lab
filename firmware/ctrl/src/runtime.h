/*
 * The control core: static pools, arming, and the deterministic scheduler.
 *
 * # The step is two passes, not one
 *
 * Each tick runs EVERY block's output, then EVERY block's state update:
 *
 *     pass 1:  signals[out] = kernel_output(state, signals[in])
 *     pass 2:  state        = kernel_update(state, signals[in])
 *
 * Fusing them into a single walk is a different and wrong system, and it fails
 * quietly. The backend's topological order covers only direct-feedthrough
 * edges, so a strictly-proper plant is scheduled BEFORE the controller driving
 * it. Its output needs no input, so the early slot is correct - but its state
 * update needs u[k], produced later in the same tick. One fused pass feeds it
 * u[k-1] and inserts a sample of delay into the loop.
 *
 * This is measured, not feared: on test-projects/04-2nd-order-system the fused
 * variant diverges from the reference by 1.2e-2, against an f32 noise floor of
 * 5.8e-6 - about 2000x. backend/src/exec.rs pins it with a test.
 */

#ifndef CTRL_RUNTIME_H
#define CTRL_RUNTIME_H

#include <stdbool.h>
#include <stdint.h>

#include "dcp.h"
#include "kernels.h"

struct ctrl_runtime {
	const struct ctrl_plan *plan;
	uint32_t tick;
	/* base_ts_ns as seconds, in f32 - the same rounding the reference
	 * executor does, because the value feeds the integrator and every
	 * time-based source.
	 */
	float ts;
	/* Set when a step stops early. Meaningless unless ctrl_step() returned
	 * false.
	 */
	enum ctrl_kernel_fault fault;
	uint32_t fault_block;
};

/* Arms a plan: signal pool zeroed, state pool set to each kernel's DECLARED
 * initial condition rather than blindly to zero - an integrator arms to its
 * initialValue, a delay line fills with its initial value. Both come from the
 * packed parameter blob, so there is no separate initial-state section.
 *
 * The plan must have loaded cleanly; arming does not re-validate.
 */
void ctrl_arm(struct ctrl_runtime *rt, const struct ctrl_plan *plan);

/* Advances one control step. Returns false if a kernel faulted, in which case
 * `fault` and `fault_block` say which and where, the tick is NOT advanced, and
 * the signal pool holds a partial result - the caller must go to a safe state
 * rather than step again.
 */
bool ctrl_step(struct ctrl_runtime *rt);

/* Simulated time of the tick that will run next: tick * ts, in f32. */
float ctrl_time(const struct ctrl_runtime *rt);

/* The live signal pool, indexed by signal slot. Valid after ctrl_arm(). */
const float *ctrl_signals(void);

/* Where the pools actually landed, for the DTCM check at boot. */
const void *ctrl_signal_pool_addr(void);
const void *ctrl_state_pool_addr(void);

#endif /* CTRL_RUNTIME_H */
