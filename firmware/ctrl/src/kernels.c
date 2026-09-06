/*
 * Kernel implementations, transcribed from backend/src/exec.rs.
 *
 * The transcription is literal on purpose, down to the order of additions. f32
 * addition is not associative, so reassociating a sum - even into an obviously
 * equivalent form - changes the last bits of the result and breaks the
 * bit-for-bit grading this stage exists to do. Where a loop below looks like it
 * could be tightened, that is the reason it has not been.
 *
 * The other half of that contract is -ffp-contract=off in CMakeLists.txt: GCC
 * would otherwise fuse `acc += a * b` into a single VFMA instruction, whose
 * un-rounded intermediate is *more* accurate than the Rust reference and
 * therefore wrong here. See the note there.
 */

#include <float.h>
#include <math.h>
#include <string.h>

#include "kernels.h"

static enum ctrl_kernel_fault fault_code;
static float fault_value;

void ctrl_kernel_fault_clear(void)
{
	fault_code = CTRL_KERNEL_FAULT_NONE;
	fault_value = 0.0f;
}

enum ctrl_kernel_fault ctrl_kernel_fault_get(void)
{
	return fault_code;
}

float ctrl_kernel_fault_value(void)
{
	return fault_value;
}

const char *ctrl_kernel_fault_str(enum ctrl_kernel_fault fault)
{
	switch (fault) {
	case CTRL_KERNEL_FAULT_NONE:
		return "none";
	case CTRL_KERNEL_FAULT_INVALID_SWITCH_SELECTOR:
		return "switch selector must be 0 or 1";
	}
	return "unknown";
}

/* --- sources ------------------------------------------------------------- */

static float constant_output(const struct ctrl_kernel_ctx *ctx)
{
	return ctx->params[0];
}

static float step_output(const struct ctrl_kernel_ctx *ctx)
{
	const float initial = ctx->params[0];
	const float final_value = ctx->params[1];
	const float step_time = ctx->params[2];

	return ctx->time < step_time ? initial : final_value;
}

/* Rust's f32::rem_euclid, not fmodf: they agree for the positive time and
 * period this kernel always sees, but the reference says rem_euclid, so this
 * says rem_euclid.
 */
static float rem_euclid(float value, float rhs)
{
	const float r = fmodf(value, rhs);

	return r < 0.0f ? r + fabsf(rhs) : r;
}

static float square_wave_output(const struct ctrl_kernel_ctx *ctx)
{
	const float amplitude = ctx->params[0];
	const float frequency = ctx->params[1];
	const float duty = ctx->params[2];

	if (duty <= FLT_EPSILON) {
		return 0.0f;
	}
	if (fabsf(100.0f - duty) <= FLT_EPSILON) {
		return amplitude;
	}

	const float period = 1.0f / frequency;
	const float high = period * (duty / 100.0f);

	return rem_euclid(ctx->time, period) < high ? amplitude : 0.0f;
}

/* --- combinational ------------------------------------------------------- */

static float gain_output(const struct ctrl_kernel_ctx *ctx)
{
	return ctx->inputs[0] * ctx->params[0];
}

/* The Sum block carries its two operators as numeric codes, matching
 * operator_code() in plan.rs: 0 '+', 1 '-', 2 '*', 3 '/'.
 */
static char operator_of(float code)
{
	switch ((int)code) {
	case 1:
		return '-';
	case 2:
		return '*';
	case 3:
		return '/';
	default:
		return '+';
	}
}

static float divide_safely(float dividend, float divisor)
{
	return divisor == 0.0f ? 0.0f : dividend / divisor;
}

static float sum_output(const struct ctrl_kernel_ctx *ctx)
{
	const float a = ctx->inputs[0];
	const float b = ctx->inputs[1];
	const char left = operator_of(ctx->params[0]);
	const char right = operator_of(ctx->params[1]);

	if (left == '+' && right == '-') {
		return a - b;
	}
	if (left == '-' && right == '+') {
		return b - a;
	}
	if (left == '*' && right == '*') {
		return a * b;
	}
	if (left == '*' && right == '/') {
		return divide_safely(a, b);
	}
	if (left == '/' && right == '*') {
		return divide_safely(b, a);
	}
	return a + b;
}

static float switch_output(const struct ctrl_kernel_ctx *ctx)
{
	const float a = ctx->inputs[0];
	const float b = ctx->inputs[1];
	const float selector = ctx->inputs[2];

	if (fabsf(selector) <= FLT_EPSILON) {
		return a;
	}
	if (fabsf(selector - 1.0f) <= FLT_EPSILON) {
		return b;
	}

	fault_code = CTRL_KERNEL_FAULT_INVALID_SWITCH_SELECTOR;
	fault_value = selector;
	return 0.0f;
}

static float scope_output(const struct ctrl_kernel_ctx *ctx)
{
	return ctx->inputs[0];
}

/* --- stateful ------------------------------------------------------------ */

/* A zero-step delay carries no state and passes straight through; otherwise the
 * oldest buffered sample leaves the queue.
 */
static float delay_output(const struct ctrl_kernel_ctx *ctx)
{
	return ctx->state_len == 0 ? ctx->inputs[0] : ctx->state[0];
}

static void delay_update(const struct ctrl_kernel_ctx *ctx)
{
	if (ctx->state_len == 0) {
		return;
	}

	memmove(&ctx->state[0], &ctx->state[1], (size_t)(ctx->state_len - 1) * sizeof(float));
	ctx->state[ctx->state_len - 1] = ctx->inputs[0];
}

static float integrator_output(const struct ctrl_kernel_ctx *ctx)
{
	return ctx->state[0];
}

static void integrator_update(const struct ctrl_kernel_ctx *ctx)
{
	ctx->state[0] += ctx->inputs[0] * ctx->ts;
}

/* Packed layout: [order, Ad row-major (order^2), Bd (order), C (order), D]. */

/* y[k] = C x[k] + D u[k] */
static float transfer_function_output(const struct ctrl_kernel_ctx *ctx)
{
	const int order = (int)ctx->params[0];
	const float *c = &ctx->params[1 + order * order + order];
	const float d = ctx->params[1 + order * order + 2 * order];

	float output = d * ctx->inputs[0];

	for (int i = 0; i < order; i++) {
		output += c[i] * ctx->state[i];
	}
	return output;
}

/* x[k+1] = Ad x[k] + Bd u[k]
 *
 * Bd u first, then the Ad row ascending: exec.rs accumulates in exactly this
 * order and any other gives different last bits.
 */
static void transfer_function_update(const struct ctrl_kernel_ctx *ctx)
{
	const int order = (int)ctx->params[0];
	const float *ad = &ctx->params[1];
	const float *bd = &ctx->params[1 + order * order];
	const float input = ctx->inputs[0];
	float next[CTRL_MAX_TF_ORDER];

	for (int row = 0; row < order; row++) {
		float accumulator = bd[row] * input;

		for (int column = 0; column < order; column++) {
			accumulator += ad[row * order + column] * ctx->state[column];
		}
		next[row] = accumulator;
	}

	for (int row = 0; row < order; row++) {
		ctx->state[row] = next[row];
	}
}

/* --- dispatch ------------------------------------------------------------ */

/* Indexed by kernel id, so entry 0 is the unused "no such kernel" slot. */
static const struct ctrl_kernel_desc descriptors[CTRL_KERNEL__COUNT] = {
	[CTRL_KERNEL_CONSTANT]    = { "constant",    constant_output,          NULL,                     0, 1 },
	[CTRL_KERNEL_STEP]        = { "step",        step_output,              NULL,                     0, 3 },
	[CTRL_KERNEL_SQUARE_WAVE] = { "squareWave",  square_wave_output,       NULL,                     0, 3 },
	[CTRL_KERNEL_GAIN]        = { "gain",        gain_output,              NULL,                     1, 1 },
	[CTRL_KERNEL_SUM]         = { "sum",         sum_output,               NULL,                     2, 2 },
	[CTRL_KERNEL_SWITCH]      = { "switch",      switch_output,            NULL,                     3, 0 },
	[CTRL_KERNEL_DELAY]       = { "delay",       delay_output,             delay_update,             1, 2 },
	[CTRL_KERNEL_INTEGRATOR]  = { "integrator",  integrator_output,        integrator_update,        1, 1 },
	[CTRL_KERNEL_TRANSFER_FUNCTION] =
				    { "transferFunction", transfer_function_output, transfer_function_update, 1, 1 },
	[CTRL_KERNEL_SCOPE]       = { "scope",       scope_output,             NULL,                     1, 0 },
};

const struct ctrl_kernel_desc *ctrl_kernel_desc(uint16_t kernel_id)
{
	if (kernel_id >= CTRL_KERNEL__COUNT) {
		return NULL;
	}

	const struct ctrl_kernel_desc *desc = &descriptors[kernel_id];

	/* Entry 0, and any gap a future append leaves, has no output function. */
	return desc->output != NULL ? desc : NULL;
}
