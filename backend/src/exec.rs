//! Reference execution of a [`ControlPlan`] in `f32`.
//!
//! This is the executable specification for the firmware control core. It runs a
//! plan exactly the way the microcontroller must: a flat `f32` signal pool, a
//! flat `f32` state pool, blocks walked in `blocks[]` order, one function per
//! [`KernelId`], no allocation inside the step, and no `f64` anywhere.
//!
//! Firmware kernels are graded against the traces this produces. If the two
//! disagree, the firmware is wrong — this file is the contract.
//!
//! # The step is two passes, not one
//!
//! Each tick runs **all** block outputs first, then **all** state updates:
//!
//! ```text
//! pass 1:  for block in blocks:  signals[out] = kernel_output(state, signals[in])
//! pass 2:  for block in blocks:  state = kernel_update(state, signals[in])
//! ```
//!
//! A single fused pass gives a different — and wrong — system. Consider a
//! strictly-proper plant `P` inside a feedback loop. Because the topological sort
//! only orders *direct-feedthrough* edges, `P` runs **before** the controller `C`
//! that feeds it. `P` has no feedthrough, so its output at tick `k` needs no
//! input and the early position is fine. But its state update needs `u[k]`, the
//! controller output produced *later in the same tick*. Fusing the passes would
//! feed it `u[k-1]` instead, silently inserting a one-sample delay into the loop
//! and changing the closed-loop dynamics.
//!
//! `firmware/AGENTS.md` currently describes a single walk of `blocks[]`. That
//! description is incomplete; the scheduler needs both passes.

use crate::plan::{BlockRecord, ControlPlan, KernelId};

/// Reason a plan could not be executed.
#[derive(Debug, Clone, PartialEq)]
pub enum ExecError {
    /// A switch selector was neither ~0 nor ~1.
    InvalidSwitchSelector { block_index: usize, value: f32 },
    /// A block's packed parameters were shorter than its kernel requires.
    MalformedParams { block_index: usize, kernel: KernelId },
    /// A block referenced a signal or state slot outside the declared pools.
    SlotOutOfRange { block_index: usize },
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSwitchSelector { block_index, value } => write!(
                f,
                "block {block_index}: switch selector must be 0 or 1, got {value}"
            ),
            Self::MalformedParams {
                block_index,
                kernel,
            } => write!(
                f,
                "block {block_index}: packed parameters too short for kernel {kernel:?}"
            ),
            Self::SlotOutOfRange { block_index } => {
                write!(f, "block {block_index}: signal or state slot out of range")
            }
        }
    }
}

impl std::error::Error for ExecError {}

/// A plan under execution: the signal pool, the state pool, and the tick count.
///
/// Both pools are sized once from the plan header and never reallocated, so the
/// memory profile matches what the firmware statically allocates.
#[derive(Debug, Clone)]
pub struct PlanExecutor {
    signals: Vec<f32>,
    state: Vec<f32>,
    tick: u32,
    ts: f32,
}

impl PlanExecutor {
    /// Arms a plan: pools sized from the header, signals zeroed, and state set to
    /// each kernel's declared initial condition (an integrator's `initialValue`,
    /// a delay's fill value) rather than blindly to zero.
    pub fn new(plan: &ControlPlan) -> Self {
        let mut executor = Self {
            signals: vec![0.0; plan.signal_count as usize],
            state: vec![0.0; plan.state_len as usize],
            tick: 0,
            ts: plan.base_ts_ns as f32 / 1.0e9,
        };
        executor.arm(plan);
        executor
    }

    fn arm(&mut self, plan: &ControlPlan) {
        for block in &plan.blocks {
            let params = params_of(plan, block);
            let base = block.state_offset as usize;
            let len = block.state_len as usize;
            match block.kernel_id {
                KernelId::Integrator => {
                    if len == 1 {
                        self.state[base] = params.first().copied().unwrap_or(0.0);
                    }
                }
                KernelId::Delay => {
                    let initial = params.first().copied().unwrap_or(0.0);
                    for slot in &mut self.state[base..base + len] {
                        *slot = initial;
                    }
                }
                _ => {}
            }
        }
    }

    /// Current signal pool, indexed by signal slot.
    pub fn signals(&self) -> &[f32] {
        &self.signals
    }

    /// Simulated time of the tick that will run next.
    pub fn time(&self) -> f32 {
        self.tick as f32 * self.ts
    }

    /// Advances one control step: every block's output, then every block's state.
    pub fn step(&mut self, plan: &ControlPlan) -> Result<(), ExecError> {
        let time = self.time();

        for (index, block) in plan.blocks.iter().enumerate() {
            let value = self.output_of(plan, block, index, time)?;
            let slot = block.output_signal as usize;
            *self
                .signals
                .get_mut(slot)
                .ok_or(ExecError::SlotOutOfRange { block_index: index })? = value;
        }

        for (index, block) in plan.blocks.iter().enumerate() {
            self.update_of(plan, block, index)?;
        }

        self.tick = self.tick.wrapping_add(1);
        Ok(())
    }

    fn input(&self, block: &BlockRecord, port: usize, index: usize) -> Result<f32, ExecError> {
        let slot = *block
            .inputs
            .get(port)
            .ok_or(ExecError::SlotOutOfRange { block_index: index })? as usize;
        self.signals
            .get(slot)
            .copied()
            .ok_or(ExecError::SlotOutOfRange { block_index: index })
    }

    fn output_of(
        &self,
        plan: &ControlPlan,
        block: &BlockRecord,
        index: usize,
        time: f32,
    ) -> Result<f32, ExecError> {
        let p = params_of(plan, block);
        let short = || ExecError::MalformedParams {
            block_index: index,
            kernel: block.kernel_id,
        };

        Ok(match block.kernel_id {
            KernelId::Constant => *p.first().ok_or_else(short)?,

            KernelId::Step => {
                let (initial, final_value, step_time) =
                    (*p.first().ok_or_else(short)?, *p.get(1).ok_or_else(short)?, *p.get(2).ok_or_else(short)?);
                if time < step_time {
                    initial
                } else {
                    final_value
                }
            }

            KernelId::SquareWave => {
                let (amplitude, frequency, duty) =
                    (*p.first().ok_or_else(short)?, *p.get(1).ok_or_else(short)?, *p.get(2).ok_or_else(short)?);
                square_wave(amplitude, frequency, duty, time)
            }

            KernelId::Gain => self.input(block, 0, index)? * *p.first().ok_or_else(short)?,

            KernelId::Sum => {
                let a = self.input(block, 0, index)?;
                let b = self.input(block, 1, index)?;
                let left = operator_of(*p.first().ok_or_else(short)?);
                let right = operator_of(*p.get(1).ok_or_else(short)?);
                apply_equation(left, right, a, b)
            }

            KernelId::Switch => {
                let a = self.input(block, 0, index)?;
                let b = self.input(block, 1, index)?;
                let selector = self.input(block, 2, index)?;
                if selector.abs() <= f32::EPSILON {
                    a
                } else if (selector - 1.0).abs() <= f32::EPSILON {
                    b
                } else {
                    return Err(ExecError::InvalidSwitchSelector {
                        block_index: index,
                        value: selector,
                    });
                }
            }

            // Zero delay steps carries no state and passes the input straight
            // through; otherwise the oldest buffered sample leaves the queue.
            KernelId::Delay => {
                if block.state_len == 0 {
                    self.input(block, 0, index)?
                } else {
                    self.state[block.state_offset as usize]
                }
            }

            KernelId::Integrator => self.state[block.state_offset as usize],

            // y[k] = C x[k] + D u[k]
            KernelId::TransferFunction => {
                let (order, _ad, c, d) = state_space_params(p, index, block.kernel_id)?;
                let x = &self.state[block.state_offset as usize..][..order];
                let mut output = d * self.input(block, 0, index)?;
                for (gain, value) in c.iter().zip(x.iter()) {
                    output += gain * value;
                }
                output
            }

            KernelId::Scope => self.input(block, 0, index)?,
        })
    }

    fn update_of(
        &mut self,
        plan: &ControlPlan,
        block: &BlockRecord,
        index: usize,
    ) -> Result<(), ExecError> {
        match block.kernel_id {
            KernelId::Integrator => {
                let input = self.input(block, 0, index)?;
                self.state[block.state_offset as usize] += input * self.ts;
            }

            KernelId::Delay => {
                if block.state_len > 0 {
                    let input = self.input(block, 0, index)?;
                    let base = block.state_offset as usize;
                    let len = block.state_len as usize;
                    self.state.copy_within(base + 1..base + len, base);
                    self.state[base + len - 1] = input;
                }
            }

            // x[k+1] = Ad x[k] + Bd u[k]
            KernelId::TransferFunction => {
                let p = params_of(plan, block);
                let (order, ad, _c, _d) = state_space_params(p, index, block.kernel_id)?;
                let bd = &p[1 + order * order..][..order];
                let input = self.input(block, 0, index)?;
                let base = block.state_offset as usize;

                let mut next = [0.0f32; MAX_TF_ORDER];
                for row in 0..order {
                    let mut accumulator = bd[row] * input;
                    for (column, value) in self.state[base..base + order].iter().enumerate() {
                        accumulator += ad[row * order + column] * value;
                    }
                    next[row] = accumulator;
                }
                self.state[base..base + order].copy_from_slice(&next[..order]);
            }

            _ => {}
        }
        Ok(())
    }
}

/// Largest transfer-function order the fixed-size update scratch buffer holds.
/// Raising it costs stack, not heap; the firmware has the same constant.
const MAX_TF_ORDER: usize = 8;

fn params_of<'a>(plan: &'a ControlPlan, block: &BlockRecord) -> &'a [f32] {
    let start = block.param_offset as usize;
    let end = start + block.param_len as usize;
    &plan.params[start..end]
}

/// Unpacks `[order, Ad row-major, Bd, C, D]`, returning everything but `Bd`.
fn state_space_params(
    params: &[f32],
    index: usize,
    kernel: KernelId,
) -> Result<(usize, &[f32], &[f32], f32), ExecError> {
    let short = || ExecError::MalformedParams {
        block_index: index,
        kernel,
    };
    let order = *params.first().ok_or_else(short)? as usize;
    if order == 0 || order > MAX_TF_ORDER || params.len() < 1 + order * order + 2 * order + 1 {
        return Err(short());
    }
    let ad = &params[1..][..order * order];
    let c = &params[1 + order * order + order..][..order];
    let d = params[1 + order * order + 2 * order];
    Ok((order, ad, c, d))
}

fn operator_of(code: f32) -> char {
    match code as i32 {
        1 => '-',
        2 => '*',
        3 => '/',
        _ => '+',
    }
}

fn apply_equation(left: char, right: char, a: f32, b: f32) -> f32 {
    match (left, right) {
        ('+', '-') => a - b,
        ('-', '+') => b - a,
        ('*', '*') => a * b,
        ('*', '/') => divide_safely(a, b),
        ('/', '*') => divide_safely(b, a),
        _ => a + b,
    }
}

fn divide_safely(dividend: f32, divisor: f32) -> f32 {
    if divisor == 0.0 {
        0.0
    } else {
        dividend / divisor
    }
}

fn square_wave(amplitude: f32, frequency: f32, duty: f32, time: f32) -> f32 {
    if duty <= f32::EPSILON {
        return 0.0;
    }
    if (100.0 - duty).abs() <= f32::EPSILON {
        return amplitude;
    }
    let period = 1.0 / frequency;
    let high = period * (duty / 100.0);
    if time.rem_euclid(period) < high {
        amplitude
    } else {
        0.0
    }
}

/// A recorded run: one `f32` series per signal slot, plus the tick times.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecTrace {
    pub times: Vec<f32>,
    /// `signals[slot][k]` — indexed by signal slot, then by tick.
    pub signals: Vec<Vec<f32>>,
}

/// Arms a plan and runs it for `steps` ticks, recording every signal.
pub fn run(plan: &ControlPlan, steps: usize) -> Result<ExecTrace, ExecError> {
    let mut executor = PlanExecutor::new(plan);
    let mut times = Vec::with_capacity(steps);
    let mut signals = vec![Vec::with_capacity(steps); plan.signal_count as usize];

    for _ in 0..steps {
        times.push(executor.time());
        executor.step(plan)?;
        for (slot, series) in signals.iter_mut().enumerate() {
            series.push(executor.signals()[slot]);
        }
    }

    Ok(ExecTrace { times, signals })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::build_control_plan;
    use crate::{parse_project_json, simulate_validated_dag, ValidatedDag};

    const FIXTURES: [&str; 4] = [
        "01-double-integrator",
        "02-feedback-TF",
        "03-TF-test",
        "04-2nd-order-system",
    ];

    fn fixture(name: &str) -> ValidatedDag {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("test-projects")
            .join(format!("{name}.json"));
        parse_project_json(&std::fs::read_to_string(path).expect("fixture must be readable"))
            .expect("fixture must parse")
    }

    /// The whole point of this module: the f32 plan executor must reproduce the
    /// f64 simulator. Reports the worst divergence per fixture rather than
    /// asserting a tolerance pulled out of the air.
    #[test]
    fn matches_the_f64_simulator_on_every_fixture() {
        let mut worst_overall = 0.0f64;

        for name in FIXTURES {
            let dag = fixture(name);
            let plan = build_control_plan(&dag).expect("plan must build");
            let reference = simulate_validated_dag(&dag).expect("simulation must succeed");
            let trace = run(&plan, reference.times.len()).expect("execution must succeed");

            let mut worst = 0.0f64;
            let mut worst_signal = String::new();
            for (slot, node_id) in dag.topological_order.iter().enumerate() {
                let expected = &reference.values_by_node_id[node_id];
                for (k, value) in trace.signals[slot].iter().enumerate() {
                    let delta = (expected[k] - *value as f64).abs();
                    if delta > worst {
                        worst = delta;
                        worst_signal = node_id.clone();
                    }
                }
            }

            eprintln!("{name}: max |f64 - f32| = {worst:.3e} (worst signal: {worst_signal})");
            worst_overall = worst_overall.max(worst);

            assert!(
                worst < 1.0e-2,
                "{name}: f32 executor diverged from the simulator by {worst:.3e} \
                 on `{worst_signal}` - far beyond f32 rounding, so this is a \
                 kernel or packing bug, not precision loss"
            );
        }

        eprintln!("worst divergence across all fixtures: {worst_overall:.3e}");
    }

    /// Fixture 04 is a closed loop whose plant is strictly proper and therefore
    /// scheduled *before* the controller feeding it. If the two passes were
    /// fused, the plant would integrate `u[k-1]` instead of `u[k]` and the loop
    /// would visibly change. This pins the ordering that prevents that.
    #[test]
    fn fusing_the_two_passes_would_change_the_closed_loop() {
        let dag = fixture("04-2nd-order-system");
        let plan = build_control_plan(&dag).expect("plan must build");
        let reference = simulate_validated_dag(&dag).expect("simulation must succeed");
        let steps = reference.times.len();

        let correct = run(&plan, steps).expect("execution must succeed");

        // Same plan, but each block's state updated immediately after its own
        // output - the naive single-pass loop.
        let mut fused = PlanExecutor::new(&plan);
        let mut fused_plant = Vec::with_capacity(steps);
        for _ in 0..steps {
            let time = fused.time();
            for (index, block) in plan.blocks.iter().enumerate() {
                let value = fused
                    .output_of(&plan, block, index, time)
                    .expect("output must evaluate");
                fused.signals[block.output_signal as usize] = value;
                fused
                    .update_of(&plan, block, index)
                    .expect("update must evaluate");
            }
            fused.tick += 1;
            fused_plant.push(fused.signals[0]);
        }

        let divergence = correct.signals[0]
            .iter()
            .zip(fused_plant.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);

        assert!(
            divergence > 1.0e-3,
            "fusing the passes should visibly change the loop, but the traces \
             differ by only {divergence:.3e} - the ordering guard is not \
             actually being exercised by this fixture"
        );
        eprintln!("two-pass vs fused divergence on the plant output: {divergence:.3e}");
    }

    #[test]
    fn arms_state_to_declared_initial_conditions() {
        let dag = fixture("01-double-integrator");
        let plan = build_control_plan(&dag).expect("plan must build");
        let executor = PlanExecutor::new(&plan);

        let integrators: Vec<&BlockRecord> = plan
            .blocks
            .iter()
            .filter(|block| block.kernel_id == KernelId::Integrator)
            .collect();
        assert!(!integrators.is_empty(), "fixture must contain integrators");

        for block in integrators {
            let expected = plan.params[block.param_offset as usize];
            assert_eq!(
                executor.state[block.state_offset as usize], expected,
                "integrator state must arm to its initialValue, not to zero"
            );
        }
    }

    #[test]
    fn signal_pool_persists_across_ticks() {
        // A feedback edge reads a slot whose producer runs later in the order,
        // so it must hold the previous tick's value rather than being cleared.
        let dag = fixture("04-2nd-order-system");
        let plan = build_control_plan(&dag).expect("plan must build");
        let mut executor = PlanExecutor::new(&plan);

        // The fixture's step fires at t = 1 s and Ts = 0.05, so nothing moves
        // for the first 20 ticks. Drive past that before asking for motion.
        for _ in 0..25 {
            executor.step(&plan).expect("step must execute");
        }
        let excited: Vec<f32> = executor.signals().to_vec();
        assert!(
            excited.iter().any(|value| value.abs() > 0.0),
            "the loop must be excited once past the step time"
        );

        executor.step(&plan).expect("step must execute");
        assert_ne!(
            excited,
            executor.signals().to_vec(),
            "an excited loop must keep evolving between ticks"
        );
    }
}
