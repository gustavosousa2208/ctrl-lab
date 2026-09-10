/*
 * Loader tests for io_bindings (src/dcp.c).
 *
 * Every case here is a plan that must be REFUSED, and refused by its own name.
 * That matters more than it sounds: a binding that names the wrong block is a
 * hand-edited JSON or a backend bug, and "plan rejected" would send you reading
 * the encoder instead of the one line that is off.
 *
 * The plans are built by mutating a real backend-produced fixture rather than
 * by hand-assembling bytes, so the layout can never drift from what plan.rs
 * actually emits. Mutating the body invalidates both integrity fields, so each
 * case repairs them - which is also a small check that the firmware's crc32 and
 * fnv1a64 still agree with the backend's, since a mistake there would show up
 * as every case failing with the wrong result.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "dcp.h"

/* Header layout, from plan.rs encode(): magic, versions, plan_id, base_ts,
 * counts, wcet, crc32. The body follows.
 */
#define HDR_LEN        52
#define OFF_PLAN_ID     8
#define OFF_CRC32      48

static int failures;

static void check(bool ok, const char *what)
{
	printf("%-62s %s\n", what, ok ? "ok" : "FAIL");
	if (!ok) {
		failures++;
	}
}

static void check_result(enum ctrl_load_result got, enum ctrl_load_result want, const char *what)
{
	const bool ok = got == want;

	printf("%-62s %s\n", what, ok ? "ok" : "FAIL");
	if (!ok) {
		printf("    wanted: %s\n", ctrl_load_result_str(want));
		printf("    got:    %s\n", ctrl_load_result_str(got));
		failures++;
	}
}

/* --- the fixture --------------------------------------------------------- */

static uint8_t plan_bytes[4096];
static uint32_t plan_len;

static void load_fixture(const char *path)
{
	FILE *f = fopen(path, "rb");

	if (f == NULL) {
		printf("cannot open %s\n", path);
		exit(1);
	}
	plan_len = (uint32_t)fread(plan_bytes, 1, sizeof(plan_bytes), f);
	fclose(f);
}

static void put_u32(uint8_t *at, uint32_t value)
{
	at[0] = (uint8_t)(value & 0xffU);
	at[1] = (uint8_t)((value >> 8) & 0xffU);
	at[2] = (uint8_t)((value >> 16) & 0xffU);
	at[3] = (uint8_t)((value >> 24) & 0xffU);
}

static void put_u64(uint8_t *at, uint64_t value)
{
	for (int i = 0; i < 8; i++) {
		at[i] = (uint8_t)((value >> (8 * i)) & 0xffU);
	}
}

/* Re-stamps plan_id and crc32 over the body, so a mutated plan is rejected for
 * the reason under test rather than for the integrity fields it broke.
 */
static void reseal(uint8_t *buf, uint32_t len)
{
	const uint8_t *body = buf + HDR_LEN;
	const uint32_t body_len = len - HDR_LEN;

	put_u64(&buf[OFF_PLAN_ID], ctrl_fnv1a64(body, body_len));
	put_u32(&buf[OFF_CRC32], ctrl_crc32(body, body_len));
}

/* The fixture's two bindings, as they sit on the wire: block 0 on LINK channel
 * 0, and block 3 on LINK channel 1. Found by pattern rather than by a hardcoded
 * offset so that a change to an earlier section does not silently move this
 * test onto the wrong bytes.
 */
static const uint8_t BINDINGS[16] = {
	0x00, 0x00, 0x00, 0x00,  0x01, 0x00,  0x00, 0x00,
	0x03, 0x00, 0x00, 0x00,  0x01, 0x00,  0x01, 0x00,
};

static uint32_t binding_offset(void)
{
	for (uint32_t i = HDR_LEN; i + sizeof(BINDINGS) <= plan_len; i++) {
		if (memcmp(&plan_bytes[i], BINDINGS, sizeof(BINDINGS)) == 0) {
			return i;
		}
	}
	printf("FAIL: could not find the binding section in the fixture\n");
	exit(1);
}

/* Copies the fixture, applies one mutation at `offset` within the binding
 * section, reseals, and loads.
 */
static enum ctrl_load_result load_with_patch(uint32_t at, const uint8_t *patch, uint32_t patch_len)
{
	static uint8_t scratch[sizeof(plan_bytes)];
	static struct ctrl_plan plan;

	memcpy(scratch, plan_bytes, plan_len);
	if (patch != NULL) {
		memcpy(&scratch[at], patch, patch_len);
	}
	reseal(scratch, plan_len);
	return ctrl_plan_load(&plan, scratch, plan_len);
}

/* --- tests --------------------------------------------------------------- */

static void test_valid_plan_loads(void)
{
	static struct ctrl_plan plan;
	const enum ctrl_load_result result = ctrl_plan_load(&plan, plan_bytes, plan_len);

	check_result(result, CTRL_LOAD_OK, "the fixture loads as it stands");
	check(plan.n_io_bindings == 2, "both bindings survive the decode");
	check(plan.io_bindings[0].block_index == 0 &&
	      plan.io_bindings[0].channel_role == CTRL_CHANNEL_LINK &&
	      plan.io_bindings[0].channel_index == 0,
	      "the Input binds LINK channel 0");
	check(plan.io_bindings[1].block_index == 3 &&
	      plan.io_bindings[1].channel_role == CTRL_CHANNEL_LINK &&
	      plan.io_bindings[1].channel_index == 1,
	      "the Output binds LINK channel 1");
	check(plan.blocks[0].kernel_id == CTRL_KERNEL_INPUT &&
	      plan.blocks[0].state_len == 1,
	      "the Input carries the one state word the runtime writes");
}

static void test_block_out_of_range(void)
{
	const uint8_t patch[4] = { 0x63, 0x00, 0x00, 0x00 };	/* block 99 */

	check_result(load_with_patch(binding_offset(), patch, sizeof(patch)),
		     CTRL_LOAD_IO_BLOCK_OUT_OF_RANGE,
		     "a binding on a block that does not exist");
}

static void test_block_is_not_an_io_block(void)
{
	const uint8_t patch[4] = { 0x01, 0x00, 0x00, 0x00 };	/* the gain */

	check_result(load_with_patch(binding_offset(), patch, sizeof(patch)),
		     CTRL_LOAD_IO_BLOCK_NOT_IO,
		     "a binding on the gain, which is neither Input nor Output");
}

static void test_unimplemented_role(void)
{
	const uint8_t patch[2] = { CTRL_CHANNEL_ADC, 0x00 };

	check_result(load_with_patch(binding_offset() + 4, patch, sizeof(patch)),
		     CTRL_LOAD_IO_ROLE_UNSUPPORTED,
		     "a binding on ADC, which is numbered but not implemented");
}

static void test_duplicate_binding(void)
{
	/* Point the second binding at block 0 as well. */
	const uint8_t patch[4] = { 0x00, 0x00, 0x00, 0x00 };

	check_result(load_with_patch(binding_offset() + 8, patch, sizeof(patch)),
		     CTRL_LOAD_IO_DUPLICATE_BINDING,
		     "two bindings naming the same block");
}

/* The case that would otherwise be silent. An unbound Input reads whatever its
 * default happens to be, produces a plausible number every tick, and looks
 * exactly like a working link that is reading zero.
 */
static void test_unbound_io_block(void)
{
	static uint8_t scratch[sizeof(plan_bytes)];
	static struct ctrl_plan plan;

	const uint32_t at = binding_offset();
	const uint32_t count_at = at - 4;	/* the u32 count precedes them */

	memcpy(scratch, plan_bytes, plan_len);
	put_u32(&scratch[count_at], 0);

	/* Drop the binding bytes and pull the meta section up over them. */
	const uint32_t tail = plan_len - (at + (uint32_t)sizeof(BINDINGS));

	memmove(&scratch[at], &scratch[at + sizeof(BINDINGS)], tail);

	const uint32_t shorter = plan_len - (uint32_t)sizeof(BINDINGS);

	reseal(scratch, shorter);
	check_result(ctrl_plan_load(&plan, scratch, shorter), CTRL_LOAD_IO_BLOCK_UNBOUND,
		     "an Input and an Output with no bindings at all");
}

static void test_plans_without_io_still_load(const char *path)
{
	static struct ctrl_plan plan;
	static uint8_t buf[4096];
	FILE *f = fopen(path, "rb");

	if (f == NULL) {
		printf("cannot open %s\n", path);
		exit(1);
	}

	const uint32_t len = (uint32_t)fread(buf, 1, sizeof(buf), f);

	fclose(f);

	check_result(ctrl_plan_load(&plan, buf, len), CTRL_LOAD_OK,
		     "a plan predating this feature still loads unchanged");
	check(plan.n_io_bindings == 0, "and carries no bindings");
	check(plan.kernel_set_version == 1,
	      "and still declares kernel set 1, so v1 firmware keeps running it");
}

int main(int argc, char **argv)
{
	if (argc < 3) {
		printf("usage: %s <06-link-io.plan.dcp> <04-2nd-order-system.plan.dcp>\n", argv[0]);
		return 1;
	}

	load_fixture(argv[1]);

	test_valid_plan_loads();
	test_block_out_of_range();
	test_block_is_not_an_io_block();
	test_unimplemented_role();
	test_duplicate_binding();
	test_unbound_io_block();
	test_plans_without_io_still_load(argv[2]);

	printf("\n%s\n", failures == 0 ? "all ok" : "FAILURES");
	return failures == 0 ? 0 : 1;
}
