//! Deployable Control Plan (DCP) — the backend→firmware contract.
//!
//! The simulation engine works from a [`ValidatedDag`]. The firmware runtime,
//! by contrast, executes a flat, pre-scheduled *plan*: an ordered list of block
//! kernels over a signal pool, with all parameters packed and all state offsets
//! resolved. This module builds that plan from a validated project and encodes
//! it to a deterministic little-endian byte stream.
//!
//! See `firmware/AGENTS.md` for the runtime model this serves and
//! `backend/AGENTS.md` for the numerical contract the packed coefficients obey.

use std::collections::HashMap;

use crate::{NodeId, SerializedNode, SimulationError, ValidatedDag};

/// Container format version. Bump on any change to the byte layout below.
pub const DCP_FORMAT_VERSION: u16 = 1;
/// Kernel-library version. Bump whenever a `KernelId` is added or its parameter
/// or state layout changes. A firmware advertising an older set must reject a
/// plan it cannot fully execute.
pub const KERNEL_SET_VERSION: u16 = 1;

const DCP_MAGIC: [u8; 4] = *b"DCP1";
/// Fixed header size: 4-byte magic + 48 bytes of fields.
const DCP_HEADER_LEN: usize = 52;

/// Stable identifiers for the block kernels the firmware runtime provides.
/// Values are wire-stable: never renumber, only append.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum KernelId {
    Constant = 1,
    Step = 2,
    SquareWave = 3,
    Gain = 4,
    Sum = 5,
    Switch = 6,
    Delay = 7,
    Integrator = 8,
    /// Discrete state space `x[k+1] = Ad x + Bd u`, `y = C x + D u`. Continuous
    /// transfer functions are ZOH-discretized before packing; native discrete
    /// ones are realized in controllable canonical form.
    TransferFunction = 9,
    /// Sink / unity passthrough (scope, display) — carries a signal for telemetry.
    Scope = 10,
}

impl KernelId {
    pub fn to_u16(self) -> u16 {
        self as u16
    }

    pub fn from_u16(value: u16) -> Option<Self> {
        Some(match value {
            1 => Self::Constant,
            2 => Self::Step,
            3 => Self::SquareWave,
            4 => Self::Gain,
            5 => Self::Sum,
            6 => Self::Switch,
            7 => Self::Delay,
            8 => Self::Integrator,
            9 => Self::TransferFunction,
            10 => Self::Scope,
            _ => return None,
        })
    }

    /// The kernel this project node type maps to.
    fn for_node_type(node_type: &str) -> Option<Self> {
        Some(match node_type {
            "constant" => Self::Constant,
            "step" => Self::Step,
            "squareWave" => Self::SquareWave,
            "gain" => Self::Gain,
            "sum" => Self::Sum,
            "switch" => Self::Switch,
            "delay" => Self::Delay,
            "integrator" => Self::Integrator,
            "transferFunction" => Self::TransferFunction,
            "scope" | "display" => Self::Scope,
            _ => return None,
        })
    }

    /// Input ports in the order the kernel gathers them. Sources have none.
    fn input_ports(self) -> &'static [&'static str] {
        match self {
            Self::Constant | Self::Step | Self::SquareWave => &[],
            Self::Gain | Self::Integrator | Self::Delay | Self::Scope => &["in"],
            Self::Sum => &["a", "b"],
            Self::Switch => &["a", "b", "sel"],
            Self::TransferFunction => &["in"],
        }
    }
}

/// A single scheduled block: which kernel, where its parameters and state live,
/// which signals it reads, and which signal it writes.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockRecord {
    pub kernel_id: KernelId,
    /// Executes when `tick % rate_div == 0`. Always 1 in v1 (single rate).
    pub rate_div: u16,
    pub param_offset: u32,
    pub param_len: u16,
    pub state_offset: u32,
    pub state_len: u16,
    pub output_signal: u32,
    /// Input signal indices, in `KernelId::input_ports` order.
    pub inputs: Vec<u32>,
}

/// Peripheral binding for a source/sink block, resolved by the HAL at load time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IoBinding {
    pub block_index: u32,
    pub channel_role: u16,
    pub channel_index: u16,
}

/// Non-executable provenance carried alongside the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanMeta {
    pub model_name: String,
    pub generated_at: String,
    pub backend_version: String,
}

/// A fully resolved control plan, ready to encode and ship.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlPlan {
    pub format_version: u16,
    pub kernel_set_version: u16,
    /// Scheduler tick period in nanoseconds (the global `Ts`).
    pub base_ts_ns: u64,
    /// Number of signal slots (one per block output).
    pub signal_count: u32,
    /// Persistent state pool size, in f32 units.
    pub state_len: u32,
    /// Worst-case execution estimate; 0 until the kernels are profiled.
    pub wcet_estimate_ns: u64,
    pub blocks: Vec<BlockRecord>,
    pub params: Vec<f32>,
    pub io_bindings: Vec<IoBinding>,
    pub meta: PlanMeta,
}

/// Builds a [`ControlPlan`] from a validated project. Signal slots follow the
/// DAG's topological order, so the firmware executes `blocks` top to bottom with
/// no runtime scheduling.
pub fn build_control_plan(dag: &ValidatedDag) -> Result<ControlPlan, SimulationError> {
    let step_size = dag.metadata.simulation.step_size;
    if !step_size.is_finite() || step_size <= 0.0 {
        return Err(SimulationError::InvalidStepSize {
            step_size: step_size.to_string(),
        });
    }

    // One signal slot per node, indexed by position in topological order.
    let slot_of: HashMap<&NodeId, u32> = dag
        .topological_order
        .iter()
        .enumerate()
        .map(|(index, node_id)| (node_id, index as u32))
        .collect();

    // Source node for each (target node, target port).
    let source_of: HashMap<(&NodeId, &str), &NodeId> = dag
        .edges
        .iter()
        .map(|edge| {
            (
                (&edge.target_node_id, edge.target_port_id.as_str()),
                &edge.source_node_id,
            )
        })
        .collect();

    let mut params: Vec<f32> = Vec::new();
    let mut state_len: u32 = 0;
    let mut blocks: Vec<BlockRecord> = Vec::with_capacity(dag.topological_order.len());

    for (index, node_id) in dag.topological_order.iter().enumerate() {
        let node = dag
            .nodes
            .get(node_id)
            .expect("validated dag must contain raw nodes");
        let kernel = KernelId::for_node_type(&node.node_type).ok_or_else(|| {
            SimulationError::UnsupportedNodeType {
                node_id: node.id.clone(),
                node_type: node.node_type.clone(),
            }
        })?;

        let mut inputs = Vec::with_capacity(kernel.input_ports().len());
        for port in kernel.input_ports() {
            let source = source_of.get(&(node_id, *port)).ok_or_else(|| {
                SimulationError::MissingInputEdge {
                    node_id: node.id.clone(),
                    port_id: (*port).to_string(),
                }
            })?;
            inputs.push(slot_of[source]);
        }

        let (block_params, block_state_len) = pack_params(kernel, node, step_size)?;
        let param_offset = params.len() as u32;
        params.extend(block_params);
        let param_len = (params.len() as u32 - param_offset) as u16;

        let state_offset = state_len;
        state_len += block_state_len;

        blocks.push(BlockRecord {
            kernel_id: kernel,
            rate_div: 1,
            param_offset,
            param_len,
            state_offset,
            state_len: block_state_len as u16,
            output_signal: index as u32,
            inputs,
        });
    }

    Ok(ControlPlan {
        format_version: DCP_FORMAT_VERSION,
        kernel_set_version: KERNEL_SET_VERSION,
        base_ts_ns: (step_size * 1e9).round() as u64,
        signal_count: dag.topological_order.len() as u32,
        state_len,
        wcet_estimate_ns: 0,
        blocks,
        params,
        io_bindings: Vec::new(),
        meta: PlanMeta {
            model_name: dag.metadata.title.clone(),
            generated_at: dag.metadata.generated_at.clone(),
            backend_version: env!("CARGO_PKG_VERSION").to_string(),
        },
    })
}

/// Packs a block's parameters and returns them with the block's state length
/// (in f32 units).
fn pack_params(
    kernel: KernelId,
    node: &SerializedNode,
    step_size: f64,
) -> Result<(Vec<f32>, u32), SimulationError> {
    let numeric = |property: &str, fallback: f64| -> Result<f32, SimulationError> {
        crate::parse_numeric_property(node, property, fallback)
            .map(|value| value as f32)
            .map_err(|value| SimulationError::InvalidNumericProperty {
                node_id: node.id.clone(),
                property: property.to_string(),
                value,
            })
    };

    Ok(match kernel {
        KernelId::Constant => (vec![numeric("value", 0.0)?], 0),
        KernelId::Step => (
            vec![
                numeric("initialValue", 0.0)?,
                numeric("finalValue", 1.0)?,
                numeric("stepTime", 0.0)?,
            ],
            0,
        ),
        KernelId::SquareWave => (
            vec![
                numeric("amplitude", 1.0)?,
                numeric("frequency", 1.0)?,
                numeric("duty", 50.0)?,
            ],
            0,
        ),
        KernelId::Gain => (vec![numeric("gain", 1.0)?], 0),
        KernelId::Sum => {
            let (left, right) =
                crate::parse_equation_tokens(node.properties.get("equation").map(String::as_str));
            (vec![operator_code(left), operator_code(right)], 0)
        }
        KernelId::Switch => (Vec::new(), 0),
        KernelId::Delay => {
            let state = crate::initialize_delay_state(node, step_size)?;
            (
                vec![state.initial_value as f32, state.delay_steps as f32],
                state.delay_steps as u32,
            )
        }
        KernelId::Integrator => (vec![numeric("initialValue", 0.0)?], 1),
        KernelId::TransferFunction => {
            let (order, ad, bd, c, d) = transfer_function_state_space(node, step_size)?;
            let mut packed = Vec::with_capacity(1 + order * order + order + order + 1);
            packed.push(order as f32);
            for row in &ad {
                packed.extend(row.iter().map(|value| *value as f32));
            }
            packed.extend(bd.iter().map(|value| *value as f32));
            packed.extend(c.iter().map(|value| *value as f32));
            packed.push(d as f32);
            (packed, order as u32)
        }
        KernelId::Scope => (Vec::new(), 0),
    })
}

fn operator_code(operator: char) -> f32 {
    match operator {
        '+' => 0.0,
        '-' => 1.0,
        '*' => 2.0,
        '/' => 3.0,
        _ => 0.0,
    }
}

/// Returns the discrete state space `(order, Ad, Bd, C, D)` the firmware TF
/// kernel executes. Continuous specs are ZOH-discretized; native discrete specs
/// are realized in controllable canonical form.
fn transfer_function_state_space(
    node: &SerializedNode,
    step_size: f64,
) -> Result<(usize, Vec<Vec<f64>>, Vec<f64>, Vec<f64>, f64), SimulationError> {
    let spec = crate::parse_transfer_function(node)?;
    match crate::TransferFunctionModel::from_spec(&spec, step_size) {
        crate::TransferFunctionModel::ContinuousZoh(ss) => {
            Ok((ss.c.len(), ss.ad, ss.bd, ss.c, ss.d))
        }
        crate::TransferFunctionModel::Discrete(model) => Ok(companion_state_space(
            &model.normalized_numerator,
            &model.normalized_denominator,
        )),
    }
}

/// Controllable canonical discrete state space for a `z^-1` difference equation
/// (numerator/denominator in ascending powers of `z^-1`, denominator's leading
/// coefficient used for normalization).
fn companion_state_space(
    numerator: &[f64],
    denominator: &[f64],
) -> (usize, Vec<Vec<f64>>, Vec<f64>, Vec<f64>, f64) {
    let leading = denominator[0];
    let order = denominator.len() - 1;

    // Monic denominator and numerator right-padded to length order + 1 (missing
    // deeper-delay terms are zero), both normalized by the leading coefficient.
    let a: Vec<f64> = denominator.iter().map(|value| value / leading).collect();
    let b: Vec<f64> = (0..=order)
        .map(|i| numerator.get(i).copied().unwrap_or(0.0) / leading)
        .collect();

    let d = b[0];
    let beta: Vec<f64> = (1..=order).map(|i| b[i] - d * a[i]).collect();

    let mut ad = vec![vec![0.0; order]; order];
    for row in 0..order.saturating_sub(1) {
        ad[row][row + 1] = 1.0;
    }
    for column in 0..order {
        ad[order - 1][column] = -a[order - column];
    }

    let mut bd = vec![0.0; order];
    bd[order - 1] = 1.0;

    let c: Vec<f64> = (0..order).map(|k| beta[order - 1 - k]).collect();

    (order, ad, bd, c, d)
}

// ---------------------------------------------------------------------------
// Binary codec
// ---------------------------------------------------------------------------

/// Reason a byte stream failed to decode as a control plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanDecodeError {
    TooShort,
    BadMagic,
    UnknownKernel(u16),
    ChecksumMismatch,
    PlanIdMismatch,
    Malformed(&'static str),
}

/// Encodes a plan to its little-endian byte representation.
pub fn encode(plan: &ControlPlan) -> Vec<u8> {
    let body = encode_body(plan);
    let plan_id = fnv1a64(&body);
    let crc = crc32(&body);

    let mut out = Vec::with_capacity(DCP_HEADER_LEN + body.len());
    out.extend_from_slice(&DCP_MAGIC);
    put_u16(&mut out, plan.format_version);
    put_u16(&mut out, plan.kernel_set_version);
    put_u64(&mut out, plan_id);
    put_u64(&mut out, plan.base_ts_ns);
    put_u32(&mut out, plan.blocks.len() as u32);
    put_u32(&mut out, plan.signal_count);
    put_u32(&mut out, plan.signal_count * 4); // signal_pool_bytes
    put_u32(&mut out, plan.state_len * 4); // state_pool_bytes
    put_u64(&mut out, plan.wcet_estimate_ns);
    put_u32(&mut out, crc);
    out.extend_from_slice(&body);
    out
}

fn encode_body(plan: &ControlPlan) -> Vec<u8> {
    let mut body = Vec::new();

    for block in &plan.blocks {
        put_u16(&mut body, block.kernel_id.to_u16());
        put_u16(&mut body, block.rate_div);
        put_u32(&mut body, block.param_offset);
        put_u16(&mut body, block.param_len);
        put_u32(&mut body, block.state_offset);
        put_u16(&mut body, block.state_len);
        put_u32(&mut body, block.output_signal);
        put_u16(&mut body, block.inputs.len() as u16);
        for input in &block.inputs {
            put_u32(&mut body, *input);
        }
    }

    put_u32(&mut body, plan.params.len() as u32);
    for value in &plan.params {
        body.extend_from_slice(&value.to_le_bytes());
    }

    put_u32(&mut body, plan.io_bindings.len() as u32);
    for binding in &plan.io_bindings {
        put_u32(&mut body, binding.block_index);
        put_u16(&mut body, binding.channel_role);
        put_u16(&mut body, binding.channel_index);
    }

    put_str(&mut body, &plan.meta.model_name);
    put_str(&mut body, &plan.meta.generated_at);
    put_str(&mut body, &plan.meta.backend_version);

    body
}

/// Decodes a plan and verifies its checksum and plan id.
pub fn decode(bytes: &[u8]) -> Result<ControlPlan, PlanDecodeError> {
    if bytes.len() < DCP_HEADER_LEN {
        return Err(PlanDecodeError::TooShort);
    }
    if bytes[0..4] != DCP_MAGIC {
        return Err(PlanDecodeError::BadMagic);
    }

    let mut header = Cursor::new(&bytes[4..DCP_HEADER_LEN]);
    let format_version = header.u16()?;
    let kernel_set_version = header.u16()?;
    let plan_id = header.u64()?;
    let base_ts_ns = header.u64()?;
    let n_blocks = header.u32()?;
    let signal_count = header.u32()?;
    let _signal_pool_bytes = header.u32()?;
    let state_pool_bytes = header.u32()?;
    let wcet_estimate_ns = header.u64()?;
    let crc = header.u32()?;

    let body = &bytes[DCP_HEADER_LEN..];
    if crc32(body) != crc {
        return Err(PlanDecodeError::ChecksumMismatch);
    }
    if fnv1a64(body) != plan_id {
        return Err(PlanDecodeError::PlanIdMismatch);
    }

    let mut cursor = Cursor::new(body);
    let mut blocks = Vec::with_capacity(n_blocks as usize);
    for _ in 0..n_blocks {
        let kernel_raw = cursor.u16()?;
        let kernel_id =
            KernelId::from_u16(kernel_raw).ok_or(PlanDecodeError::UnknownKernel(kernel_raw))?;
        let rate_div = cursor.u16()?;
        let param_offset = cursor.u32()?;
        let param_len = cursor.u16()?;
        let state_offset = cursor.u32()?;
        let state_len = cursor.u16()?;
        let output_signal = cursor.u32()?;
        let in_count = cursor.u16()?;
        let mut inputs = Vec::with_capacity(in_count as usize);
        for _ in 0..in_count {
            inputs.push(cursor.u32()?);
        }
        blocks.push(BlockRecord {
            kernel_id,
            rate_div,
            param_offset,
            param_len,
            state_offset,
            state_len,
            output_signal,
            inputs,
        });
    }

    let param_count = cursor.u32()?;
    let mut params = Vec::with_capacity(param_count as usize);
    for _ in 0..param_count {
        params.push(cursor.f32()?);
    }

    let io_count = cursor.u32()?;
    let mut io_bindings = Vec::with_capacity(io_count as usize);
    for _ in 0..io_count {
        io_bindings.push(IoBinding {
            block_index: cursor.u32()?,
            channel_role: cursor.u16()?,
            channel_index: cursor.u16()?,
        });
    }

    let meta = PlanMeta {
        model_name: cursor.string()?,
        generated_at: cursor.string()?,
        backend_version: cursor.string()?,
    };

    Ok(ControlPlan {
        format_version,
        kernel_set_version,
        base_ts_ns,
        signal_count,
        state_len: state_pool_bytes / 4,
        wcet_estimate_ns,
        blocks,
        params,
        io_bindings,
        meta,
    })
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_str(out: &mut Vec<u8>, value: &str) {
    put_u16(out, value.len() as u16);
    out.extend_from_slice(value.as_bytes());
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], PlanDecodeError> {
        let end = self
            .position
            .checked_add(n)
            .ok_or(PlanDecodeError::Malformed("length overflow"))?;
        if end > self.bytes.len() {
            return Err(PlanDecodeError::TooShort);
        }
        let slice = &self.bytes[self.position..end];
        self.position = end;
        Ok(slice)
    }

    fn u16(&mut self) -> Result<u16, PlanDecodeError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, PlanDecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, PlanDecodeError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, PlanDecodeError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<String, PlanDecodeError> {
        let len = self.u16()? as usize;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| PlanDecodeError::Malformed("invalid utf-8"))
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xffff_ffff;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_project_json;
    use std::path::PathBuf;

    fn fixture(name: &str) -> ValidatedDag {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("test-projects")
            .join(name);
        let json = std::fs::read_to_string(path).expect("fixture must be readable");
        parse_project_json(&json).expect("fixture must parse")
    }

    #[test]
    fn builds_plan_with_one_signal_per_node() {
        let dag = fixture("04-2nd-order-system.json");
        let plan = build_control_plan(&dag).expect("plan must build");

        assert_eq!(plan.signal_count as usize, dag.topological_order.len());
        assert_eq!(plan.blocks.len(), dag.topological_order.len());
        assert_eq!(plan.base_ts_ns, 50_000_000); // 0.05 s
        // Every output signal is written exactly once, in order.
        for (index, block) in plan.blocks.iter().enumerate() {
            assert_eq!(block.output_signal as usize, index);
        }
    }

    #[test]
    fn discrete_transfer_function_packs_second_order_state_space() {
        let dag = fixture("04-2nd-order-system.json");
        let plan = build_control_plan(&dag).expect("plan must build");

        let tf = plan
            .blocks
            .iter()
            .find(|block| block.kernel_id == KernelId::TransferFunction)
            .expect("a transfer-function block must exist");

        // Layout: [order, Ad(order^2), Bd(order), C(order), D] with order = 2.
        assert_eq!(tf.state_len, 2);
        assert_eq!(tf.param_len, 1 + 4 + 2 + 2 + 1);
        let base = tf.param_offset as usize;
        assert_eq!(plan.params[base], 2.0); // order
    }

    #[test]
    fn continuous_transfer_function_is_zoh_discretized() {
        let dag = fixture("02-feedback-TF.json");
        let plan = build_control_plan(&dag).expect("plan must build");
        let tf = plan
            .blocks
            .iter()
            .find(|block| block.kernel_id == KernelId::TransferFunction)
            .expect("a transfer-function block must exist");
        let base = tf.param_offset as usize;

        // 1/(s+1) at Ts=0.1 -> Ad = e^-0.1 = 0.904837, order 1.
        assert_eq!(plan.params[base], 1.0);
        let ad = plan.params[base + 1];
        assert!((ad - 0.904_837_4).abs() < 1e-5, "Ad was {ad}");
    }

    #[test]
    fn round_trips_every_fixture() {
        for name in [
            "01-double-integrator.json",
            "02-feedback-TF.json",
            "03-TF-test.json",
            "04-2nd-order-system.json",
        ] {
            let dag = fixture(name);
            let plan = build_control_plan(&dag).expect("plan must build");
            let bytes = encode(&plan);
            let decoded = decode(&bytes).expect("plan must decode");
            assert_eq!(plan, decoded, "round trip mismatch for {name}");
        }
    }

    #[test]
    fn rejects_corrupted_stream() {
        let dag = fixture("01-double-integrator.json");
        let mut bytes = encode(&build_control_plan(&dag).expect("plan must build"));
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        assert_eq!(decode(&bytes), Err(PlanDecodeError::ChecksumMismatch));
    }
}
