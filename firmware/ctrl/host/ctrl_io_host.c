/*
 * The host's hardware channels: there are none.
 *
 * Every read fails and every write is discarded, which is not a placeholder but
 * the specification. A failed read leaves the Input block's state word alone,
 * and on the host nothing ever writes it, so it holds the `default` that
 * ctrl_arm put there for the whole run - which is exactly what
 * backend/src/exec.rs evaluates an Input to.
 *
 * That is what lets a plan containing Input and Output still be graded
 * bit-for-bit here against the reference executor. The two agree by
 * construction rather than by coincidence, and the only thing that changes on a
 * board is that reads start succeeding.
 */

#include "ctrl_io.h"

void ctrl_io_begin_tick(void)
{
}

void ctrl_io_end_tick(void)
{
}

bool ctrl_io_read(uint16_t role, uint16_t index, float *out)
{
	(void)role;
	(void)index;
	(void)out;
	return false;
}

bool ctrl_io_write(uint16_t role, uint16_t index, float value)
{
	(void)role;
	(void)index;
	(void)value;
	return true;
}
