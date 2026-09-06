/*
 * Host harness: the firmware control core, built natively and run off-target.
 *
 * This exists because the board and the repository are not always on the same
 * machine - and more importantly because "it compiles" is a much weaker claim
 * than "it computes the right numbers". Everything under ../src/ is portable C
 * with no Zephyr dependency except one linker section tag, so the identical
 * sources run here against the identical plans.
 *
 * What this proves: the loader, the two-pass scheduler and every kernel agree
 * with backend/src/exec.rs, bit for bit, on the committed fixtures.
 *
 * What this does NOT prove: anything about the target FPU, DTCM, cache
 * behaviour or timing. The device run is still required - this only means a
 * numerical bug is caught before the board is involved.
 *
 * Output is byte-identical in shape to the device's, so one grader reads both.
 */

#include <stdio.h>
#include <stdlib.h>

#include "dcp.h"
#include "runtime.h"
#include "trace.h"

static struct ctrl_plan plan;
static struct ctrl_runtime runtime;

int main(int argc, char **argv)
{
	if (argc != 3) {
		fprintf(stderr, "usage: %s <plan.dcp> <steps>\n", argv[0]);
		return 2;
	}

	FILE *file = fopen(argv[1], "rb");

	if (file == NULL) {
		fprintf(stderr, "cannot open %s\n", argv[1]);
		return 2;
	}

	static uint8_t blob[64 * 1024];
	const size_t len = fread(blob, 1, sizeof(blob), file);

	fclose(file);

	const long steps = strtol(argv[2], NULL, 10);

	if (steps <= 0) {
		fprintf(stderr, "steps must be positive\n");
		return 2;
	}

	const enum ctrl_load_result loaded = ctrl_plan_load(&plan, blob, (uint32_t)len);

	if (loaded != CTRL_LOAD_OK) {
		fprintf(stderr, "plan rejected: %s\n", ctrl_load_result_str(loaded));
		return 1;
	}

	printf("plan_id=0x%016llx\n", (unsigned long long)plan.plan_id);
	printf("base_ts_ns=%llu\n", (unsigned long long)plan.base_ts_ns);
	printf("blocks=%u signals=%u state=%u params=%u\n", plan.n_blocks, plan.signal_count,
	       plan.state_len, plan.param_count);
	printf("steps=%ld\n", steps);

	ctrl_arm(&runtime, &plan);

	struct ctrl_trace_hash hash;

	ctrl_trace_hash_init(&hash);

	const float *signals = ctrl_signals();

	/* The reference records the time BEFORE the step and the signals AFTER
	 * it (exec.rs `run`). Row k is therefore (k*ts, result of step k), and
	 * getting this backwards is an off-by-one that looks like a delay bug.
	 */
	for (long k = 0; k < steps; k++) {
		const float time = ctrl_time(&runtime);

		if (!ctrl_step(&runtime)) {
			fprintf(stderr, "fault at step %ld, block %u: %s\n", k, runtime.fault_block,
				ctrl_kernel_fault_str(runtime.fault));
			return 1;
		}

		printf("T,%08x", ctrl_f32_bits(time));
		ctrl_trace_hash_push(&hash, time);

		for (uint32_t slot = 0; slot < plan.signal_count; slot++) {
			printf(",%08x", ctrl_f32_bits(signals[slot]));
			ctrl_trace_hash_push(&hash, signals[slot]);
		}
		printf("\n");
	}

	printf("trace_fnv1a64=0x%016llx\n", (unsigned long long)ctrl_trace_hash_value(&hash));
	return 0;
}
