/*
 * ctrl-lab control runtime - stage D.
 *
 * Loads a Deployable Control Plan, arms it, runs it for the same number of
 * ticks as the committed reference trace, and prints the result in a form that
 * can be graded bit-for-bit against backend/src/exec.rs.
 *
 * This is the first thing in the project that executes a plan on hardware. What
 * it is NOT yet: driven by a hardware timer. The steps run back to back as fast
 * as the core manages, because stage D's question is "are the numbers right and
 * what does a step cost", not "can we hit a 1 kHz deadline". Wiring the tick to
 * a timer is the next step and needs the measurement below to size the budget.
 *
 * Grade a run with:
 *   firmware/scripts/console.ps1 > run.txt          (Windows, board attached)
 *   python3 firmware/scripts/grade-trace.py \
 *       test-projects/<plan>.f32.csv run.txt
 */

#include <zephyr/kernel.h>
#include <zephyr/devicetree.h>
#include <zephyr/drivers/uart.h>
#include <zephyr/sys/printk.h>
#include <zephyr/timing/timing.h>

#include "dcp.h"
#include "kernels.h"
#include "plan_blob.h"
#include "runtime.h"
#include "trace.h"

#if !DT_HAS_CHOSEN(zephyr_dtcm)
#error "no zephyr,dtcm chosen - add an overlay for this board (see BRINGUP.md)"
#endif
#define DTCM_BASE DT_REG_ADDR(DT_CHOSEN(zephyr_dtcm))
#define DTCM_SIZE DT_REG_SIZE(DT_CHOSEN(zephyr_dtcm))

/* The recording buffers, deliberately in ordinary .bss rather than DTCM.
 *
 * The signal and state pools belong in DTCM because they are the hot path. This
 * does not: it is written once per tick and read only at the end, and putting a
 * ~68 KB buffer in a 128 KB DTCM would crowd out the thing that actually
 * benefits. It also gives the cache A/B something to measure for the first
 * time - see caches-off.conf.
 */
static float trace_signals[CTRL_PLAN_STEPS][CTRL_MAX_SIGNALS];
static float trace_times[CTRL_PLAN_STEPS];
static uint32_t step_cycles[CTRL_PLAN_STEPS];

static struct ctrl_plan plan;
static struct ctrl_runtime runtime;

/* Raw bytes to the console UART.
 *
 * printk cannot carry a binary frame - it formats - so the frame goes straight
 * at the device the console is already using. uart_poll_out is synchronous, so
 * it stays correctly ordered with the printk lines around it and needs no
 * interrupt handler of its own (which the control path is better off without).
 */
#ifndef CTRL_TRACE_TEXT
static const struct device *const console_uart = DEVICE_DT_GET(DT_CHOSEN(zephyr_console));

static void uart_sink(void *ctx, const uint8_t *bytes, uint32_t len)
{
	ARG_UNUSED(ctx);

	for (uint32_t i = 0; i < len; i++) {
		uart_poll_out(console_uart, bytes[i]);
	}
}
#endif

static bool is_in_dtcm(const void *p)
{
	const uintptr_t a = (uintptr_t)p;

	return a >= DTCM_BASE && a < DTCM_BASE + DTCM_SIZE;
}

/* The integrity primitives decide whether a plan is executed at all, so they
 * are checked against known-answer vectors before the plan is trusted. A
 * miscompiled CRC that rejects every plan is obvious; one that ACCEPTS a
 * corrupt plan is not, and this is the cheap guard against it.
 */
static bool integrity_self_test(void)
{
	static const uint8_t vector[] = "123456789";
	const uint32_t crc = ctrl_crc32(vector, sizeof(vector) - 1);
	const uint64_t fnv = ctrl_fnv1a64(vector, sizeof(vector) - 1);
	const bool crc_ok = crc == 0xcbf43926U;
	const bool fnv_ok = fnv == 0x06d5573923c6cdfcULL;

	printk("crc32(\"123456789\")   = 0x%08x %s\n", crc, crc_ok ? "ok" : "*** WRONG ***");
	printk("fnv1a64(\"123456789\") = 0x%08x%08x %s\n", (uint32_t)(fnv >> 32), (uint32_t)fnv,
	       fnv_ok ? "ok" : "*** WRONG ***");

	return crc_ok && fnv_ok;
}

int main(void)
{
	printk("\nctrl-lab control runtime (stage D)\n");
	printk("board " CONFIG_BOARD_TARGET ", SoC " CONFIG_SOC "\n");
	printk("icache=%d dcache=%d fpu_dp=%d\n", IS_ENABLED(CONFIG_ICACHE),
	       IS_ENABLED(CONFIG_DCACHE), IS_ENABLED(CONFIG_CPU_HAS_FPU_DOUBLE_PRECISION));
	printk("core %u Hz, kernel ticks %u Hz, irq_lock=%d\n",
	       sys_clock_hw_cycles_per_sec(), CONFIG_SYS_CLOCK_TICKS_PER_SEC,
	       IS_ENABLED(CTRL_IRQ_LOCK));

	printk("signal_pool @ %p  %s\n", ctrl_signal_pool_addr(),
	       is_in_dtcm(ctrl_signal_pool_addr()) ? "in DTCM" : "*** NOT IN DTCM ***");
	printk("state_pool  @ %p  %s\n", ctrl_state_pool_addr(),
	       is_in_dtcm(ctrl_state_pool_addr()) ? "in DTCM" : "*** NOT IN DTCM ***");
	printk("trace_buf   @ %p  %s (expected: cacheable SRAM)\n", (void *)trace_signals,
	       is_in_dtcm(trace_signals) ? "in DTCM" : "not in DTCM");

	if (!integrity_self_test()) {
		printk("FAIL: integrity primitives are miscompiled, refusing to load\n");
		return 0;
	}

	const enum ctrl_load_result loaded =
		ctrl_plan_load(&plan, ctrl_plan_blob, sizeof(ctrl_plan_blob));

	printk("\nplan " CTRL_PLAN_NAME " (%u bytes): %s\n", (unsigned int)sizeof(ctrl_plan_blob),
	       ctrl_load_result_str(loaded));
	if (loaded != CTRL_LOAD_OK) {
		printk("FAIL: plan rejected\n");
		return 0;
	}

	printk("plan_id=0x%08x%08x format=%u kernels=%u\n", (uint32_t)(plan.plan_id >> 32),
	       (uint32_t)plan.plan_id, plan.format_version, plan.kernel_set_version);
	printk("base_ts_ns=%u blocks=%u signals=%u state=%u params=%u\n",
	       (uint32_t)plan.base_ts_ns, plan.n_blocks, plan.signal_count, plan.state_len,
	       plan.param_count);
	printk("wcet_estimate_ns=%u (0 = not yet stamped by the backend)\n",
	       (uint32_t)plan.wcet_estimate_ns);

	if (plan.signal_count > CTRL_MAX_SIGNALS) {
		printk("FAIL: more signals than the trace buffer holds\n");
		return 0;
	}

	timing_init();
	timing_start();

	ctrl_arm(&runtime, &plan);

	const float *signals = ctrl_signals();
	uint32_t completed = 0;

	/* The reference records the time BEFORE the step and the signals AFTER it
	 * (exec.rs `run`). Row k is (k*ts, result of step k).
	 *
	 * Printing happens after the loop, not inside it: the console is a 115200
	 * baud UART and interleaving it with the measurement would time the
	 * console rather than the control step.
	 */
	for (uint32_t k = 0; k < CTRL_PLAN_STEPS; k++) {
		trace_times[k] = ctrl_time(&runtime);

		/* Not const: timing_cycles_get() takes non-const pointers. */
#ifdef CTRL_IRQ_LOCK
		const unsigned int key = irq_lock();
#endif
		timing_t start = timing_counter_get();
		const bool ok = ctrl_step(&runtime);
		timing_t end = timing_counter_get();
#ifdef CTRL_IRQ_LOCK
		irq_unlock(key);
#endif

		step_cycles[k] = (uint32_t)timing_cycles_get(&start, &end);

		if (!ok) {
			printk("FAIL: fault at step %u, block %u: %s\n", k, runtime.fault_block,
			       ctrl_kernel_fault_str(runtime.fault));
			break;
		}

		for (uint32_t slot = 0; slot < plan.signal_count; slot++) {
			trace_signals[k][slot] = signals[slot];
		}
		completed++;
	}

	uint64_t digest;

#ifdef CTRL_TRACE_TEXT
	struct ctrl_trace_hash hash;

	ctrl_trace_hash_init(&hash);

	printk("\ntrace_begin steps=%u signals=%u\n", completed, plan.signal_count);
	for (uint32_t k = 0; k < completed; k++) {
		printk("T,%08x", ctrl_f32_bits(trace_times[k]));
		ctrl_trace_hash_push(&hash, trace_times[k]);

		for (uint32_t slot = 0; slot < plan.signal_count; slot++) {
			printk(",%08x", ctrl_f32_bits(trace_signals[k][slot]));
			ctrl_trace_hash_push(&hash, trace_signals[k][slot]);
		}
		printk("\n");
	}
	printk("trace_end\n");
	digest = ctrl_trace_hash_value(&hash);
#else
	struct ctrl_trace_writer writer;

	/* The marker is for a human reading the console. The reader finds the
	 * frame by its magic and length, not by this line.
	 */
	printk("\ntrace_frame steps=%u signals=%u bytes=%u\n", completed, plan.signal_count,
	       (unsigned int)(CTRL_TRACE_HEADER_LEN +
			      completed * (1U + plan.signal_count) * 4U +
			      CTRL_TRACE_TRAILER_LEN));

	ctrl_trace_begin(&writer, uart_sink, NULL, plan.plan_id, (uint16_t)plan.signal_count,
			 completed);
	for (uint32_t k = 0; k < completed; k++) {
		ctrl_trace_row(&writer, trace_times[k], trace_signals[k]);
	}
	digest = ctrl_trace_end(&writer);
	printk("\n");
#endif
	printk("trace_fnv1a64=0x%08x%08x\n", (uint32_t)(digest >> 32), (uint32_t)digest);

	/* Step cost. `max` is the number that belongs in wcet_estimate_ns, and the
	 * spread is the jitter metric the project commits to reporting.
	 */
	uint32_t best = UINT32_MAX;
	uint32_t worst = 0;
	uint64_t total = 0;

	for (uint32_t k = 0; k < completed; k++) {
		best = MIN(best, step_cycles[k]);
		worst = MAX(worst, step_cycles[k]);
		total += step_cycles[k];
	}

	/* The first step is reported separately because it is not the same
	 * measurement as the rest: it runs with a cold I-cache and an untrained
	 * branch predictor, and it is the only step whose state pool has never been
	 * written. Folding it into `max` would report a startup cost as if it were
	 * the worst-case control step, which is exactly the number that later goes
	 * into wcet_estimate_ns.
	 */
	uint32_t steady_worst = 0;
	uint32_t worst_at = 0;
	uint32_t outliers = 0;

	for (uint32_t k = 1; k < completed; k++) {
		if (step_cycles[k] > steady_worst) {
			steady_worst = step_cycles[k];
			worst_at = k;
		}
		/* Anything half again as long as the fastest step did not simply run
		 * slowly - something interrupted it. Counting these separates "the
		 * kernel tick landed inside a step" from "the step itself is variable",
		 * which need completely different fixes.
		 */
		if (step_cycles[k] > best + best / 2U) {
			outliers++;
		}
	}

	if (completed > 0) {
		const uint32_t hz = sys_clock_hw_cycles_per_sec();
		const uint32_t mean = (uint32_t)(total / completed);

		printk("\nstep_cycles min=%u mean=%u max=%u spread=%u\n", best, mean, worst,
		       worst - best);
		printk("first_step  %u cycles; steady max=%u at step %u, jitter=%u (%u%%)\n",
		       step_cycles[0], steady_worst, worst_at, steady_worst - best,
		       steady_worst ? (steady_worst - best) * 100U / steady_worst : 0U);
		printk("outliers    %u of %u steps took >1.5x the fastest step\n", outliers,
		       completed - 1);
		printk("step_ns     min=%u mean=%u max=%u\n",
		       (uint32_t)((uint64_t)best * 1000000000U / hz),
		       (uint32_t)((uint64_t)mean * 1000000000U / hz),
		       (uint32_t)((uint64_t)worst * 1000000000U / hz));
		printk("budget      base_ts_ns=%u, so max step is %u%% of the period\n",
		       (uint32_t)plan.base_ts_ns,
		       (uint32_t)((uint64_t)worst * 1000000000U / hz * 100U /
				  (plan.base_ts_ns ? plan.base_ts_ns : 1)));
	}

	printk("\ndone\n");
	return 0;
}
