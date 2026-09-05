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
#include <zephyr/devicetree.h>
#include <zephyr/linker/section_tags.h>
#include <zephyr/sys/printk.h>

/* Where DTCM actually is, taken from the devicetree rather than hardcoded, so
 * the runtime check below stays correct across boards.
 *
 * nucleo_f767zi already has `zephyr,dtcm = &dtcm` in the chosen block of its
 * board .dts, so unlike the WeAct H743 it needs no overlay. Fail the build,
 * not the run, if some future board forgets it.
 */
#if !DT_HAS_CHOSEN(zephyr_dtcm)
#error "no zephyr,dtcm chosen - add an overlay for this board (see BRINGUP.md)"
#endif
#define DTCM_BASE DT_REG_ADDR(DT_CHOSEN(zephyr_dtcm))
#define DTCM_SIZE DT_REG_SIZE(DT_CHOSEN(zephyr_dtcm))

/* Sized for the probe, not for a real plan. The largest committed fixture needs
 * 6 signals and 4 state words; a plan loader will size these from the DCP
 * header instead.
 */
#define MAX_SIGNALS 64
#define MAX_STATE   64

/* __dtcm_bss_section puts these in tightly-coupled memory: zero wait states and
 * never cached, which is exactly what the control loop's hot pools want.
 *
 * The hazard is that this fails SILENTLY. With nothing choosing zephyr,dtcm the
 * tag still compiles and links, the data just falls back to ordinary SRAM, and
 * the build passes. The #error above catches a missing chosen node; is_in_dtcm()
 * catches the rest, because "the section exists" and "the data ended up in it"
 * are different claims. Read the check at boot rather than trusting the build.
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

static bool is_in_dtcm(const void *p)
{
	uintptr_t a = (uintptr_t)p;

	return a >= DTCM_BASE && a < DTCM_BASE + DTCM_SIZE;
}

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
	printk("board " CONFIG_BOARD_TARGET ", SoC " CONFIG_SOC "\n");

	printk("DTCM is %u KB at 0x%08lx\n",
	       (unsigned int)(DTCM_SIZE / 1024U), (unsigned long)DTCM_BASE);
	printk("signal_pool @ %p  %s\n", (void *)signal_pool,
	       is_in_dtcm(signal_pool) ? "in DTCM" : "*** NOT IN DTCM ***");
	printk("state_pool  @ %p  %s\n", (void *)state_pool,
	       is_in_dtcm(state_pool) ? "in DTCM" : "*** NOT IN DTCM ***");

	printk("icache=%d dcache=%d fpu_dp=%d\n",
	       IS_ENABLED(CONFIG_ICACHE), IS_ENABLED(CONFIG_DCACHE),
	       IS_ENABLED(CONFIG_CPU_HAS_FPU_DOUBLE_PRECISION));
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
