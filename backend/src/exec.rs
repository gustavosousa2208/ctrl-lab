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
//! `firmware/AGENTS.md` documents both passes, and `firmware/ctrl/src/runtime.c`
//! implements them. The three are checked against each other by the trace
//! digests below rather than by reading.

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

/// Largest transfer-function order a packed state space is trusted to run.
///
/// This is a numerical limit, not a memory one. Both transfer-function paths
/// pack into one dense state space and step it in f32, and
/// `transfer_function_order_is_capped_at_two` measures how far that drifts from
/// the f64 simulator as order rises. Against the project's 5.8e-6 noise floor,
/// with a repeated pole at z = 0.95:
///
/// ```text
///   order 1: 1.1e-6      order 5: 1.6e-1
///   order 2: 3.5e-6      order 6: 8.4e+5   <- diverged
///   order 3: 5.7e-4      order 7: 5.0e+8
///   order 4: 1.7e-2      order 8: 1.5e+13
/// ```
///
/// Well-separated poles survive to about order 4 (3.1e-6, then 1.8e-5 at
/// order 5), but the cap has to hold for the worst shape a user can draw, not
/// the best. Order 2 is the last one safe everywhere — and it costs nothing
/// today, because a discrete PID *is* a second-order discrete transfer function
/// and every model in `test-projects/` is order 1 or 2.
///
/// **If you need higher order, do not raise this.** Add the second-order-section
/// cascade kernel that `firmware/AGENTS.md` describes and give it a new
/// `KernelId`; keeping the roots factored is the entire point of that form.
/// Raising the number instead buys silently wrong answers.
///
/// To re-measure, raise this and relax the matching check in
/// `parse_discrete_transfer_function`, then run the sweep with `--nocapture`.
const MAX_TF_ORDER: usize = 2;

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

/// FNV-1a64 over the raw little-endian bits of every sample in a trace, in
/// emission order: each row's time, then that row's signals by slot.
///
/// This is how a firmware trace is graded bit-for-bit. Comparing against the
/// committed `NN-*.f32.csv` can only ever support a *tolerance* claim, because
/// nine decimal places do not always round-trip an f32 — a value of 1e-8 prints
/// as `0.000000010`. Hashing the bits sidesteps the text entirely, and both
/// `firmware/ctrl/host/` and the device compute the same digest over the same
/// bytes (see `firmware/ctrl/src/trace.c`).
///
/// FNV-1a rather than a checksum because `plan.rs` already uses it for
/// `plan_id`: one hash in the project, not two.
pub fn trace_digest(trace: &ExecTrace) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut push = |value: f32| {
        for byte in value.to_le_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
    };

    for (k, time) in trace.times.iter().enumerate() {
        push(*time);
        for series in &trace.signals {
            push(series[k]);
        }
    }
    hash
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

    /// The firmware grading contract, pinned.
    ///
    /// These digests are what `firmware/ctrl/` must reproduce - the host
    /// harness natively, and the board over its console. They were verified
    /// bit-for-bit against the C control core on 2026-09-06.
    ///
    /// If a change here is deliberate, the firmware side moves with it. If it
    /// is not, this test is the alarm: a digest change means every committed
    /// f32 vector and every device trace just went stale, which the
    /// tolerance-based tests can miss. `-ffp-contract=fast` on the C side, for
    /// instance, changes these digests on three of four fixtures while still
    /// landing inside the 5.8e-6 noise floor.
    #[test]
    fn firmware_trace_digests_are_pinned() {
        let expected: [(&str, u64); 4] = [
            ("01-double-integrator", 0xe4b8_b805_7816_2eaf),
            ("02-feedback-TF", 0xf6a4_3fbf_fe09_b100),
            ("03-TF-test", 0xf2ef_1769_744e_1a56),
            ("04-2nd-order-system", 0xfddb_22c1_a952_5b2c),
        ];

        for (name, digest) in expected {
            let dag = fixture(name);
            let plan = build_control_plan(&dag).expect("plan must build");
            let simulation = &dag.metadata.simulation;
            let steps = (simulation.end_time / simulation.step_size).floor() as usize + 1;
            let trace = run(&plan, steps).expect("execution must succeed");

            assert_eq!(
                trace_digest(&trace),
                digest,
                "{name}: trace digest changed - firmware traces and committed \
                 f32 vectors are now stale"
            );
        }
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

    /// The smallest project that exercises exactly one transfer function:
    /// `constant(1) -> transferFunction -> scope`. A step into the filter, which
    /// is the excitation that drags its state through the whole transient.
    fn synthetic_tf_project(numerator: &str, denominator: &str, end_time: f64) -> String {
        format!(
            r#"{{
  "version": 1,
  "kind": "ctrl-lab-project",
  "generatedAt": "2026-09-06T00:00:00.000Z",
  "title": "order-sweep",
  "simulation": {{ "endTime": {end_time}, "stepSize": 0.01 }},
  "nodes": [
    {{ "id": "constant-1", "type": "constant", "label": "Constant", "role": "const-01",
       "position": {{ "x": 0, "y": 0 }},
       "properties": {{ "value": "1.0", "dataType": "f32" }} }},
    {{ "id": "transferFunction-2", "type": "transferFunction", "label": "Transfer Function",
       "role": "tf-01", "position": {{ "x": 144, "y": 0 }},
       "properties": {{ "numerator": "{numerator}", "denominator": "{denominator}",
                        "domain": "discrete", "discreteVariable": "z",
                        "stateName": "x", "dataType": "f32" }} }},
    {{ "id": "scope-3", "type": "scope", "label": "Scope", "role": "scope-01",
       "position": {{ "x": 288, "y": 0 }},
       "properties": {{ "channel": "CH-1", "timebase": "1 s/div", "dataType": "f32" }} }}
  ],
  "edges": [
    {{ "id": "edge-constant-to-tf", "sourceNodeId": "constant-1", "sourcePortId": "out",
       "targetNodeId": "transferFunction-2", "targetPortId": "in" }},
    {{ "id": "edge-tf-to-scope", "sourceNodeId": "transferFunction-2", "sourcePortId": "out",
       "targetNodeId": "scope-3", "targetPortId": "in" }}
  ],
  "graphIndex": {{
    "nodesById": {{
      "constant-1": {{ "type": "constant", "role": "const-01",
                       "inputPortIds": [], "outputPortIds": ["out"] }},
      "transferFunction-2": {{ "type": "transferFunction", "role": "tf-01",
                               "inputPortIds": ["in"], "outputPortIds": ["out"] }},
      "scope-3": {{ "type": "scope", "role": "scope-01",
                    "inputPortIds": ["in"], "outputPortIds": [] }}
    }},
    "incomingEdgesByNodeId": {{
      "constant-1": [],
      "transferFunction-2": ["edge-constant-to-tf"],
      "scope-3": ["edge-tf-to-scope"]
    }},
    "outgoingEdgesByNodeId": {{
      "constant-1": ["edge-constant-to-tf"],
      "transferFunction-2": ["edge-tf-to-scope"],
      "scope-3": []
    }}
  }}
}}"#
        )
    }

    /// Expands `(z - p0)(z - p1)...` into coefficients, highest power first,
    /// which is the convention the project's denominators use.
    ///
    /// The packed state space stores this *expanded* polynomial, which is the
    /// whole point: a second-order-section cascade would keep the roots apart
    /// instead. Expanding is where the precision goes.
    fn polynomial_from_poles(poles: &[f64]) -> Vec<f64> {
        let mut coefficients = vec![1.0];
        for pole in poles {
            let mut next = vec![0.0; coefficients.len() + 1];
            for (index, coefficient) in coefficients.iter().enumerate() {
                next[index] += *coefficient;
                next[index + 1] -= pole * *coefficient;
            }
            coefficients = next;
        }
        coefficients
    }

    fn join(coefficients: &[f64]) -> String {
        coefficients
            .iter()
            .map(|value| format!("{value}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// How far the f32 executor drifts from the f64 simulator for a filter with
    /// the given poles. Returns `None` if the project is rejected before it can
    /// run.
    fn divergence_for_poles(poles: &[f64]) -> Option<f64> {
        let denominator = polynomial_from_poles(poles);

        // A bare constant numerator chosen so the DC gain is exactly 1 at every
        // order, which keeps the output comparable as the order changes.
        let mut numerator = vec![0.0; poles.len()];
        numerator.push(poles.iter().map(|pole| 1.0 - pole).product());

        let json = synthetic_tf_project(&join(&numerator), &join(&denominator), 4.0);
        let dag = parse_project_json(&json).ok()?;
        let plan = build_control_plan(&dag).ok()?;
        let reference = simulate_validated_dag(&dag).ok()?;
        let trace = run(&plan, reference.times.len()).ok()?;

        let mut worst = 0.0f64;
        for (slot, node_id) in dag.topological_order.iter().enumerate() {
            let expected = &reference.values_by_node_id[node_id];
            for (k, value) in trace.signals[slot].iter().enumerate() {
                worst = worst.max((expected[k] - *value as f64).abs());
            }
        }
        Some(worst)
    }

    /// Finds where a single packed state space stops tracking f64 in f32.
    ///
    /// This exists because `MAX_TF_ORDER` was picked without evidence. The
    /// firmware design doc originally called for a biquad second-order-section
    /// cascade precisely to avoid high-order f32 fragility; the packed state
    /// space is the cheaper choice and is what `plan.rs` emits, so the honest
    /// way to keep it is to know where it stops being trustworthy and refuse to
    /// go past that, rather than to assume it is fine everywhere.
    /// Pins `MAX_TF_ORDER`, from both sides.
    ///
    /// Below the cap, the packed state space must actually track f64 — measured,
    /// not asserted from theory. Above it, the model must be refused rather than
    /// run, because past order 2 a clustered-pole filter in f32 does not merely
    /// lose precision, it diverges. The full sweep behind the number is recorded
    /// on `MAX_TF_ORDER`; regenerating it means raising the cap deliberately.
    #[test]
    fn transfer_function_order_is_capped_at_two() {
        // The harshest shape the cap has to survive: repeated poles close to the
        // unit circle, where expanding the polynomial loses the most precision.
        for pole in [0.5f64, 0.9, 0.95] {
            for order in 1..=MAX_TF_ORDER {
                let worst = divergence_for_poles(&vec![pole; order]).unwrap_or_else(|| {
                    panic!("order {order} is within MAX_TF_ORDER and must still run")
                });

                eprintln!("repeated pole {pole}, order {order}: max |f64 - f32| = {worst:.3e}");
                assert!(
                    worst < 1.0e-5,
                    "a repeated pole at z = {pole} at order {order} diverged by \
                     {worst:.3e}, which is past the f32 noise floor - the packed \
                     state space is no longer trustworthy at this order, so \
                     MAX_TF_ORDER is too high"
                );
            }

            assert!(
                divergence_for_poles(&vec![pole; MAX_TF_ORDER + 1]).is_none(),
                "order {} must be refused, not executed: in f32 this shape is \
                 already far past the noise floor and by order 6 it diverges \
                 outright. Silently running it is the failure this cap prevents",
                MAX_TF_ORDER + 1
            );
        }
    }

}
