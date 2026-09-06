/*
 * Deployable Control Plan: the backend->firmware wire format.
 *
 * The decoder for what `backend/src/plan.rs` encodes. That file is the
 * authority on the byte layout; this one must follow it exactly, and the
 * round-trip is checked on device by the plan_id and crc32 the backend stamps.
 *
 * Everything here runs at load time, never inside the control step. The point
 * of validating this hard is that the step afterwards needs no bounds checks:
 * once a plan loads, every slot index in it is known to be in range, so
 * ctrl_step() is straight-line work with a computable WCET.
 */

#ifndef CTRL_DCP_H
#define CTRL_DCP_H

#include <stdbool.h>
#include <stdint.h>

/* Versions this firmware implements. The loader refuses a plan it cannot fully
 * execute rather than running a partially understood one.
 *
 * format_version guards the container; kernel_set_version guards the kernel
 * library. A plan built by a NEWER backend may use kernels this build lacks, so
 * a higher kernel_set_version is rejected; an older one is accepted, because
 * ids are append-only (see plan.rs, "wire-stable: never renumber, only append").
 */
#define CTRL_DCP_FORMAT_VERSION     1
#define CTRL_DCP_KERNEL_SET_VERSION 1

/* Static capacity. Pools are sized once from these, never allocated, and a plan
 * that exceeds any of them is rejected at load. Sized with headroom over the
 * largest committed fixture (04-2nd-order-system: 6 blocks, 6 signals, 4 state
 * words, 33 params).
 */
#define CTRL_MAX_BLOCKS  32
#define CTRL_MAX_SIGNALS 32
#define CTRL_MAX_STATE   64
#define CTRL_MAX_PARAMS  256
/* The widest kernel is Switch, which gathers a, b and sel. */
#define CTRL_MAX_INPUTS  3

/* Wire-stable kernel ids. Mirrors plan.rs KernelId; never renumber. */
enum ctrl_kernel_id {
	CTRL_KERNEL_CONSTANT          = 1,
	CTRL_KERNEL_STEP              = 2,
	CTRL_KERNEL_SQUARE_WAVE       = 3,
	CTRL_KERNEL_GAIN              = 4,
	CTRL_KERNEL_SUM               = 5,
	CTRL_KERNEL_SWITCH            = 6,
	CTRL_KERNEL_DELAY             = 7,
	CTRL_KERNEL_INTEGRATOR        = 8,
	CTRL_KERNEL_TRANSFER_FUNCTION = 9,
	CTRL_KERNEL_SCOPE             = 10,
	CTRL_KERNEL__COUNT
};

/* One scheduled block, already in the backend's topological order. */
struct ctrl_block {
	uint16_t kernel_id;
	uint16_t rate_div;
	uint32_t param_offset;
	uint16_t param_len;
	uint32_t state_offset;
	uint16_t state_len;
	uint32_t output_signal;
	uint16_t in_count;
	uint32_t inputs[CTRL_MAX_INPUTS];
};

/* A loaded plan. Fixed-size throughout: this struct IS the allocation.
 *
 * io_bindings are deliberately absent. The format carries the section and the
 * backend always emits it empty (see PROJECT_STATUS.md, "Open decisions"); the
 * decoder therefore skips it and rejects a non-empty one rather than pretending
 * to bind channels it has no HAL for. Stage E is where that changes.
 */
struct ctrl_plan {
	uint16_t format_version;
	uint16_t kernel_set_version;
	uint64_t plan_id;
	uint64_t base_ts_ns;
	uint64_t wcet_estimate_ns;
	uint32_t n_blocks;
	uint32_t signal_count;
	uint32_t state_len;
	uint32_t param_count;
	struct ctrl_block blocks[CTRL_MAX_BLOCKS];
	float params[CTRL_MAX_PARAMS];
};

enum ctrl_load_result {
	CTRL_LOAD_OK = 0,
	CTRL_LOAD_TOO_SHORT,
	CTRL_LOAD_BAD_MAGIC,
	CTRL_LOAD_BAD_FORMAT_VERSION,
	CTRL_LOAD_BAD_KERNEL_SET_VERSION,
	CTRL_LOAD_CHECKSUM_MISMATCH,
	CTRL_LOAD_PLAN_ID_MISMATCH,
	CTRL_LOAD_UNKNOWN_KERNEL,
	CTRL_LOAD_MALFORMED,
	CTRL_LOAD_TOO_LARGE,
	CTRL_LOAD_SLOT_OUT_OF_RANGE,
	CTRL_LOAD_ARITY_MISMATCH,
	CTRL_LOAD_PARAMS_TOO_SHORT,
	CTRL_LOAD_IO_BINDINGS_UNSUPPORTED,
	CTRL_LOAD_UNSUPPORTED_RATE_DIV,
	CTRL_LOAD_WCET_EXCEEDS_PERIOD,
};

const char *ctrl_load_result_str(enum ctrl_load_result result);

/* Decodes and fully validates `len` bytes into `plan`.
 *
 * On anything but CTRL_LOAD_OK the plan must not be executed; its contents are
 * undefined. Meta strings (model name, generated_at, backend version) are
 * non-executable provenance and are not retained.
 */
enum ctrl_load_result ctrl_plan_load(struct ctrl_plan *plan, const uint8_t *bytes, uint32_t len);

/* Integrity primitives, matching plan.rs byte for byte. Exposed for the
 * self-test in main.c, which pins them against known vectors.
 */
uint32_t ctrl_crc32(const uint8_t *bytes, uint32_t len);
uint64_t ctrl_fnv1a64(const uint8_t *bytes, uint32_t len);

/* Incremental form, for checksumming something larger than memory while it is
 * being written out. Start from CTRL_CRC32_INIT, feed chunks, finish with
 * CTRL_CRC32_FINAL. `ctrl_crc32` is exactly these three steps.
 */
#define CTRL_CRC32_INIT      0xffffffffU
#define CTRL_CRC32_FINAL(c)  (~(c))
uint32_t ctrl_crc32_update(uint32_t crc, const uint8_t *bytes, uint32_t len);

#endif /* CTRL_DCP_H */
