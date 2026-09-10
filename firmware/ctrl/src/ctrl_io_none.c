/*
 * No hardware channels on this build.
 *
 * The control app does not own a link yet: `firmware/link` does, and binding
 * the two is ctrl-lab-ri0.3. Until then a plan carrying Input and Output blocks
 * loads, arms and runs here, with every Input holding the `default` its
 * parameter declares.
 *
 * That is the same behaviour as the host harness, deliberately, so this build
 * and `firmware/ctrl/host` produce identical traces for the same plan. It is
 * also why this file is named for what it does rather than called a stub: a
 * board running it is not a board with a broken link, it is a board with no
 * link bound, and the loop it closes is the one through its own defaults.
 */

#include "ctrl_io.h"

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
