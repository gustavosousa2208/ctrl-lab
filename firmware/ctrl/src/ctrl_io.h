/*
 * The hardware channel boundary.
 *
 * The control core knows nothing about peripherals; this is the whole of what
 * it may say to one. `runtime.c` calls these at the tick boundary - every read
 * before pass 1, every write between the passes - and nothing in `kernels.c`
 * calls them at all, which is what keeps the kernels pure and keeps two blocks
 * reading one channel in a tick from seeing two different samples.
 *
 * Two implementations exist:
 *
 *   host   returns false from every read and discards every write, so a plan
 *          carrying Input and Output still grades bit-for-bit against
 *          backend/src/exec.rs. A failed read leaves the Input's state word
 *          alone, and on the host that word never leaves its `default`.
 *   link   maps CTRL_CHANNEL_LINK onto the MCU-to-MCU link.
 *
 * A read that returns false is not an error the runtime reports. It means "no
 * fresh sample", and the Input holds its last good value - a zero-order hold,
 * which is what a control loop should do with a late packet and what the host
 * does with no link at all.
 */

#ifndef CTRL_IO_H
#define CTRL_IO_H

#include <stdbool.h>
#include <stdint.h>

/* `role` is an enum ctrl_channel_role; `index` is the channel within it. Both
 * come from a validated io_binding, so an implementation may assume the role is
 * one it advertises and need only bounds-check the index.
 */
bool ctrl_io_read(uint16_t role, uint16_t index, float *out);
bool ctrl_io_write(uint16_t role, uint16_t index, float value);

#endif /* CTRL_IO_H */
