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
#include <zephyr/sys/barrier.h>
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
#define DWT_LAR    (*(volatile uint32_t *)0xE0001FB0UL)
#define DWT_LSR    (*(volatile uint32_t *)0xE0001FB4UL)
#define DEMCR      (*(volatile uint32_t *)0xE000EDFCUL)

#define DEMCR_TRCENA     (1UL << 24)
#define DWT_CTRL_CYCCNT  (1UL << 0)
#define DWT_CTRL_NOCYCCNT (1UL << 25)  /* set = CYCCNT not implemented */

#define DWT_LSR_PRESENT (1UL << 0)     /* a Lock Access Register exists */
#define DWT_LSR_LOCKED  (1UL << 1)     /* and writes are currently locked */
#define DWT_LAR_UNLOCK  0xC5ACCE55UL

static bool is_in_dtcm(const void *p)
{
	uintptr_t a = (uintptr_t)p;

	return a >= DTCM_BASE && a < DTCM_BASE + DTCM_SIZE;
}

/* Returns false if this core cannot count cycles at all, so a caller never
 * reports a measurement of zero as if it were a fast one.
 *
 * The first version of this probe did only TRCENA + CYCCNTENA, which is the
 * sequence every Cortex-M3/M4 tutorial gives, and CYCCNT stayed at 0 on the
 * F767. On Cortex-M7 the DWT is behind a CoreSight Lock Access Register: while
 * it is locked, writes to DWT registers are silently discarded -- enabling the
 * counter appears to succeed and simply does nothing. Zephyr's own
 * arch/arm/include/cortex_m/dwt.h does the same unlock, guarded the same way.
 *
 * Zephyr also offers this as a supported service via CONFIG_TIMING_FUNCTIONS
 * (timing_init/timing_counter_get, DWT-backed on Cortex-M). The probe stays
 * self-contained on purpose, but the control runtime should prefer that API:
 * it is portable to cores whose DWT is laid out differently.
 */
/* The LSR as found at entry, before we unlock anything. Reading it after the
 * unlock only ever shows "unlocked" and hides why the unlock was needed.
 */
static uint32_t dwt_lsr_at_entry;

/* Put the DWT back to something close to its cold state, so the enable
 * sequences below can be compared against each other rather than against
 * whatever the previous attempt left behind.
 */
static void dwt_disable(void)
{
	DWT_CTRL &= ~DWT_CTRL_CYCCNT;
	DWT_CYCCNT = 0;
	DEMCR &= ~DEMCR_TRCENA;
	barrier_dsync_fence_full();
}

/* The minimal enable sequence, with or without barriers, and nothing else.
 *
 * This exists to answer a question the first run raised rather than to be used:
 * the naive sequence (the one in every Cortex-M3/M4 tutorial) left CYCCNT dead
 * on this part, and the working version changed several things at once. Running
 * both here isolates which one mattered instead of guessing.
 */
static bool dwt_try_enable(bool serialize)
{
	DEMCR |= DEMCR_TRCENA;

	if (serialize) {
		barrier_dsync_fence_full();
	}

	DWT_CYCCNT = 0;
	DWT_CTRL |= DWT_CTRL_CYCCNT;

	if (serialize) {
		barrier_dsync_fence_full();
	}

	uint32_t c0 = DWT_CYCCNT;
	uint32_t c1 = DWT_CYCCNT;

	return c1 != c0;
}

static bool cycle_counter_enable(void)
{
	DEMCR |= DEMCR_TRCENA;

	dwt_lsr_at_entry = DWT_LSR;

	/* Unlock only if a LAR is present and currently locked. On parts with no
	 * LAR the LSR reads as absent and this is correctly skipped.
	 */
	if ((dwt_lsr_at_entry & DWT_LSR_PRESENT) != 0U) {
		if ((dwt_lsr_at_entry & DWT_LSR_LOCKED) != 0U) {
			DWT_LAR = DWT_LAR_UNLOCK;
		}
	}

	if ((DWT_CTRL & DWT_CTRL_NOCYCCNT) != 0U) {
		return false;
	}

	DWT_CYCCNT = 0;
	DWT_CTRL |= DWT_CTRL_CYCCNT;
	barrier_dsync_fence_full();

	/* Two reads rather than one compared against zero. The single-read check
	 * is a race: the enabling write can still be in flight when the read
	 * issues, so a working counter intermittently reports itself dead. It did
	 * exactly that on this board -- "DEAD" alongside a plausible nonzero
	 * measurement, which is how the race was noticed. Two reads of a running
	 * counter always differ, because the read itself costs cycles.
	 */
	uint32_t c0 = DWT_CYCCNT;
	uint32_t c1 = DWT_CYCCNT;

	return c1 != c0;
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

	/* Which part of the fix actually mattered. Cold first, then barriers only. */
	bool naive_ok = dwt_try_enable(false);

	dwt_disable();

	bool barrier_ok = dwt_try_enable(true);

	dwt_disable();

	printk("DWT enable: naive=%s barriers-only=%s\n",
	       naive_ok ? "counts" : "DEAD", barrier_ok ? "counts" : "DEAD");

	bool timed = cycle_counter_enable();

	printk("DWT: lsr_at_entry=0x%08x (%s) ctrl=0x%08x cyccnt=%s\n",
	       (unsigned int)dwt_lsr_at_entry,
	       (dwt_lsr_at_entry & DWT_LSR_LOCKED) ? "was locked, unlocked it"
						   : "was already unlocked",
	       (unsigned int)DWT_CTRL, timed ? "running" : "DEAD");
	if (!timed) {
		printk("WARNING: no cycle counter - every timing below is meaningless\n");
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

#ifdef CONFIG_RTT_CONSOLE
	/* Do not let the CPU idle when the console is RTT.
	 *
	 * RTT is read out of target RAM by the debug probe over SWD, so it only
	 * works while the debug port is alive. Returning from main() lets Zephyr's
	 * idle thread execute WFI, and on the STM32H743 that takes the core domain
	 * off the debug bus: J-Link then reports "DAP initialized successfully"
	 * followed by "Can not attach to CPU", and OpenOCD reads
	 * "Cortex-M PARTNO 0x0". The board looks bricked and is not - but the only
	 * way back in is BOOT0 plus a power cycle, which is a miserable thing to
	 * need after every run.
	 *
	 * CONFIG_STM32_ENABLE_DEBUG_SLEEP_STOP is the documented fix and is set in
	 * rtt.conf, but it did not hold on this part. Measured on hardware
	 * 2026-09-06: the core still became unreachable after the probe finished.
	 *
	 * So the probe simply never finishes. A busy loop costs nothing here - this
	 * is a bring-up probe that has already printed everything it has to say -
	 * and it keeps the debug port up so the transcript can actually be read.
	 *
	 * The F767 does not need this: its console is a UART, readable with no
	 * debugger attached at all, which is why this is guarded.
	 */
	printk("\nholding the CPU awake so RTT stays readable (see main.c)\n");
	while (1) {
		/* Deliberately not k_sleep(): sleeping is the thing being avoided. */
		arch_nop();
	}
#endif

	return 0;
}
