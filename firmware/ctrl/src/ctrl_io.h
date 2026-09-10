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

/* The tick's edges, so an implementation can be transactional.
 *
 * Without these a channel would be sampled once per READ, and two Input blocks
 * on one channel could straddle the interrupt that publishes a new packet -
 * seeing two different samples inside a single tick. `begin` takes one snapshot
 * of everything inbound and every read for the rest of the tick is served from
 * it.
 *
 * `end` is the other half: writes accumulate, and this publishes them together.
 * A link that sent one packet per Output block would put two packets on the
 * wire for a two-channel plan and halve the rate it can sustain.
 *
 * The runtime calls `begin` before pass 1 and `end` between the passes. An
 * implementation with no channels leaves both empty.
 */
void ctrl_io_begin_tick(void);
void ctrl_io_end_tick(void);

/* `role` is an enum ctrl_channel_role; `index` is the channel within it. Both
 * come from a validated io_binding, so an implementation may assume the role is
 * one it advertises and need only bounds-check the index.
 */
bool ctrl_io_read(uint16_t role, uint16_t index, float *out);
bool ctrl_io_write(uint16_t role, uint16_t index, float value);

#endif /* CTRL_IO_H */
