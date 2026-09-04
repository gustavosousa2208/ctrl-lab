/*
 * ctrl-lab bring-up probe.
 *
 * Not the control runtime. This answers the questions stage D depends on,
 * before any of stage D is written:
 *
 *   - do the signal and state pools actually land in DTCM?
 *   - are the caches on, and what do they cost in jitter?
 *   - does the DWT cycle counter work, so a control step can be timed?
 *
 * Build and run notes are in ../BRINGUP.md.
 */

#include <zephyr/kernel.h>
#include <zephyr/linker/section_tags.h>
#include <zephyr/sys/printk.h>

/* Sized for the probe, not for a real plan. The largest committed fixture needs
 * 6 signals and 4 state words; a plan loader will size these from the DCP
 * header instead.
 */
#define MAX_SIGNALS 64
#define MAX_STATE   64

/* __dtcm_bss_section exists only because boards/mini_stm32h743.overlay chooses
 * zephyr,dtcm. Without that overlay these silently fall back to ordinary SRAM
 * and the build still succeeds - check the linker's DTCM usage, not just that
 * it compiled.
 */
static float signal_pool[MAX_SIGNALS] __dtcm_bss_section;
static float state_pool[MAX_STATE] __dtcm_bss_section;

/* Cortex-M7 Data Watchpoint and Trace unit. CYCCNT is the cheapest honest
 * measure of control-step cost: a free-running core-clock counter with no
 * driver, no interrupt, and no timer to share.
 */
#define DWT_CTRL   (*(volatile uint32_t *)0xE0001000UL)
#define DWT_CYCCNT (*(volatile uint32_t *)0xE0001004UL)
#define DEMCR      (*(volatile uint32_t *)0xE000EDFCUL)

#define DEMCR_TRCENA    (1UL << 24)
#define DWT_CTRL_CYCCNT (1UL << 0)

static void cycle_counter_enable(void)
{
	DEMCR |= DEMCR_TRCENA;
	DWT_CYCCNT = 0;
	DWT_CTRL |= DWT_CTRL_CYCCNT;
}

/* Stand-in for a control step: a chain of dependent f32 multiply-accumulates,
 * the same shape as a state-space update. Deliberately dependent so the result
 * reflects FPU latency rather than how well the core pipelines independent work.
 */
static void fake_step(void)
{
	for (int i = 1; i < MAX_STATE; i++) {
		state_pool[i] = state_pool[i - 1] * 0.99f + signal_pool[i] * 0.01f;
	}
}

int main(void)
{
	printk("ctrl-lab bringup probe\n");
	printk("signal_pool @ %p  state_pool @ %p  (DTCM base is 0x20000000)\n",
	       (void *)signal_pool, (void *)state_pool);
	printk("icache=%d dcache=%d\n",
	       IS_ENABLED(CONFIG_ICACHE), IS_ENABLED(CONFIG_DCACHE));
	printk("core %u Hz, kernel tick %u Hz\n",
	       sys_clock_hw_cycles_per_sec(), CONFIG_SYS_CLOCK_TICKS_PER_SEC);

	cycle_counter_enable();
	if (DWT_CYCCNT == 0) {
		printk("WARNING: DWT cycle counter is not running\n");
	}

	for (int i = 0; i < MAX_SIGNALS; i++) {
		signal_pool[i] = (float)i * 0.5f;
	}

	/* Repeat so the spread is visible. With caches on, the first runs are
	 * slower than the rest; that gap is the number stage D has to budget for.
	 */
	uint32_t best = 0xFFFFFFFFU;
	uint32_t worst = 0U;
	uint32_t first = 0U;

	for (int run = 0; run < 100; run++) {
		uint32_t start = DWT_CYCCNT;

		fake_step();

		uint32_t elapsed = DWT_CYCCNT - start;

		if (run == 0) {
			first = elapsed;
		}
		if (elapsed < best) {
			best = elapsed;
		}
		if (elapsed > worst) {
			worst = elapsed;
		}
	}

	printk("%d dependent f32 MACs: first=%u best=%u worst=%u cycles (spread %u)\n",
	       MAX_STATE - 1, first, best, worst, worst - best);
	printk("at 1 kHz a control step may use %u cycles\n",
	       sys_clock_hw_cycles_per_sec() / 1000U);

	return 0;
}
