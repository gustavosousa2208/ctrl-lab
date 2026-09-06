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
#include <zephyr/sys/atomic.h>
#ifdef CONFIG_RTT_CONSOLE
#include <SEGGER_RTT.h>
#endif
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
#ifdef CONFIG_RTT_CONSOLE

/* On an RTT console there is no UART to write to - the console is a ring buffer
 * in target RAM that the debug probe reads over SWD - so the frame goes at RTT
 * directly. It lands in DTCM (CONFIG_SEGGER_RTT_SECTION_DTCM) where it can be
 * read back with mem8, which is how the H743 is graded.
 */
static void trace_sink(void *ctx, const uint8_t *bytes, uint32_t len)
{
	ARG_UNUSED(ctx);
	SEGGER_RTT_Write(0, bytes, len);
}

#else
static const struct device *const console_uart = DEVICE_DT_GET(DT_CHOSEN(zephyr_console));

static void trace_sink(void *ctx, const uint8_t *bytes, uint32_t len)
{
	ARG_UNUSED(ctx);

	for (uint32_t i = 0; i < len; i++) {
		uart_poll_out(console_uart, bytes[i]);
	}
}
#endif /* CONFIG_RTT_CONSOLE */
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

/* One control step, recorded and timed. Shared by both run modes so the thing
 * being measured is identical whether the tick comes from a timer or from the
 * top of a loop.
 *
 * The reference records the time BEFORE the step and the signals AFTER it
 * (exec.rs `run`), so row k is (k*ts, result of step k).
 */
static bool run_one_step(uint32_t k)
{
	const float *signals = ctrl_signals();

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
		return false;
	}

	for (uint32_t slot = 0; slot < plan.signal_count; slot++) {
		trace_signals[k][slot] = signals[slot];
	}
	return true;
}

#ifdef CTRL_FREE_RUN

/* Steps back to back, as fast as the core manages. Not a control loop - this is
 * the mode every measurement before the timer landed was taken in, kept because
 * it answers "are the numbers right" in a second rather than in real time
 * (fixture 04 at its real 50 ms tick takes 25 s).
 */
static uint32_t run_plan(void)
{
	for (uint32_t k = 0; k < CTRL_PLAN_STEPS; k++) {
		if (!run_one_step(k)) {
			return k;
		}
	}
	return CTRL_PLAN_STEPS;
}

#else

/* Timer-driven: the plan's own base_ts_ns becomes the tick.
 *
 * The step runs in a dedicated cooperative thread woken by the timer, not in
 * the timer ISR itself. Two reasons, and the first is a correctness one:
 *
 *   - Floating point in an ISR needs CONFIG_FPU_SHARING, because without it the
 *     callee-saved FP registers (s16-s31) are not preserved and an ISR doing FP
 *     silently corrupts the interrupted thread's context. Cortex-M lazy stacking
 *     covers only s0-s15. A thread sidesteps the whole question.
 *   - firmware/AGENTS.md lists "high-priority Zephyr thread / ISR-triggered
 *     work" as the intended shape, and a cooperative priority means nothing
 *     short of an interrupt can preempt the step.
 *
 * The cost is one context switch of latency per tick, which is itself worth
 * measuring rather than assuming - the tick-to-tick figures below include it.
 */
#define CTRL_THREAD_STACK_SIZE 2048

K_THREAD_STACK_DEFINE(ctrl_thread_stack, CTRL_THREAD_STACK_SIZE);
static struct k_thread ctrl_thread;
static struct k_timer tick_timer;
static K_SEM_DEFINE(tick_signal, 0, 1);
static K_SEM_DEFINE(run_complete, 0, 1);

/* Ticks raised by the ISR, and ticks the thread actually serviced. They diverge
 * only if a step overran its period, which is the definition of a missed
 * deadline - so the difference is the measurement, not a bookkeeping detail.
 */
static atomic_t ticks_raised;
static uint32_t ticks_missed;
static uint32_t completed_steps;
static uint32_t tick_deltas[CTRL_PLAN_STEPS];
static uint32_t awake_deltas[CTRL_PLAN_STEPS];

static void tick_isr(struct k_timer *timer)
{
	ARG_UNUSED(timer);
	atomic_inc(&ticks_raised);
	k_sem_give(&tick_signal);
}

static void control_thread(void *a, void *b, void *c)
{
	ARG_UNUSED(a);
	ARG_UNUSED(b);
	ARG_UNUSED(c);

	uint32_t previous = k_cycle_get_32();
	timing_t previous_awake = timing_counter_get();
	uint32_t serviced = 0;

	while (completed_steps < CTRL_PLAN_STEPS) {
		k_sem_take(&tick_signal, K_FOREVER);

		/* The semaphore saturates at 1, so a tick raised while the
		 * previous step was still running is lost rather than queued.
		 * Comparing the ISR's count against our own is what makes that
		 * visible instead of silent.
		 */
		timing_t now_awake = timing_counter_get();
		const uint32_t raised = (uint32_t)atomic_get(&ticks_raised);

		serviced++;
		if (raised > serviced) {
			ticks_missed += raised - serviced;
			serviced = raised;
		}

		/* k_cycle_get_32(), NOT the DWT-backed timing API used for the step.
		 *
		 * Between ticks the core has nothing to run and Zephyr's idle thread
		 * executes WFI, which gates the core clock - and DWT CYCCNT counts
		 * core clock cycles, so it simply stops. Measuring the tick period
		 * with it reported ~11 900 cycles for a 50 ms period, which is not
		 * the period at all: it is the time the CPU was *awake*. The kernel
		 * cycle counter is driven by the system timer and keeps running
		 * across idle, so it can see the sleep.
		 *
		 * Both numbers are wanted, and they answer different questions:
		 * awake-cycles-per-tick is the CPU cost, this is the period.
		 */
		const uint32_t now = k_cycle_get_32();

		tick_deltas[completed_steps] = now - previous;
		awake_deltas[completed_steps] = (uint32_t)timing_cycles_get(&previous_awake, &now_awake);
		previous = now;
		previous_awake = now_awake;

		if (!run_one_step(completed_steps)) {
			break;
		}
		completed_steps++;
	}

	k_timer_stop(&tick_timer);
	k_sem_give(&run_complete);
}

static uint32_t run_plan(void)
{
	k_timer_init(&tick_timer, tick_isr, NULL);

	k_thread_create(&ctrl_thread, ctrl_thread_stack, CTRL_THREAD_STACK_SIZE, control_thread,
			NULL, NULL, NULL, K_PRIO_COOP(0), 0, K_NO_WAIT);
	k_thread_name_set(&ctrl_thread, "control");

	/* CTRL_TICK_NS overrides the scheduling period ONLY. runtime.ts still comes
	 * from the plan, so the arithmetic is unchanged and the trace still has to
	 * match the reference - this drives the loop faster or slower than the model
	 * it is executing, which is exactly what is wanted to exercise the
	 * missed-deadline path.
	 */
#ifdef CTRL_TICK_NS
	const uint64_t tick_ns = CTRL_TICK_NS;

	printk("tick        %u ns (OVERRIDDEN; plan says %u ns), timer-driven\n",
	       (uint32_t)tick_ns, (uint32_t)plan.base_ts_ns);
#else
	const uint64_t tick_ns = plan.base_ts_ns;

	printk("tick        %u ns from the plan, timer-driven\n", (uint32_t)tick_ns);
#endif

	k_timer_start(&tick_timer, K_NSEC(tick_ns), K_NSEC(tick_ns));
	k_sem_take(&run_complete, K_FOREVER);

	return completed_steps;
}

#endif /* CTRL_FREE_RUN */

int main(void)
{
	printk("\nctrl-lab control runtime (stage D)\n");
	printk("board " CONFIG_BOARD_TARGET ", SoC " CONFIG_SOC "\n");
	printk("icache=%d dcache=%d fpu_dp=%d\n", IS_ENABLED(CONFIG_ICACHE),
	       IS_ENABLED(CONFIG_DCACHE), IS_ENABLED(CONFIG_CPU_HAS_FPU_DOUBLE_PRECISION));
	printk("core %u Hz, kernel ticks %u Hz, irq_lock=%d\n",
	       sys_clock_hw_cycles_per_sec(), CONFIG_SYS_CLOCK_TICKS_PER_SEC,
	       IS_ENABLED(CTRL_IRQ_LOCK));
	printk("run mode    %s\n",
	       IS_ENABLED(CTRL_FREE_RUN) ? "free-running (no timer)" : "timer-driven");

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

	const uint32_t completed = run_plan();

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

	ctrl_trace_begin(&writer, trace_sink, NULL, plan.plan_id, (uint16_t)plan.signal_count,
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

#ifndef CTRL_FREE_RUN
		/* Tick-to-tick, which is a different question from step cost: it
		 * measures when the step STARTED, so it includes timer accuracy
		 * and the wake-up latency of the control thread. This is the
		 * jitter figure the project commits to reporting.
		 */
#ifdef CTRL_TICK_NS
		const uint64_t nominal_ns = CTRL_TICK_NS;
#else
		const uint64_t nominal_ns = plan.base_ts_ns;
#endif
		const uint32_t expected = (uint32_t)(nominal_ns * hz / 1000000000U);
		uint32_t tick_best = UINT32_MAX;
		uint32_t tick_worst = 0;
		uint64_t tick_total = 0;

		/* Skip index 0: its "previous" timestamp is from before the timer
		 * started, so the first delta measures start-up, not a period.
		 */
		uint64_t awake_total = 0;
		uint32_t awake_worst = 0;

		for (uint32_t k = 1; k < completed; k++) {
			tick_best = MIN(tick_best, tick_deltas[k]);
			tick_worst = MAX(tick_worst, tick_deltas[k]);
			tick_total += tick_deltas[k];
			awake_worst = MAX(awake_worst, awake_deltas[k]);
			awake_total += awake_deltas[k];
		}

		if (completed > 1) {
			const uint32_t tick_mean = (uint32_t)(tick_total / (completed - 1));
			const uint32_t awake_mean = (uint32_t)(awake_total / (completed - 1));

			printk("tick_period min=%u mean=%u max=%u expected=%u cycles\n", tick_best,
			       tick_mean, tick_worst, expected);
			printk("tick_jitter %u cycles p-p (%u ns), mean is %d cycles off nominal\n",
			       tick_worst - tick_best,
			       (uint32_t)((uint64_t)(tick_worst - tick_best) * 1000000000U / hz),
			       (int32_t)(tick_mean - expected));

			/* Awake cycles per tick: the step plus the timer ISR, the
			 * semaphore and two context switches. The difference between
			 * this and the step alone is what the scheduling costs.
			 */
			printk("cpu_awake   mean=%u max=%u cycles per tick (step is %u of that)\n",
			       awake_mean, awake_worst, mean);
			/* Against the MEASURED period, not the requested one.
			 *
			 * k_timer resolution is one kernel tick, so a requested
			 * period is rounded UP to a whole tick - at the default
			 * 10 kHz, anything under 100 us silently becomes 100 us.
			 * Dividing by the request rather than the reality reports
			 * loads above 100% for a loop that is comfortably idle,
			 * which is exactly what it did before this line changed.
			 */
			printk("cpu_load    %u.%02u%% of the measured period is spent awake\n",
			       tick_mean ? awake_mean * 100U / tick_mean : 0U,
			       tick_mean ? (uint32_t)((uint64_t)awake_mean * 10000U / tick_mean) % 100U
					 : 0U);
			if (tick_mean > expected + expected / 100U) {
				printk("NOTE        requested %u ns but the timer delivered %u ns"
				       " - k_timer rounds up to a whole kernel tick\n",
				       (uint32_t)nominal_ns,
				       (uint32_t)((uint64_t)tick_mean * 1000000000U / hz));
			}
		}
		printk("deadlines   %u missed of %u ticks\n", ticks_missed, completed);
#endif
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

#ifdef CONFIG_RTT_CONSOLE
	/* Same reason as the bring-up probe: RTT is read over SWD, and letting
	 * Zephyr idle into WFI takes the H743's core domain off the debug bus,
	 * after which the board cannot be attached or reflashed without BOOT0 and
	 * a power cycle. See firmware/BRINGUP.md.
	 */
	printk("holding the CPU awake so RTT stays readable\n");
	while (1) {
		arch_nop();
	}
#endif

	return 0;
}
