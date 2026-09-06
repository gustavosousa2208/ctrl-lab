#include <string.h>

#include "runtime.h"

/* The same sources build twice: once for the board, and once natively by
 * firmware/ctrl/host/ so the kernels can be graded against the reference
 * traces without hardware. Off-target there is no DTCM and no Zephyr linker
 * section, so the tag becomes nothing.
 */
#ifdef __ZEPHYR__
#include <zephyr/linker/section_tags.h>
#else
#define __dtcm_bss_section
#endif

/* The hot pools live in tightly-coupled memory: zero wait states, never cached,
 * and therefore not a source of jitter. The bring-up probe established that
 * nucleo_f767zi already chooses zephyr,dtcm in its board .dts, so no overlay is
 * needed - and that the placement must be checked at runtime rather than
 * trusted, because with no chosen node the tag still links and the data just
 * falls back to ordinary SRAM. main.c reads the addresses below and says so.
 */
static float signal_pool[CTRL_MAX_SIGNALS] __dtcm_bss_section;
static float state_pool[CTRL_MAX_STATE] __dtcm_bss_section;

const float *ctrl_signals(void)
{
	return signal_pool;
}

const void *ctrl_signal_pool_addr(void)
{
	return signal_pool;
}

const void *ctrl_state_pool_addr(void)
{
	return state_pool;
}

float ctrl_time(const struct ctrl_runtime *rt)
{
	return (float)rt->tick * rt->ts;
}

void ctrl_arm(struct ctrl_runtime *rt, const struct ctrl_plan *plan)
{
	rt->plan = plan;
	rt->tick = 0;
	rt->ts = (float)plan->base_ts_ns / 1.0e9f;
	rt->fault = CTRL_KERNEL_FAULT_NONE;
	rt->fault_block = 0;

	memset(signal_pool, 0, sizeof(signal_pool));
	memset(state_pool, 0, sizeof(state_pool));

	for (uint32_t i = 0; i < plan->n_blocks; i++) {
		const struct ctrl_block *block = &plan->blocks[i];
		const float *params = &plan->params[block->param_offset];
		float *state = &state_pool[block->state_offset];

		switch (block->kernel_id) {
		case CTRL_KERNEL_INTEGRATOR:
			state[0] = params[0];
			break;

		case CTRL_KERNEL_DELAY:
			for (uint16_t k = 0; k < block->state_len; k++) {
				state[k] = params[0];
			}
			break;

		default:
			/* Everything else arms to zero, which memset already did. */
			break;
		}
	}
}

/* Gathers a block's upstream signal values into `into`, in the kernel's port
 * order, so kernels never see a slot index. Every index was proved in range at
 * load time, so this does no checking.
 */
static void gather(const struct ctrl_block *block, float *into)
{
	for (uint16_t k = 0; k < block->in_count; k++) {
		into[k] = signal_pool[block->inputs[k]];
	}
}

bool ctrl_step(struct ctrl_runtime *rt)
{
	const struct ctrl_plan *plan = rt->plan;
	const float time = ctrl_time(rt);
	float inputs[CTRL_MAX_INPUTS];

	ctrl_kernel_fault_clear();

	/* Pass 1: every block's output. */
	for (uint32_t i = 0; i < plan->n_blocks; i++) {
		const struct ctrl_block *block = &plan->blocks[i];
		const struct ctrl_kernel_desc *desc = ctrl_kernel_desc(block->kernel_id);

		gather(block, inputs);

		const struct ctrl_kernel_ctx ctx = {
			.params = &plan->params[block->param_offset],
			.inputs = inputs,
			.state = &state_pool[block->state_offset],
			.state_len = block->state_len,
			.tick = rt->tick,
			.time = time,
			.ts = rt->ts,
		};

		const float value = desc->output(&ctx);

		/* Checked before the slot is written, so a faulted block leaves the
		 * previous value in place - matching exec.rs, which returns Err
		 * without writing.
		 */
		if (ctrl_kernel_fault_get() != CTRL_KERNEL_FAULT_NONE) {
			rt->fault = ctrl_kernel_fault_get();
			rt->fault_block = i;
			return false;
		}

		signal_pool[block->output_signal] = value;
	}

	/* Pass 2: every block's state update, reading the signals pass 1 just
	 * produced. This is where a strictly-proper block finally sees u[k].
	 */
	for (uint32_t i = 0; i < plan->n_blocks; i++) {
		const struct ctrl_block *block = &plan->blocks[i];
		const struct ctrl_kernel_desc *desc = ctrl_kernel_desc(block->kernel_id);

		if (desc->update == NULL) {
			continue;
		}

		gather(block, inputs);

		const struct ctrl_kernel_ctx ctx = {
			.params = &plan->params[block->param_offset],
			.inputs = inputs,
			.state = &state_pool[block->state_offset],
			.state_len = block->state_len,
			.tick = rt->tick,
			.time = time,
			.ts = rt->ts,
		};

		desc->update(&ctx);
	}

	rt->tick++;
	return true;
}
