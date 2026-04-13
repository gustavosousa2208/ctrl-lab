use std::collections::{BTreeSet, HashMap, VecDeque};
use std::fmt;

use serde::{Deserialize, Serialize};

pub type NodeId = String;
pub type EdgeId = String;
pub type PortId = String;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDocument {
    pub version: u32,
    pub kind: String,
    pub generated_at: String,
    pub title: String,
    pub simulation: SimulationConfig,
    pub nodes: Vec<SerializedNode>,
    pub edges: Vec<SerializedEdge>,
    pub graph_index: Option<ProjectGraphIndex>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SimulationConfig {
    pub end_time: f64,
    pub step_size: f64,
    pub zoom_step: Option<f64>,
    pub zoom: Option<f64>,
    pub viewport_x: Option<f64>,
    pub viewport_y: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SerializedNode {
    pub id: NodeId,
    #[serde(rename = "type")]
    pub node_type: String,
    pub label: String,
    pub role: String,
    pub position: Position,
    pub properties: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SerializedEdge {
    pub id: EdgeId,
    pub source_node_id: NodeId,
    pub source_port_id: PortId,
    pub target_node_id: NodeId,
    pub target_port_id: PortId,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGraphIndex {
    pub nodes_by_id: HashMap<NodeId, IndexedNode>,
    pub incoming_edges_by_node_id: HashMap<NodeId, Vec<EdgeId>>,
    pub outgoing_edges_by_node_id: HashMap<NodeId, Vec<EdgeId>>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IndexedNode {
    #[serde(rename = "type")]
    pub node_type: String,
    pub role: String,
    pub input_port_ids: Vec<PortId>,
    pub output_port_ids: Vec<PortId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedDag {
    pub metadata: ProjectMetadata,
    pub nodes: HashMap<NodeId, SerializedNode>,
    pub nodes_by_id: HashMap<NodeId, IndexedNode>,
    pub block_behaviors: HashMap<NodeId, BlockBehavior>,
    pub edges: Vec<SerializedEdge>,
    pub topological_order: Vec<NodeId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectMetadata {
    pub version: u32,
    pub kind: String,
    pub generated_at: String,
    pub title: String,
    pub simulation: SimulationConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SimulationOutput {
    pub times: Vec<f64>,
    pub values_by_node_id: HashMap<NodeId, Vec<f64>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockBehavior {
    pub is_stateful: bool,
    pub is_direct_feedthrough: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SimulationError {
    Parse {
        message: String,
    },
    InvalidStepSize {
        step_size: String,
    },
    InvalidEndTime {
        end_time: String,
    },
    InvalidNumericProperty {
        node_id: NodeId,
        property: String,
        value: String,
    },
    InvalidCoefficientList {
        node_id: NodeId,
        property: String,
        value: String,
    },
    MissingInputEdge {
        node_id: NodeId,
        port_id: PortId,
    },
    UnsupportedTransferFunctionShape {
        node_id: NodeId,
        numerator_len: usize,
        denominator_len: usize,
    },
    InvalidTransferFunctionDenominator {
        node_id: NodeId,
    },
    InvalidDelayTime {
        node_id: NodeId,
        delay_time: f64,
    },
    InvalidSquareWaveFrequency {
        node_id: NodeId,
        frequency: f64,
    },
    InvalidSquareWaveDuty {
        node_id: NodeId,
        duty: f64,
    },
    InvalidSwitchSelector {
        node_id: NodeId,
        value: f64,
    },
    UnsupportedNodeType {
        node_id: NodeId,
        node_type: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Json(String),
    MissingGraphIndex,
    DuplicateNodeId {
        node_id: NodeId,
    },
    DuplicateEdgeId {
        edge_id: EdgeId,
    },
    GraphIndexNodeMissing {
        node_id: NodeId,
    },
    GraphIndexExtraNode {
        node_id: NodeId,
    },
    GraphIndexIncomingNodeMissing {
        node_id: NodeId,
    },
    GraphIndexOutgoingNodeMissing {
        node_id: NodeId,
    },
    GraphIndexIncomingMismatch {
        node_id: NodeId,
        expected: Vec<EdgeId>,
        actual: Vec<EdgeId>,
    },
    GraphIndexOutgoingMismatch {
        node_id: NodeId,
        expected: Vec<EdgeId>,
        actual: Vec<EdgeId>,
    },
    SourceNodeMissing {
        edge_id: EdgeId,
        node_id: NodeId,
    },
    TargetNodeMissing {
        edge_id: EdgeId,
        node_id: NodeId,
    },
    InvalidSourcePort {
        edge_id: EdgeId,
        node_id: NodeId,
        port_id: PortId,
    },
    InvalidTargetPort {
        edge_id: EdgeId,
        node_id: NodeId,
        port_id: PortId,
    },
    UnconnectedRequiredInput {
        node_id: NodeId,
        port_id: PortId,
    },
    MultipleIncomingEdges {
        node_id: NodeId,
        port_id: PortId,
        edge_ids: Vec<EdgeId>,
    },
    AlgebraicLoopDetected,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(message) => write!(f, "failed to parse project JSON: {message}"),
            Self::MissingGraphIndex => write!(f, "project is missing required graphIndex"),
            Self::DuplicateNodeId { node_id } => write!(f, "duplicate node id `{node_id}`"),
            Self::DuplicateEdgeId { edge_id } => write!(f, "duplicate edge id `{edge_id}`"),
            Self::GraphIndexNodeMissing { node_id } => {
                write!(
                    f,
                    "serialized node `{node_id}` is missing from graphIndex.nodesById"
                )
            }
            Self::GraphIndexExtraNode { node_id } => {
                write!(f, "graphIndex.nodesById contains extra node `{node_id}`")
            }
            Self::GraphIndexIncomingNodeMissing { node_id } => {
                write!(
                    f,
                    "graphIndex.incomingEdgesByNodeId is missing node `{node_id}`"
                )
            }
            Self::GraphIndexOutgoingNodeMissing { node_id } => {
                write!(
                    f,
                    "graphIndex.outgoingEdgesByNodeId is missing node `{node_id}`"
                )
            }
            Self::GraphIndexIncomingMismatch {
                node_id,
                expected,
                actual,
            } => write!(
                f,
                "graphIndex incoming edge mismatch for `{node_id}`: expected {:?}, got {:?}",
                expected, actual
            ),
            Self::GraphIndexOutgoingMismatch {
                node_id,
                expected,
                actual,
            } => write!(
                f,
                "graphIndex outgoing edge mismatch for `{node_id}`: expected {:?}, got {:?}",
                expected, actual
            ),
            Self::SourceNodeMissing { edge_id, node_id } => {
                write!(
                    f,
                    "edge `{edge_id}` references missing source node `{node_id}`"
                )
            }
            Self::TargetNodeMissing { edge_id, node_id } => {
                write!(
                    f,
                    "edge `{edge_id}` references missing target node `{node_id}`"
                )
            }
            Self::InvalidSourcePort {
                edge_id,
                node_id,
                port_id,
            } => write!(
                f,
                "edge `{edge_id}` references invalid source port `{port_id}` on node `{node_id}`"
            ),
            Self::InvalidTargetPort {
                edge_id,
                node_id,
                port_id,
            } => write!(
                f,
                "edge `{edge_id}` references invalid target port `{port_id}` on node `{node_id}`"
            ),
            Self::UnconnectedRequiredInput { node_id, port_id } => write!(
                f,
                "required input port `{port_id}` on node `{node_id}` is unconnected"
            ),
            Self::MultipleIncomingEdges {
                node_id,
                port_id,
                edge_ids,
            } => write!(
                f,
                "input port `{port_id}` on node `{node_id}` has multiple incoming edges {:?}",
                edge_ids
            ),
            Self::AlgebraicLoopDetected => write!(f, "graph contains an algebraic loop"),
        }
    }
}

impl std::error::Error for ParseError {}

impl fmt::Display for SimulationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse { message } => write!(f, "{message}"),
            Self::InvalidStepSize { step_size } => {
                write!(
                    f,
                    "simulation step size must be positive, got `{step_size}`"
                )
            }
            Self::InvalidEndTime { end_time } => {
                write!(
                    f,
                    "simulation end time must be zero or positive, got `{end_time}`"
                )
            }
            Self::InvalidNumericProperty {
                node_id,
                property,
                value,
            } => write!(
                f,
                "node `{node_id}` has invalid numeric property `{property}` with value `{value}`"
            ),
            Self::InvalidCoefficientList {
                node_id,
                property,
                value,
            } => write!(
                f,
                "node `{node_id}` has invalid coefficient list `{property}` with value `{value}`"
            ),
            Self::MissingInputEdge { node_id, port_id } => {
                write!(
                    f,
                    "node `{node_id}` is missing a validated input edge for port `{port_id}`"
                )
            }
            Self::UnsupportedTransferFunctionShape {
                node_id,
                numerator_len,
                denominator_len,
            } => write!(
                f,
                "node `{node_id}` uses unsupported transfer function shape with {numerator_len} numerator coefficients and {denominator_len} denominator coefficients"
            ),
            Self::InvalidTransferFunctionDenominator { node_id } => {
                write!(
                    f,
                    "node `{node_id}` has an invalid transfer function denominator leading coefficient"
                )
            }
            Self::InvalidDelayTime { node_id, delay_time } => {
                write!(
                    f,
                    "node `{node_id}` has invalid delay time `{delay_time}`; expected -1 for global step size or a non-negative value"
                )
            }
            Self::InvalidSquareWaveFrequency { node_id, frequency } => {
                write!(
                    f,
                    "node `{node_id}` has invalid square wave frequency `{frequency}`; expected a positive value"
                )
            }
            Self::InvalidSquareWaveDuty { node_id, duty } => {
                write!(
                    f,
                    "node `{node_id}` has invalid square wave duty `{duty}`; expected a value between 0 and 100"
                )
            }
            Self::InvalidSwitchSelector { node_id, value } => {
                write!(
                    f,
                    "node `{node_id}` received invalid switch selector value `{value}`; expected 0 or 1"
                )
            }
            Self::UnsupportedNodeType { node_id, node_type } => {
                write!(
                    f,
                    "node `{node_id}` uses unsupported simulation block type `{node_type}`"
                )
            }
        }
    }
}

impl std::error::Error for SimulationError {}

pub fn parse_project_json(json: &str) -> Result<ValidatedDag, ParseError> {
    let document: ProjectDocument =
        serde_json::from_str(json).map_err(|error| ParseError::Json(error.to_string()))?;
    parse_project(document)
}

pub fn parse_project(document: ProjectDocument) -> Result<ValidatedDag, ParseError> {
    let graph_index = document
        .graph_index
        .clone()
        .ok_or(ParseError::MissingGraphIndex)?;

    let mut seen_node_ids = BTreeSet::new();
    let mut node_order = Vec::with_capacity(document.nodes.len());
    let mut raw_nodes_by_id = HashMap::with_capacity(document.nodes.len());
    for node in &document.nodes {
        if !seen_node_ids.insert(node.id.clone()) {
            return Err(ParseError::DuplicateNodeId {
                node_id: node.id.clone(),
            });
        }

        node_order.push(node.id.clone());
        raw_nodes_by_id.insert(node.id.clone(), node);
    }

    let mut seen_edge_ids = BTreeSet::new();
    for edge in &document.edges {
        if !seen_edge_ids.insert(edge.id.clone()) {
            return Err(ParseError::DuplicateEdgeId {
                edge_id: edge.id.clone(),
            });
        }
    }

    for node in &document.nodes {
        if !graph_index.nodes_by_id.contains_key(&node.id) {
            return Err(ParseError::GraphIndexNodeMissing {
                node_id: node.id.clone(),
            });
        }
    }

    for node_id in graph_index.nodes_by_id.keys() {
        if !raw_nodes_by_id.contains_key(node_id) {
            return Err(ParseError::GraphIndexExtraNode {
                node_id: node_id.clone(),
            });
        }
    }

    let mut actual_incoming: HashMap<NodeId, Vec<EdgeId>> = node_order
        .iter()
        .cloned()
        .map(|node_id| (node_id, Vec::new()))
        .collect();
    let mut actual_outgoing: HashMap<NodeId, Vec<EdgeId>> = node_order
        .iter()
        .cloned()
        .map(|node_id| (node_id, Vec::new()))
        .collect();
    let block_behaviors: HashMap<NodeId, BlockBehavior> = document
        .nodes
        .iter()
        .map(|node| (node.id.clone(), infer_block_behavior(node)))
        .collect();
    let mut incoming_edges_per_port: HashMap<(NodeId, PortId), Vec<EdgeId>> = HashMap::new();
    let mut dependency_adjacency: HashMap<NodeId, BTreeSet<NodeId>> = node_order
        .iter()
        .cloned()
        .map(|node_id| (node_id, BTreeSet::new()))
        .collect();
    let mut dependency_indegree: HashMap<NodeId, usize> = node_order
        .iter()
        .cloned()
        .map(|node_id| (node_id, 0usize))
        .collect();

    for edge in &document.edges {
        let source_index = graph_index
            .nodes_by_id
            .get(&edge.source_node_id)
            .ok_or_else(|| ParseError::SourceNodeMissing {
                edge_id: edge.id.clone(),
                node_id: edge.source_node_id.clone(),
            })?;
        let target_index = graph_index
            .nodes_by_id
            .get(&edge.target_node_id)
            .ok_or_else(|| ParseError::TargetNodeMissing {
                edge_id: edge.id.clone(),
                node_id: edge.target_node_id.clone(),
            })?;

        if !source_index.output_port_ids.contains(&edge.source_port_id) {
            return Err(ParseError::InvalidSourcePort {
                edge_id: edge.id.clone(),
                node_id: edge.source_node_id.clone(),
                port_id: edge.source_port_id.clone(),
            });
        }

        if !target_index.input_port_ids.contains(&edge.target_port_id) {
            return Err(ParseError::InvalidTargetPort {
                edge_id: edge.id.clone(),
                node_id: edge.target_node_id.clone(),
                port_id: edge.target_port_id.clone(),
            });
        }

        actual_incoming
            .get_mut(&edge.target_node_id)
            .expect("validated node ids must exist")
            .push(edge.id.clone());
        actual_outgoing
            .get_mut(&edge.source_node_id)
            .expect("validated node ids must exist")
            .push(edge.id.clone());
        incoming_edges_per_port
            .entry((edge.target_node_id.clone(), edge.target_port_id.clone()))
            .or_default()
            .push(edge.id.clone());
        if block_behaviors
            .get(&edge.target_node_id)
            .copied()
            .unwrap_or_else(|| block_behavior(&target_index.node_type))
            .is_direct_feedthrough
        {
            let inserted = dependency_adjacency
                .get_mut(&edge.source_node_id)
                .expect("validated node ids must exist")
                .insert(edge.target_node_id.clone());
            if inserted {
                *dependency_indegree
                    .get_mut(&edge.target_node_id)
                    .expect("validated node ids must exist") += 1;
            }
        }
    }

    for node_id in &node_order {
        let expected_incoming = graph_index
            .incoming_edges_by_node_id
            .get(node_id)
            .ok_or_else(|| ParseError::GraphIndexIncomingNodeMissing {
                node_id: node_id.clone(),
            })?;
        let actual = actual_incoming
            .get(node_id)
            .expect("validated node ids must exist");
        if expected_incoming != actual {
            return Err(ParseError::GraphIndexIncomingMismatch {
                node_id: node_id.clone(),
                expected: expected_incoming.clone(),
                actual: actual.clone(),
            });
        }

        let expected_outgoing = graph_index
            .outgoing_edges_by_node_id
            .get(node_id)
            .ok_or_else(|| ParseError::GraphIndexOutgoingNodeMissing {
                node_id: node_id.clone(),
            })?;
        let actual = actual_outgoing
            .get(node_id)
            .expect("validated node ids must exist");
        if expected_outgoing != actual {
            return Err(ParseError::GraphIndexOutgoingMismatch {
                node_id: node_id.clone(),
                expected: expected_outgoing.clone(),
                actual: actual.clone(),
            });
        }
    }

    for (node_id, indexed_node) in &graph_index.nodes_by_id {
        for input_port_id in &indexed_node.input_port_ids {
            match incoming_edges_per_port.get(&(node_id.clone(), input_port_id.clone())) {
                None => {
                    return Err(ParseError::UnconnectedRequiredInput {
                        node_id: node_id.clone(),
                        port_id: input_port_id.clone(),
                    })
                }
                Some(edge_ids) if edge_ids.len() > 1 => {
                    return Err(ParseError::MultipleIncomingEdges {
                        node_id: node_id.clone(),
                        port_id: input_port_id.clone(),
                        edge_ids: edge_ids.clone(),
                    })
                }
                Some(_) => {}
            }
        }
    }

    let order_index: HashMap<NodeId, usize> = node_order
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, node_id)| (node_id, index))
        .collect();

    let mut queue = Vec::new();
    for node_id in &node_order {
        if dependency_indegree
            .get(node_id)
            .copied()
            .unwrap_or_default()
            == 0
        {
            queue.push(node_id.clone());
        }
    }

    let mut topological_order = Vec::with_capacity(node_order.len());
    while !queue.is_empty() {
        let node_id = queue.remove(0);
        topological_order.push(node_id.clone());

        if let Some(targets) = dependency_adjacency.get(&node_id) {
            for target_id in targets {
                let count = dependency_indegree
                    .get_mut(target_id)
                    .expect("validated node ids must exist");
                *count -= 1;
                if *count == 0 {
                    queue.push(target_id.clone());
                }
            }
        }

        queue.sort_by_key(|queued_node_id| {
            order_index
                .get(queued_node_id)
                .copied()
                .expect("validated node ids must exist")
        });
    }

    if topological_order.len() != node_order.len() {
        return Err(ParseError::AlgebraicLoopDetected);
    }

    Ok(ValidatedDag {
        metadata: ProjectMetadata {
            version: document.version,
            kind: document.kind,
            generated_at: document.generated_at,
            title: document.title,
            simulation: document.simulation,
        },
        nodes: document
            .nodes
            .into_iter()
            .map(|node| (node.id.clone(), node))
            .collect(),
        nodes_by_id: graph_index.nodes_by_id,
        block_behaviors,
        edges: document.edges,
        topological_order,
    })
}

pub fn simulate_project_json(json: &str) -> Result<SimulationOutput, SimulationError> {
    let dag = parse_project_json(json).map_err(|error| SimulationError::Parse {
        message: error.to_string(),
    })?;
    simulate_validated_dag(&dag)
}

pub fn simulate_validated_dag(dag: &ValidatedDag) -> Result<SimulationOutput, SimulationError> {
    let step_size = dag.metadata.simulation.step_size;
    let end_time = dag.metadata.simulation.end_time;
    if !step_size.is_finite() || step_size <= 0.0 {
        return Err(SimulationError::InvalidStepSize {
            step_size: step_size.to_string(),
        });
    }
    if !end_time.is_finite() || end_time < 0.0 {
        return Err(SimulationError::InvalidEndTime {
            end_time: end_time.to_string(),
        });
    }

    let steps = (end_time / step_size).floor() as usize;
    let times: Vec<f64> = (0..=steps).map(|index| index as f64 * step_size).collect();
    let incoming_edges_by_port = build_incoming_edges_by_port(&dag.edges);

    let mut values_by_node_id: HashMap<NodeId, Vec<f64>> = dag
        .topological_order
        .iter()
        .cloned()
        .map(|node_id| (node_id, Vec::with_capacity(times.len())))
        .collect();

    let mut integrator_state: HashMap<NodeId, f64> = HashMap::new();
    let mut delay_state: HashMap<NodeId, DelayState> = HashMap::new();
    let mut transfer_function_models: HashMap<NodeId, TransferFunctionModel> = HashMap::new();
    let mut transfer_function_state: HashMap<NodeId, TransferFunctionState> = HashMap::new();
    for node_id in &dag.topological_order {
        let node = dag
            .nodes
            .get(node_id)
            .expect("validated dag must contain raw nodes");
        if node.node_type == "integrator" {
            let initial_value =
                parse_numeric_property(node, "initialValue", 0.0).map_err(|value| {
                    SimulationError::InvalidNumericProperty {
                        node_id: node.id.clone(),
                        property: "initialValue".to_string(),
                        value,
                    }
                })?;
            integrator_state.insert(node.id.clone(), initial_value);
        }
        if node.node_type == "delay" {
            delay_state.insert(node.id.clone(), initialize_delay_state(node, step_size)?);
        }
        if node.node_type == "transferFunction" {
            let model = parse_transfer_function(node)?;
            transfer_function_models.insert(node.id.clone(), model.clone());
            transfer_function_state
                .insert(node.id.clone(), TransferFunctionState::from_model(&model));
        }
    }

    for step_index in 0..times.len() {
        let mut current_values: HashMap<NodeId, f64> =
            HashMap::with_capacity(dag.topological_order.len());

        for node_id in &dag.topological_order {
            let node = dag
                .nodes
                .get(node_id)
                .expect("validated dag must contain raw nodes");

            let value = match node.node_type.as_str() {
                "constant" => parse_numeric_property(node, "value", 0.0).map_err(|value| {
                    SimulationError::InvalidNumericProperty {
                        node_id: node.id.clone(),
                        property: "value".to_string(),
                        value,
                    }
                })?,
                "step" => evaluate_step(node, times[step_index]).map_err(|(property, value)| {
                    SimulationError::InvalidNumericProperty {
                        node_id: node.id.clone(),
                        property,
                        value,
                    }
                })?,
                "delay" => {
                    let state = delay_state
                        .get(node_id)
                        .expect("delay state must exist after initialization");
                    if state.delay_steps == 0 {
                        read_input_value(&current_values, &incoming_edges_by_port, node_id, "in")?
                    } else {
                        state
                            .buffered_values
                            .front()
                            .copied()
                            .unwrap_or(state.initial_value)
                    }
                }
                "gain" => {
                    let input =
                        read_input_value(&current_values, &incoming_edges_by_port, node_id, "in")?;
                    let gain = parse_numeric_property(node, "gain", 1.0).map_err(|value| {
                        SimulationError::InvalidNumericProperty {
                            node_id: node.id.clone(),
                            property: "gain".to_string(),
                            value,
                        }
                    })?;
                    input * gain
                }
                "integrator" => *integrator_state
                    .get(node_id)
                    .expect("integrator state must exist after initialization"),
                "transferFunction" => transfer_function_state
                    .get(node_id)
                    .map(|state| {
                        let model = transfer_function_models
                            .get(node_id)
                            .expect("transfer function model must exist");
                        let input = if model.output_requires_input() {
                            read_input_value(
                                &current_values,
                                &incoming_edges_by_port,
                                node_id,
                                "in",
                            )?
                        } else {
                            0.0
                        };
                        Ok(model.output(state, input))
                    })
                    .expect("transfer function state must exist after initialization")?,
                "switch" => {
                    let input_a =
                        read_input_value(&current_values, &incoming_edges_by_port, node_id, "a")?;
                    let input_b =
                        read_input_value(&current_values, &incoming_edges_by_port, node_id, "b")?;
                    let selector =
                        read_input_value(&current_values, &incoming_edges_by_port, node_id, "sel")?;
                    if selector.abs() <= f64::EPSILON {
                        input_a
                    } else if (selector - 1.0).abs() <= f64::EPSILON {
                        input_b
                    } else {
                        return Err(SimulationError::InvalidSwitchSelector {
                            node_id: node.id.clone(),
                            value: selector,
                        });
                    }
                }
                "squareWave" => evaluate_square_wave(node, times[step_index])?,
                "sum" => {
                    let input_a =
                        read_input_value(&current_values, &incoming_edges_by_port, node_id, "a")?;
                    let input_b =
                        read_input_value(&current_values, &incoming_edges_by_port, node_id, "b")?;
                    evaluate_equation(
                        node.properties.get("equation").map(String::as_str),
                        input_a,
                        input_b,
                    )
                }
                "scope" | "display" => {
                    read_input_value(&current_values, &incoming_edges_by_port, node_id, "in")?
                }
                unsupported => {
                    return Err(SimulationError::UnsupportedNodeType {
                        node_id: node.id.clone(),
                        node_type: unsupported.to_string(),
                    })
                }
            };

            current_values.insert(node_id.clone(), value);
            values_by_node_id
                .get_mut(node_id)
                .expect("node output trace must exist")
                .push(value);
        }

        if step_index + 1 < times.len() {
            let mut next_integrator_state = integrator_state.clone();
            let mut next_delay_state = delay_state.clone();
            let mut next_transfer_function_state = transfer_function_state.clone();

            for node_id in &dag.topological_order {
                let node = dag
                    .nodes
                    .get(node_id)
                    .expect("validated dag must contain raw nodes");
                if node.node_type == "integrator" {
                    let input =
                        read_input_value(&current_values, &incoming_edges_by_port, node_id, "in")?;
                    let state = next_integrator_state
                        .get_mut(node_id)
                        .expect("integrator state must exist");
                    *state += input * step_size;
                }
                if node.node_type == "delay" {
                    let input =
                        read_input_value(&current_values, &incoming_edges_by_port, node_id, "in")?;
                    let state = next_delay_state
                        .get_mut(node_id)
                        .expect("delay state must exist");
                    if state.delay_steps > 0 {
                        let _ = state.buffered_values.pop_front();
                        state.buffered_values.push_back(input);
                    }
                }
                if node.node_type == "transferFunction" {
                    let input =
                        read_input_value(&current_values, &incoming_edges_by_port, node_id, "in")?;
                    let current_state = transfer_function_state
                        .get(node_id)
                        .cloned()
                        .expect("transfer function state must exist");
                    let model = transfer_function_models
                        .get(node_id)
                        .expect("transfer function model must exist");
                    next_transfer_function_state.insert(
                        node_id.clone(),
                        model.next_state(&current_state, input, step_size),
                    );
                }
            }

            integrator_state = next_integrator_state;
            delay_state = next_delay_state;
            transfer_function_state = next_transfer_function_state;
        }
    }

    Ok(SimulationOutput {
        times,
        values_by_node_id,
    })
}

fn build_incoming_edges_by_port(
    edges: &[SerializedEdge],
) -> HashMap<(NodeId, PortId), SerializedEdge> {
    edges
        .iter()
        .cloned()
        .map(|edge| {
            (
                (edge.target_node_id.clone(), edge.target_port_id.clone()),
                edge,
            )
        })
        .collect()
}

fn read_input_value(
    current_values: &HashMap<NodeId, f64>,
    incoming_edges_by_port: &HashMap<(NodeId, PortId), SerializedEdge>,
    node_id: &str,
    port_id: &str,
) -> Result<f64, SimulationError> {
    let edge = incoming_edges_by_port
        .get(&(node_id.to_string(), port_id.to_string()))
        .ok_or_else(|| SimulationError::MissingInputEdge {
            node_id: node_id.to_string(),
            port_id: port_id.to_string(),
        })?;

    current_values
        .get(&edge.source_node_id)
        .copied()
        .ok_or_else(|| SimulationError::MissingInputEdge {
            node_id: node_id.to_string(),
            port_id: port_id.to_string(),
        })
}

fn block_behavior(node_type: &str) -> BlockBehavior {
    match node_type {
        "integrator" | "delay" => BlockBehavior {
            is_stateful: true,
            is_direct_feedthrough: false,
        },
        "constant" | "step" | "squareWave" => BlockBehavior {
            is_stateful: false,
            is_direct_feedthrough: false,
        },
        "gain" | "sum" | "switch" | "scope" | "display" => BlockBehavior {
            is_stateful: false,
            is_direct_feedthrough: true,
        },
        _ => BlockBehavior {
            is_stateful: false,
            is_direct_feedthrough: true,
        },
    }
}

fn infer_block_behavior(node: &SerializedNode) -> BlockBehavior {
    if node.node_type != "transferFunction" {
        return block_behavior(&node.node_type);
    }

    match inspect_transfer_function_orders(node) {
        Some((numerator_len, denominator_len)) => BlockBehavior {
            is_stateful: true,
            is_direct_feedthrough: numerator_len >= denominator_len,
        },
        None => BlockBehavior {
            is_stateful: true,
            is_direct_feedthrough: true,
        },
    }
}

fn inspect_transfer_function_orders(node: &SerializedNode) -> Option<(usize, usize)> {
    let numerator = parse_numeric_list_property_for_behavior(node.properties.get("numerator")?)?;
    let denominator =
        parse_numeric_list_property_for_behavior(node.properties.get("denominator")?)?;
    Some((numerator.len(), denominator.len()))
}

fn parse_numeric_list_property_for_behavior(raw_value: &str) -> Option<Vec<f64>> {
    let mut values = Vec::new();
    for token in raw_value
        .split(|character: char| character.is_whitespace() || character == ',')
        .filter(|token| !token.is_empty())
    {
        values.push(token.parse::<f64>().ok()?);
    }

    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn parse_numeric_property(
    node: &SerializedNode,
    property: &str,
    fallback: f64,
) -> Result<f64, String> {
    let Some(value) = node.properties.get(property) else {
        return Ok(fallback);
    };

    value.trim().parse::<f64>().map_err(|_| value.clone())
}

fn evaluate_step(node: &SerializedNode, time: f64) -> Result<f64, (String, String)> {
    let initial_value = parse_numeric_property(node, "initialValue", 0.0)
        .map_err(|value| ("initialValue".to_string(), value))?;
    let final_value = parse_numeric_property(node, "finalValue", 1.0)
        .map_err(|value| ("finalValue".to_string(), value))?;
    let step_time = parse_numeric_property(node, "stepTime", 0.0)
        .map_err(|value| ("stepTime".to_string(), value))?;

    if time < step_time {
        Ok(initial_value)
    } else {
        Ok(final_value)
    }
}

fn evaluate_square_wave(node: &SerializedNode, time: f64) -> Result<f64, SimulationError> {
    let amplitude = parse_numeric_property(node, "amplitude", 1.0).map_err(|value| {
        SimulationError::InvalidNumericProperty {
            node_id: node.id.clone(),
            property: "amplitude".to_string(),
            value,
        }
    })?;
    let frequency = parse_numeric_property(node, "frequency", 1.0).map_err(|value| {
        SimulationError::InvalidNumericProperty {
            node_id: node.id.clone(),
            property: "frequency".to_string(),
            value,
        }
    })?;
    let duty = parse_numeric_property(node, "duty", 50.0).map_err(|value| {
        SimulationError::InvalidNumericProperty {
            node_id: node.id.clone(),
            property: "duty".to_string(),
            value,
        }
    })?;

    if !frequency.is_finite() || frequency <= 0.0 {
        return Err(SimulationError::InvalidSquareWaveFrequency {
            node_id: node.id.clone(),
            frequency,
        });
    }

    if !duty.is_finite() || !(0.0..=100.0).contains(&duty) {
        return Err(SimulationError::InvalidSquareWaveDuty {
            node_id: node.id.clone(),
            duty,
        });
    }

    if duty <= f64::EPSILON {
        return Ok(0.0);
    }
    if (100.0 - duty).abs() <= f64::EPSILON {
        return Ok(amplitude);
    }

    let period = 1.0 / frequency;
    let high_duration = period * (duty / 100.0);
    let phase_time = time.rem_euclid(period);

    if phase_time < high_duration {
        Ok(amplitude)
    } else {
        Ok(0.0)
    }
}

fn initialize_delay_state(
    node: &SerializedNode,
    step_size: f64,
) -> Result<DelayState, SimulationError> {
    let initial_value = parse_numeric_property(node, "initialValue", 0.0).map_err(|value| {
        SimulationError::InvalidNumericProperty {
            node_id: node.id.clone(),
            property: "initialValue".to_string(),
            value,
        }
    })?;
    let delay_time = parse_numeric_property(node, "delayTime", step_size).map_err(|value| {
        SimulationError::InvalidNumericProperty {
            node_id: node.id.clone(),
            property: "delayTime".to_string(),
            value,
        }
    })?;

    let resolved_delay_time = if (delay_time + 1.0).abs() <= f64::EPSILON {
        step_size
    } else {
        delay_time
    };

    if resolved_delay_time < 0.0 {
        return Err(SimulationError::InvalidDelayTime {
            node_id: node.id.clone(),
            delay_time,
        });
    }

    let delay_steps = if resolved_delay_time <= f64::EPSILON {
        0
    } else {
        (resolved_delay_time / step_size).round() as usize
    };

    Ok(DelayState {
        initial_value,
        delay_steps,
        buffered_values: std::iter::repeat_n(initial_value, delay_steps).collect(),
    })
}

#[derive(Debug, Clone)]
enum TransferFunctionModel {
    FirstOrder(FirstOrderTransferFunction),
    SecondOrder(SecondOrderTransferFunction),
}

impl TransferFunctionModel {
    fn output_requires_input(&self) -> bool {
        match self {
            Self::FirstOrder(model) => model.direct_gain.abs() > f64::EPSILON,
            Self::SecondOrder(_) => false,
        }
    }

    fn output(&self, state: &TransferFunctionState, input: f64) -> f64 {
        match (self, state) {
            (Self::FirstOrder(model), TransferFunctionState::FirstOrder { output }) => {
                model.direct_gain * input + *output
            }
            (
                Self::SecondOrder(model),
                TransferFunctionState::SecondOrder { position, velocity },
            ) => model.output_position_gain * *position + model.output_velocity_gain * *velocity,
            _ => unreachable!("transfer function model and state must match"),
        }
    }

    fn next_state(
        &self,
        current_state: &TransferFunctionState,
        input: f64,
        step_size: f64,
    ) -> TransferFunctionState {
        match (self, current_state) {
            (Self::FirstOrder(model), TransferFunctionState::FirstOrder { output }) => {
                TransferFunctionState::FirstOrder {
                    output: *output
                        + step_size
                            * ((model.dynamic_numerator_gain * input
                                - model.output_gain * *output)
                                / model.derivative_gain),
                }
            }
            (
                Self::SecondOrder(model),
                TransferFunctionState::SecondOrder { position, velocity },
            ) => {
                let position_dot = *velocity;
                let velocity_dot =
                    (-model.position_gain * *position - model.velocity_gain * *velocity + input)
                        / model.acceleration_gain;

                TransferFunctionState::SecondOrder {
                    position: *position + step_size * position_dot,
                    velocity: *velocity + step_size * velocity_dot,
                }
            }
            _ => unreachable!("transfer function model and state must match"),
        }
    }
}

#[derive(Debug, Clone)]
enum TransferFunctionState {
    FirstOrder { output: f64 },
    SecondOrder { position: f64, velocity: f64 },
}

impl TransferFunctionState {
    fn from_model(model: &TransferFunctionModel) -> Self {
        match model {
            TransferFunctionModel::FirstOrder(_) => Self::FirstOrder { output: 0.0 },
            TransferFunctionModel::SecondOrder(_) => Self::SecondOrder {
                position: 0.0,
                velocity: 0.0,
            },
        }
    }
}

#[derive(Debug, Clone)]
struct FirstOrderTransferFunction {
    derivative_gain: f64,
    output_gain: f64,
    dynamic_numerator_gain: f64,
    direct_gain: f64,
}

#[derive(Debug, Clone)]
struct SecondOrderTransferFunction {
    acceleration_gain: f64,
    velocity_gain: f64,
    position_gain: f64,
    output_position_gain: f64,
    output_velocity_gain: f64,
}

#[derive(Debug, Clone)]
struct DelayState {
    initial_value: f64,
    delay_steps: usize,
    buffered_values: VecDeque<f64>,
}

fn parse_transfer_function(
    node: &SerializedNode,
) -> Result<TransferFunctionModel, SimulationError> {
    let numerator = parse_numeric_list_property(node, "numerator")?;
    let denominator = parse_numeric_list_property(node, "denominator")?;

    match (numerator.len(), denominator.len()) {
        (1, 2) => {
            let derivative_gain = denominator[0];
            if !derivative_gain.is_finite() || derivative_gain.abs() <= f64::EPSILON {
                return Err(SimulationError::InvalidTransferFunctionDenominator {
                    node_id: node.id.clone(),
                });
            }

            Ok(TransferFunctionModel::FirstOrder(
                FirstOrderTransferFunction {
                    derivative_gain,
                    output_gain: denominator[1],
                    dynamic_numerator_gain: numerator[0],
                    direct_gain: 0.0,
                },
            ))
        }
        (2, 2) => {
            let derivative_gain = denominator[0];
            if !derivative_gain.is_finite() || derivative_gain.abs() <= f64::EPSILON {
                return Err(SimulationError::InvalidTransferFunctionDenominator {
                    node_id: node.id.clone(),
                });
            }

            let direct_gain = numerator[0] / derivative_gain;
            let dynamic_numerator_gain = numerator[1] - denominator[1] * direct_gain;

            Ok(TransferFunctionModel::FirstOrder(
                FirstOrderTransferFunction {
                    derivative_gain,
                    output_gain: denominator[1],
                    dynamic_numerator_gain,
                    direct_gain,
                },
            ))
        }
        (1 | 2, 3) => {
            let acceleration_gain = denominator[0];
            if !acceleration_gain.is_finite() || acceleration_gain.abs() <= f64::EPSILON {
                return Err(SimulationError::InvalidTransferFunctionDenominator {
                    node_id: node.id.clone(),
                });
            }

            let velocity_gain = denominator[1];
            let position_gain = denominator[2];
            let output_velocity_gain = if numerator.len() == 2 {
                numerator[0] / acceleration_gain
            } else {
                0.0
            };
            let output_position_gain = if numerator.len() == 2 {
                numerator[1] / acceleration_gain
            } else {
                numerator[0] / acceleration_gain
            };

            Ok(TransferFunctionModel::SecondOrder(
                SecondOrderTransferFunction {
                    acceleration_gain,
                    velocity_gain,
                    position_gain,
                    output_position_gain,
                    output_velocity_gain,
                },
            ))
        }
        _ => Err(SimulationError::UnsupportedTransferFunctionShape {
            node_id: node.id.clone(),
            numerator_len: numerator.len(),
            denominator_len: denominator.len(),
        }),
    }
}

fn parse_numeric_list_property(
    node: &SerializedNode,
    property: &str,
) -> Result<Vec<f64>, SimulationError> {
    let raw_value = node.properties.get(property).cloned().unwrap_or_default();

    let mut values = Vec::new();
    for token in raw_value
        .split(|character: char| character.is_whitespace() || character == ',')
        .filter(|token| !token.is_empty())
    {
        let parsed = token
            .parse::<f64>()
            .map_err(|_| SimulationError::InvalidCoefficientList {
                node_id: node.id.clone(),
                property: property.to_string(),
                value: raw_value.clone(),
            })?;
        values.push(parsed);
    }

    if values.is_empty() {
        return Err(SimulationError::InvalidCoefficientList {
            node_id: node.id.clone(),
            property: property.to_string(),
            value: raw_value,
        });
    }

    Ok(values)
}

fn evaluate_equation(equation: Option<&str>, a: f64, b: f64) -> f64 {
    let (left_operator, right_operator) = parse_equation_tokens(equation);

    match (left_operator, right_operator) {
        ('+', '+') => a + b,
        ('+', '-') => a - b,
        ('-', '+') => b - a,
        ('*', '*') => a * b,
        ('*', '/') => divide_safely(a, b),
        ('/', '*') => divide_safely(b, a),
        _ => a + b,
    }
}

fn parse_equation_tokens(equation: Option<&str>) -> (char, char) {
    let mut operators = equation
        .unwrap_or_default()
        .chars()
        .filter(|character| matches!(character, '+' | '-' | '*' | '/'));

    let left = operators.next().unwrap_or('+');
    let right = operators.next().unwrap_or('+');
    (left, right)
}

fn divide_safely(dividend: f64, divisor: f64) -> f64 {
    if divisor == 0.0 {
        0.0
    } else {
        dividend / divisor
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::path::PathBuf;

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("test-projects")
            .join("01-double-integrator.json")
    }

    fn second_order_system_fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("test-projects")
            .join("04-2nd-order-system.json")
    }

    fn fixture_json() -> String {
        std::fs::read_to_string(fixture_path()).expect("fixture must be readable")
    }

    fn second_order_system_fixture_json() -> String {
        std::fs::read_to_string(second_order_system_fixture_path()).expect("fixture must be readable")
    }

    fn fixture_value() -> Value {
        serde_json::from_str(&fixture_json()).expect("fixture must be valid JSON")
    }

    fn parse_value(value: Value) -> Result<ValidatedDag, ParseError> {
        parse_project_json(&serde_json::to_string(&value).expect("json must serialize"))
    }

    fn transfer_function_fixture_value() -> Value {
        json!({
            "version": 1,
            "kind": "ctrl-lab-project",
            "generatedAt": "2026-04-12T00:00:00.000Z",
            "title": "first-order-tf",
            "simulation": {
                "endTime": 1.0,
                "stepSize": 0.1
            },
            "nodes": [
                {
                    "id": "constant-1",
                    "type": "constant",
                    "label": "Constant",
                    "role": "const-01",
                    "position": { "x": 0, "y": 0 },
                    "properties": { "value": "1.0", "dataType": "f32" }
                },
                {
                    "id": "transferFunction-2",
                    "type": "transferFunction",
                    "label": "Transfer Function",
                    "role": "tf-01",
                    "position": { "x": 144, "y": 0 },
                    "properties": {
                        "numerator": "1.0",
                        "denominator": "1.0 1.0",
                        "stateName": "x",
                        "dataType": "f32"
                    }
                },
                {
                    "id": "scope-3",
                    "type": "scope",
                    "label": "Scope",
                    "role": "scope-01",
                    "position": { "x": 288, "y": 0 },
                    "properties": {
                        "channel": "CH-1",
                        "timebase": "1 s/div",
                        "dataType": "f32"
                    }
                }
            ],
            "edges": [
                {
                    "id": "edge-constant-to-tf",
                    "sourceNodeId": "constant-1",
                    "sourcePortId": "out",
                    "targetNodeId": "transferFunction-2",
                    "targetPortId": "in"
                },
                {
                    "id": "edge-tf-to-scope",
                    "sourceNodeId": "transferFunction-2",
                    "sourcePortId": "out",
                    "targetNodeId": "scope-3",
                    "targetPortId": "in"
                }
            ],
            "graphIndex": {
                "nodesById": {
                    "constant-1": {
                        "type": "constant",
                        "role": "const-01",
                        "inputPortIds": [],
                        "outputPortIds": ["out"]
                    },
                    "transferFunction-2": {
                        "type": "transferFunction",
                        "role": "tf-01",
                        "inputPortIds": ["in"],
                        "outputPortIds": ["out"]
                    },
                    "scope-3": {
                        "type": "scope",
                        "role": "scope-01",
                        "inputPortIds": ["in"],
                        "outputPortIds": []
                    }
                },
                "incomingEdgesByNodeId": {
                    "constant-1": [],
                    "transferFunction-2": ["edge-constant-to-tf"],
                    "scope-3": ["edge-tf-to-scope"]
                },
                "outgoingEdgesByNodeId": {
                    "constant-1": ["edge-constant-to-tf"],
                    "transferFunction-2": ["edge-tf-to-scope"],
                    "scope-3": []
                }
            }
        })
    }

    fn second_order_transfer_function_fixture_value() -> Value {
        json!({
            "version": 1,
            "kind": "ctrl-lab-project",
            "generatedAt": "2026-04-12T00:00:00.000Z",
            "title": "second-order-transfer-function",
            "simulation": {
                "endTime": 1.0,
                "stepSize": 0.1
            },
            "nodes": [
                {
                    "id": "constant-1",
                    "type": "constant",
                    "label": "Constant",
                    "role": "const-01",
                    "position": { "x": 0, "y": 0 },
                    "properties": { "value": "1.0", "dataType": "f32" }
                },
                {
                    "id": "transferFunction-2",
                    "type": "transferFunction",
                    "label": "Transfer Function",
                    "role": "tf-01",
                    "position": { "x": 160, "y": 0 },
                    "properties": {
                        "numerator": "1.0",
                        "denominator": "1.0 1.0 1.0",
                        "stateName": "x",
                        "dataType": "f32"
                    }
                },
                {
                    "id": "scope-3",
                    "type": "scope",
                    "label": "Scope",
                    "role": "scope-01",
                    "position": { "x": 320, "y": 0 },
                    "properties": {
                        "channel": "CH-1",
                        "timebase": "1 s/div",
                        "dataType": "f32"
                    }
                }
            ],
            "edges": [
                {
                    "id": "edge-constant-to-tf",
                    "sourceNodeId": "constant-1",
                    "sourcePortId": "out",
                    "targetNodeId": "transferFunction-2",
                    "targetPortId": "in"
                },
                {
                    "id": "edge-tf-to-scope",
                    "sourceNodeId": "transferFunction-2",
                    "sourcePortId": "out",
                    "targetNodeId": "scope-3",
                    "targetPortId": "in"
                }
            ],
            "graphIndex": {
                "nodesById": {
                    "constant-1": {
                        "type": "constant",
                        "role": "const-01",
                        "inputPortIds": [],
                        "outputPortIds": ["out"]
                    },
                    "transferFunction-2": {
                        "type": "transferFunction",
                        "role": "tf-01",
                        "inputPortIds": ["in"],
                        "outputPortIds": ["out"]
                    },
                    "scope-3": {
                        "type": "scope",
                        "role": "scope-01",
                        "inputPortIds": ["in"],
                        "outputPortIds": []
                    }
                },
                "incomingEdgesByNodeId": {
                    "constant-1": [],
                    "transferFunction-2": ["edge-constant-to-tf"],
                    "scope-3": ["edge-tf-to-scope"]
                },
                "outgoingEdgesByNodeId": {
                    "constant-1": ["edge-constant-to-tf"],
                    "transferFunction-2": ["edge-tf-to-scope"],
                    "scope-3": []
                }
            }
        })
    }

    fn same_order_transfer_function_fixture_value() -> Value {
        json!({
            "version": 1,
            "kind": "ctrl-lab-project",
            "generatedAt": "2026-04-12T00:00:00.000Z",
            "title": "same-order-transfer-function",
            "simulation": {
                "endTime": 1.0,
                "stepSize": 0.1
            },
            "nodes": [
                {
                    "id": "constant-1",
                    "type": "constant",
                    "label": "Constant",
                    "role": "const-01",
                    "position": { "x": 0, "y": 0 },
                    "properties": { "value": "1.0", "dataType": "f32" }
                },
                {
                    "id": "transferFunction-2",
                    "type": "transferFunction",
                    "label": "Transfer Function",
                    "role": "tf-01",
                    "position": { "x": 160, "y": 0 },
                    "properties": {
                        "numerator": "1.0 1.0",
                        "denominator": "1.0 1.0",
                        "stateName": "x",
                        "dataType": "f32"
                    }
                },
                {
                    "id": "scope-3",
                    "type": "scope",
                    "label": "Scope",
                    "role": "scope-01",
                    "position": { "x": 320, "y": 0 },
                    "properties": {
                        "channel": "CH-1",
                        "timebase": "1 s/div",
                        "dataType": "f32"
                    }
                }
            ],
            "edges": [
                {
                    "id": "edge-constant-to-tf",
                    "sourceNodeId": "constant-1",
                    "sourcePortId": "out",
                    "targetNodeId": "transferFunction-2",
                    "targetPortId": "in"
                },
                {
                    "id": "edge-tf-to-scope",
                    "sourceNodeId": "transferFunction-2",
                    "sourcePortId": "out",
                    "targetNodeId": "scope-3",
                    "targetPortId": "in"
                }
            ],
            "graphIndex": {
                "nodesById": {
                    "constant-1": {
                        "type": "constant",
                        "role": "const-01",
                        "inputPortIds": [],
                        "outputPortIds": ["out"]
                    },
                    "transferFunction-2": {
                        "type": "transferFunction",
                        "role": "tf-01",
                        "inputPortIds": ["in"],
                        "outputPortIds": ["out"]
                    },
                    "scope-3": {
                        "type": "scope",
                        "role": "scope-01",
                        "inputPortIds": ["in"],
                        "outputPortIds": []
                    }
                },
                "incomingEdgesByNodeId": {
                    "constant-1": [],
                    "transferFunction-2": ["edge-constant-to-tf"],
                    "scope-3": ["edge-tf-to-scope"]
                },
                "outgoingEdgesByNodeId": {
                    "constant-1": ["edge-constant-to-tf"],
                    "transferFunction-2": ["edge-tf-to-scope"],
                    "scope-3": []
                }
            }
        })
    }

    fn algebraic_loop_with_same_order_transfer_function_fixture_value() -> Value {
        json!({
            "version": 1,
            "kind": "ctrl-lab-project",
            "generatedAt": "2026-04-12T00:00:00.000Z",
            "title": "same-order-tf-algebraic-loop",
            "simulation": {
                "endTime": 0.2,
                "stepSize": 0.1
            },
            "nodes": [
                {
                    "id": "constant-1",
                    "type": "constant",
                    "label": "Constant",
                    "role": "const-01",
                    "position": { "x": 0, "y": 0 },
                    "properties": { "value": "1.0", "dataType": "f32" }
                },
                {
                    "id": "sum-2",
                    "type": "sum",
                    "label": "Summing Node",
                    "role": "sum-01",
                    "position": { "x": 144, "y": 0 },
                    "properties": { "equation": "+ +", "dataType": "f32" }
                },
                {
                    "id": "transferFunction-3",
                    "type": "transferFunction",
                    "label": "Transfer Function",
                    "role": "tf-01",
                    "position": { "x": 288, "y": 0 },
                    "properties": {
                        "numerator": "1.0 1.0",
                        "denominator": "1.0 1.0",
                        "stateName": "x",
                        "dataType": "f32"
                    }
                }
            ],
            "edges": [
                {
                    "id": "edge-constant-to-sum-a",
                    "sourceNodeId": "constant-1",
                    "sourcePortId": "out",
                    "targetNodeId": "sum-2",
                    "targetPortId": "a"
                },
                {
                    "id": "edge-tf-to-sum-b",
                    "sourceNodeId": "transferFunction-3",
                    "sourcePortId": "out",
                    "targetNodeId": "sum-2",
                    "targetPortId": "b"
                },
                {
                    "id": "edge-sum-to-tf",
                    "sourceNodeId": "sum-2",
                    "sourcePortId": "out",
                    "targetNodeId": "transferFunction-3",
                    "targetPortId": "in"
                }
            ],
            "graphIndex": {
                "nodesById": {
                    "constant-1": {
                        "type": "constant",
                        "role": "const-01",
                        "inputPortIds": [],
                        "outputPortIds": ["out"]
                    },
                    "sum-2": {
                        "type": "sum",
                        "role": "sum-01",
                        "inputPortIds": ["a", "b"],
                        "outputPortIds": ["out"]
                    },
                    "transferFunction-3": {
                        "type": "transferFunction",
                        "role": "tf-01",
                        "inputPortIds": ["in"],
                        "outputPortIds": ["out"]
                    }
                },
                "incomingEdgesByNodeId": {
                    "constant-1": [],
                    "sum-2": ["edge-constant-to-sum-a", "edge-tf-to-sum-b"],
                    "transferFunction-3": ["edge-sum-to-tf"]
                },
                "outgoingEdgesByNodeId": {
                    "constant-1": ["edge-constant-to-sum-a"],
                    "sum-2": ["edge-sum-to-tf"],
                    "transferFunction-3": ["edge-tf-to-sum-b"]
                }
            }
        })
    }

    fn switch_fixture_value(selector_value: &str) -> Value {
        json!({
            "version": 1,
            "kind": "ctrl-lab-project",
            "generatedAt": "2026-04-12T00:00:00.000Z",
            "title": "switch-and-step",
            "simulation": {
                "endTime": 2.0,
                "stepSize": 0.5
            },
            "nodes": [
                {
                    "id": "step-1",
                    "type": "step",
                    "label": "Step",
                    "role": "step-01",
                    "position": { "x": 0, "y": 0 },
                    "properties": {
                        "initialValue": "2.0",
                        "finalValue": "5.0",
                        "stepTime": "1.0",
                        "dataType": "f32"
                    }
                },
                {
                    "id": "constant-2",
                    "type": "constant",
                    "label": "Constant",
                    "role": "const-01",
                    "position": { "x": 0, "y": 120 },
                    "properties": { "value": "20.0", "dataType": "f32" }
                },
                {
                    "id": "constant-3",
                    "type": "constant",
                    "label": "Constant",
                    "role": "const-02",
                    "position": { "x": 0, "y": 240 },
                    "properties": { "value": selector_value, "dataType": "f32" }
                },
                {
                    "id": "switch-4",
                    "type": "switch",
                    "label": "Switch",
                    "role": "sw-01",
                    "position": { "x": 180, "y": 120 },
                    "properties": { "dataType": "f32" }
                },
                {
                    "id": "scope-5",
                    "type": "scope",
                    "label": "Scope",
                    "role": "scope-01",
                    "position": { "x": 360, "y": 120 },
                    "properties": {
                        "channel": "CH-1",
                        "timebase": "1 s/div",
                        "dataType": "f32"
                    }
                }
            ],
            "edges": [
                {
                    "id": "edge-step-to-switch-a",
                    "sourceNodeId": "step-1",
                    "sourcePortId": "out",
                    "targetNodeId": "switch-4",
                    "targetPortId": "a"
                },
                {
                    "id": "edge-const-to-switch-b",
                    "sourceNodeId": "constant-2",
                    "sourcePortId": "out",
                    "targetNodeId": "switch-4",
                    "targetPortId": "b"
                },
                {
                    "id": "edge-selector-to-switch-sel",
                    "sourceNodeId": "constant-3",
                    "sourcePortId": "out",
                    "targetNodeId": "switch-4",
                    "targetPortId": "sel"
                },
                {
                    "id": "edge-switch-to-scope",
                    "sourceNodeId": "switch-4",
                    "sourcePortId": "out",
                    "targetNodeId": "scope-5",
                    "targetPortId": "in"
                }
            ],
            "graphIndex": {
                "nodesById": {
                    "step-1": {
                        "type": "step",
                        "role": "step-01",
                        "inputPortIds": [],
                        "outputPortIds": ["out"]
                    },
                    "constant-2": {
                        "type": "constant",
                        "role": "const-01",
                        "inputPortIds": [],
                        "outputPortIds": ["out"]
                    },
                    "constant-3": {
                        "type": "constant",
                        "role": "const-02",
                        "inputPortIds": [],
                        "outputPortIds": ["out"]
                    },
                    "switch-4": {
                        "type": "switch",
                        "role": "sw-01",
                        "inputPortIds": ["a", "sel", "b"],
                        "outputPortIds": ["out"]
                    },
                    "scope-5": {
                        "type": "scope",
                        "role": "scope-01",
                        "inputPortIds": ["in"],
                        "outputPortIds": []
                    }
                },
                "incomingEdgesByNodeId": {
                    "step-1": [],
                    "constant-2": [],
                    "constant-3": [],
                    "switch-4": [
                        "edge-step-to-switch-a",
                        "edge-const-to-switch-b",
                        "edge-selector-to-switch-sel"
                    ],
                    "scope-5": ["edge-switch-to-scope"]
                },
                "outgoingEdgesByNodeId": {
                    "step-1": ["edge-step-to-switch-a"],
                    "constant-2": ["edge-const-to-switch-b"],
                    "constant-3": ["edge-selector-to-switch-sel"],
                    "switch-4": ["edge-switch-to-scope"],
                    "scope-5": []
                }
            }
        })
    }

    fn delay_fixture_value(delay_time: &str) -> Value {
        json!({
            "version": 1,
            "kind": "ctrl-lab-project",
            "generatedAt": "2026-04-12T00:00:00.000Z",
            "title": "delay-test",
            "simulation": {
                "endTime": 0.4,
                "stepSize": 0.1
            },
            "nodes": [
                {
                    "id": "constant-1",
                    "type": "constant",
                    "label": "Constant",
                    "role": "const-01",
                    "position": { "x": 0, "y": 0 },
                    "properties": { "value": "1.0", "dataType": "f32" }
                },
                {
                    "id": "delay-2",
                    "type": "delay",
                    "label": "Delay",
                    "role": "dly-01",
                    "position": { "x": 144, "y": 0 },
                    "properties": {
                        "initialValue": "0.0",
                        "delayTime": delay_time,
                        "dataType": "f32"
                    }
                },
                {
                    "id": "scope-3",
                    "type": "scope",
                    "label": "Scope",
                    "role": "scope-01",
                    "position": { "x": 288, "y": 0 },
                    "properties": {
                        "channel": "CH-1",
                        "timebase": "1 s/div",
                        "dataType": "f32"
                    }
                }
            ],
            "edges": [
                {
                    "id": "edge-constant-to-delay",
                    "sourceNodeId": "constant-1",
                    "sourcePortId": "out",
                    "targetNodeId": "delay-2",
                    "targetPortId": "in"
                },
                {
                    "id": "edge-delay-to-scope",
                    "sourceNodeId": "delay-2",
                    "sourcePortId": "out",
                    "targetNodeId": "scope-3",
                    "targetPortId": "in"
                }
            ],
            "graphIndex": {
                "nodesById": {
                    "constant-1": {
                        "type": "constant",
                        "role": "const-01",
                        "inputPortIds": [],
                        "outputPortIds": ["out"]
                    },
                    "delay-2": {
                        "type": "delay",
                        "role": "dly-01",
                        "inputPortIds": ["in"],
                        "outputPortIds": ["out"]
                    },
                    "scope-3": {
                        "type": "scope",
                        "role": "scope-01",
                        "inputPortIds": ["in"],
                        "outputPortIds": []
                    }
                },
                "incomingEdgesByNodeId": {
                    "constant-1": [],
                    "delay-2": ["edge-constant-to-delay"],
                    "scope-3": ["edge-delay-to-scope"]
                },
                "outgoingEdgesByNodeId": {
                    "constant-1": ["edge-constant-to-delay"],
                    "delay-2": ["edge-delay-to-scope"],
                    "scope-3": []
                }
            }
        })
    }

    fn gain_fixture_value() -> Value {
        json!({
            "version": 1,
            "kind": "ctrl-lab-project",
            "generatedAt": "2026-04-12T00:00:00.000Z",
            "title": "gain-test",
            "simulation": {
                "endTime": 0.2,
                "stepSize": 0.1
            },
            "nodes": [
                {
                    "id": "constant-1",
                    "type": "constant",
                    "label": "Constant",
                    "role": "const-01",
                    "position": { "x": 0, "y": 0 },
                    "properties": { "value": "2.0", "dataType": "f32" }
                },
                {
                    "id": "gain-2",
                    "type": "gain",
                    "label": "Gain",
                    "role": "gain-01",
                    "position": { "x": 144, "y": 0 },
                    "properties": { "gain": "3.0", "dataType": "f32" }
                },
                {
                    "id": "scope-3",
                    "type": "scope",
                    "label": "Scope",
                    "role": "scope-01",
                    "position": { "x": 288, "y": 0 },
                    "properties": {
                        "channel": "CH-1",
                        "timebase": "1 s/div",
                        "dataType": "f32"
                    }
                }
            ],
            "edges": [
                {
                    "id": "edge-constant-to-gain",
                    "sourceNodeId": "constant-1",
                    "sourcePortId": "out",
                    "targetNodeId": "gain-2",
                    "targetPortId": "in"
                },
                {
                    "id": "edge-gain-to-scope",
                    "sourceNodeId": "gain-2",
                    "sourcePortId": "out",
                    "targetNodeId": "scope-3",
                    "targetPortId": "in"
                }
            ],
            "graphIndex": {
                "nodesById": {
                    "constant-1": {
                        "type": "constant",
                        "role": "const-01",
                        "inputPortIds": [],
                        "outputPortIds": ["out"]
                    },
                    "gain-2": {
                        "type": "gain",
                        "role": "gain-01",
                        "inputPortIds": ["in"],
                        "outputPortIds": ["out"]
                    },
                    "scope-3": {
                        "type": "scope",
                        "role": "scope-01",
                        "inputPortIds": ["in"],
                        "outputPortIds": []
                    }
                },
                "incomingEdgesByNodeId": {
                    "constant-1": [],
                    "gain-2": ["edge-constant-to-gain"],
                    "scope-3": ["edge-gain-to-scope"]
                },
                "outgoingEdgesByNodeId": {
                    "constant-1": ["edge-constant-to-gain"],
                    "gain-2": ["edge-gain-to-scope"],
                    "scope-3": []
                }
            }
        })
    }

    fn square_wave_fixture_value() -> Value {
        json!({
            "version": 1,
            "kind": "ctrl-lab-project",
            "generatedAt": "2026-04-12T00:00:00.000Z",
            "title": "square-wave-test",
            "simulation": {
                "endTime": 1.0,
                "stepSize": 0.25
            },
            "nodes": [
                {
                    "id": "squareWave-1",
                    "type": "squareWave",
                    "label": "Square Wave",
                    "role": "wave-01",
                    "position": { "x": 0, "y": 0 },
                    "properties": {
                        "amplitude": "2.0",
                        "frequency": "1.0",
                        "duty": "25",
                        "dataType": "f32"
                    }
                },
                {
                    "id": "scope-2",
                    "type": "scope",
                    "label": "Scope",
                    "role": "scope-01",
                    "position": { "x": 144, "y": 0 },
                    "properties": {
                        "channel": "CH-1",
                        "timebase": "1 s/div",
                        "dataType": "f32"
                    }
                }
            ],
            "edges": [
                {
                    "id": "edge-wave-to-scope",
                    "sourceNodeId": "squareWave-1",
                    "sourcePortId": "out",
                    "targetNodeId": "scope-2",
                    "targetPortId": "in"
                }
            ],
            "graphIndex": {
                "nodesById": {
                    "squareWave-1": {
                        "type": "squareWave",
                        "role": "wave-01",
                        "inputPortIds": [],
                        "outputPortIds": ["out"]
                    },
                    "scope-2": {
                        "type": "scope",
                        "role": "scope-01",
                        "inputPortIds": ["in"],
                        "outputPortIds": []
                    }
                },
                "incomingEdgesByNodeId": {
                    "squareWave-1": [],
                    "scope-2": ["edge-wave-to-scope"]
                },
                "outgoingEdgesByNodeId": {
                    "squareWave-1": ["edge-wave-to-scope"],
                    "scope-2": []
                }
            }
        })
    }

    fn stateful_feedback_fixture_value() -> Value {
        json!({
            "version": 1,
            "kind": "ctrl-lab-project",
            "generatedAt": "2026-04-12T00:00:00.000Z",
            "title": "stateful-feedback",
            "simulation": {
                "endTime": 0.3,
                "stepSize": 0.1
            },
            "nodes": [
                {
                    "id": "constant-1",
                    "type": "constant",
                    "label": "Constant",
                    "role": "const-01",
                    "position": { "x": 0, "y": 0 },
                    "properties": { "value": "1.0", "dataType": "f32" }
                },
                {
                    "id": "integrator-2",
                    "type": "integrator",
                    "label": "Integrator",
                    "role": "int-01",
                    "position": { "x": 288, "y": 0 },
                    "properties": { "initialValue": "0.0", "dataType": "f32" }
                },
                {
                    "id": "sum-3",
                    "type": "sum",
                    "label": "Summing Node",
                    "role": "sum-01",
                    "position": { "x": 144, "y": 0 },
                    "properties": { "equation": "+ +", "dataType": "f32" }
                },
                {
                    "id": "scope-4",
                    "type": "scope",
                    "label": "Scope",
                    "role": "scope-01",
                    "position": { "x": 432, "y": 0 },
                    "properties": {
                        "channel": "CH-1",
                        "timebase": "1 s/div",
                        "dataType": "f32"
                    }
                }
            ],
            "edges": [
                {
                    "id": "edge-constant-to-sum-a",
                    "sourceNodeId": "constant-1",
                    "sourcePortId": "out",
                    "targetNodeId": "sum-3",
                    "targetPortId": "a"
                },
                {
                    "id": "edge-integrator-to-sum-b",
                    "sourceNodeId": "integrator-2",
                    "sourcePortId": "out",
                    "targetNodeId": "sum-3",
                    "targetPortId": "b"
                },
                {
                    "id": "edge-sum-to-integrator",
                    "sourceNodeId": "sum-3",
                    "sourcePortId": "out",
                    "targetNodeId": "integrator-2",
                    "targetPortId": "in"
                },
                {
                    "id": "edge-integrator-to-scope",
                    "sourceNodeId": "integrator-2",
                    "sourcePortId": "out",
                    "targetNodeId": "scope-4",
                    "targetPortId": "in"
                }
            ],
            "graphIndex": {
                "nodesById": {
                    "constant-1": {
                        "type": "constant",
                        "role": "const-01",
                        "inputPortIds": [],
                        "outputPortIds": ["out"]
                    },
                    "integrator-2": {
                        "type": "integrator",
                        "role": "int-01",
                        "inputPortIds": ["in"],
                        "outputPortIds": ["out"]
                    },
                    "sum-3": {
                        "type": "sum",
                        "role": "sum-01",
                        "inputPortIds": ["a", "b"],
                        "outputPortIds": ["out"]
                    },
                    "scope-4": {
                        "type": "scope",
                        "role": "scope-01",
                        "inputPortIds": ["in"],
                        "outputPortIds": []
                    }
                },
                "incomingEdgesByNodeId": {
                    "constant-1": [],
                    "integrator-2": ["edge-sum-to-integrator"],
                    "sum-3": ["edge-constant-to-sum-a", "edge-integrator-to-sum-b"],
                    "scope-4": ["edge-integrator-to-scope"]
                },
                "outgoingEdgesByNodeId": {
                    "constant-1": ["edge-constant-to-sum-a"],
                    "integrator-2": ["edge-integrator-to-sum-b", "edge-integrator-to-scope"],
                    "sum-3": ["edge-sum-to-integrator"],
                    "scope-4": []
                }
            }
        })
    }

    fn algebraic_loop_fixture_value() -> Value {
        json!({
            "version": 1,
            "kind": "ctrl-lab-project",
            "generatedAt": "2026-04-12T00:00:00.000Z",
            "title": "algebraic-loop",
            "simulation": {
                "endTime": 0.2,
                "stepSize": 0.1
            },
            "nodes": [
                {
                    "id": "constant-1",
                    "type": "constant",
                    "label": "Constant",
                    "role": "const-01",
                    "position": { "x": 0, "y": 0 },
                    "properties": { "value": "1.0", "dataType": "f32" }
                },
                {
                    "id": "gain-2",
                    "type": "gain",
                    "label": "Gain",
                    "role": "gain-01",
                    "position": { "x": 288, "y": 0 },
                    "properties": { "gain": "2.0", "dataType": "f32" }
                },
                {
                    "id": "sum-3",
                    "type": "sum",
                    "label": "Summing Node",
                    "role": "sum-01",
                    "position": { "x": 144, "y": 0 },
                    "properties": { "equation": "+ +", "dataType": "f32" }
                }
            ],
            "edges": [
                {
                    "id": "edge-constant-to-sum-a",
                    "sourceNodeId": "constant-1",
                    "sourcePortId": "out",
                    "targetNodeId": "sum-3",
                    "targetPortId": "a"
                },
                {
                    "id": "edge-gain-to-sum-b",
                    "sourceNodeId": "gain-2",
                    "sourcePortId": "out",
                    "targetNodeId": "sum-3",
                    "targetPortId": "b"
                },
                {
                    "id": "edge-sum-to-gain",
                    "sourceNodeId": "sum-3",
                    "sourcePortId": "out",
                    "targetNodeId": "gain-2",
                    "targetPortId": "in"
                }
            ],
            "graphIndex": {
                "nodesById": {
                    "constant-1": {
                        "type": "constant",
                        "role": "const-01",
                        "inputPortIds": [],
                        "outputPortIds": ["out"]
                    },
                    "gain-2": {
                        "type": "gain",
                        "role": "gain-01",
                        "inputPortIds": ["in"],
                        "outputPortIds": ["out"]
                    },
                    "sum-3": {
                        "type": "sum",
                        "role": "sum-01",
                        "inputPortIds": ["a", "b"],
                        "outputPortIds": ["out"]
                    }
                },
                "incomingEdgesByNodeId": {
                    "constant-1": [],
                    "gain-2": ["edge-sum-to-gain"],
                    "sum-3": ["edge-constant-to-sum-a", "edge-gain-to-sum-b"]
                },
                "outgoingEdgesByNodeId": {
                    "constant-1": ["edge-constant-to-sum-a"],
                    "gain-2": ["edge-gain-to-sum-b"],
                    "sum-3": ["edge-sum-to-gain"]
                }
            }
        })
    }

    fn assert_approx_eq(left: f64, right: f64) {
        let delta = (left - right).abs();
        assert!(
            delta <= 1e-9,
            "expected {left} to be within tolerance of {right}, delta was {delta}"
        );
    }

    #[test]
    fn parses_valid_fixture_and_returns_topological_order() {
        let dag = parse_project_json(&fixture_json()).expect("fixture should parse");
        assert_eq!(dag.topological_order.len(), 6);
        let order_index: HashMap<_, _> = dag
            .topological_order
            .iter()
            .enumerate()
            .map(|(index, node_id)| (node_id.as_str(), index))
            .collect();
        assert!(order_index["constant-1"] < order_index["sum-3"]);
        assert!(order_index["integrator-2"] < order_index["sum-3"]);
        assert!(order_index["integrator-4"] < order_index["sum-3"]);
        assert!(order_index["sum-3"] < order_index["scope-6"]);
    }

    #[test]
    fn simulates_constant_into_integrator_over_time() {
        let dag = parse_project_json(&fixture_json()).expect("fixture should parse");
        let simulation = simulate_validated_dag(&dag).expect("fixture should simulate");

        assert_eq!(simulation.times.len(), 101);
        assert_eq!(simulation.times.first().copied(), Some(0.0));
        assert_eq!(simulation.times.last().copied(), Some(10.0));

        let integrator_1 = simulation.values_by_node_id.get("integrator-2").unwrap();
        let integrator_2 = simulation.values_by_node_id.get("integrator-4").unwrap();
        let sum = simulation.values_by_node_id.get("sum-3").unwrap();
        let scope = simulation.values_by_node_id.get("scope-6").unwrap();

        assert_approx_eq(integrator_1.first().copied().unwrap(), 0.0);
        assert_approx_eq(integrator_1.get(1).copied().unwrap(), 0.1);
        assert_approx_eq(integrator_1.last().copied().unwrap(), 10.0);
        assert_approx_eq(integrator_2.last().copied().unwrap(), 5.0);
        assert_approx_eq(sum.last().copied().unwrap(), 5.0);
        assert_approx_eq(scope.last().copied().unwrap(), 5.0);
    }

    #[test]
    fn simulates_constant_into_first_order_transfer_function() {
        let dag = parse_value(transfer_function_fixture_value())
            .expect("transfer function fixture should parse");
        let simulation =
            simulate_validated_dag(&dag).expect("transfer function fixture should simulate");

        assert_eq!(simulation.times.len(), 11);

        let tf = simulation
            .values_by_node_id
            .get("transferFunction-2")
            .expect("transfer function trace must exist");
        let scope = simulation
            .values_by_node_id
            .get("scope-3")
            .expect("scope trace must exist");

        assert_approx_eq(tf.first().copied().unwrap(), 0.0);
        assert_approx_eq(tf.get(1).copied().unwrap(), 0.1);
        assert_approx_eq(*tf.last().unwrap(), 0.6513215599);
        assert_approx_eq(*scope.last().unwrap(), 0.6513215599);
    }

    #[test]
    fn simulates_same_order_first_order_transfer_function() {
        let dag = parse_value(same_order_transfer_function_fixture_value())
            .expect("same-order transfer function fixture should parse");
        let simulation = simulate_validated_dag(&dag)
            .expect("same-order transfer function fixture should simulate");

        let tf = simulation
            .values_by_node_id
            .get("transferFunction-2")
            .expect("transfer function trace must exist");
        let scope = simulation
            .values_by_node_id
            .get("scope-3")
            .expect("scope trace must exist");

        assert_approx_eq(tf[0], 1.0);
        assert_approx_eq(tf[1], 1.0);
        assert_approx_eq(tf[2], 1.0);
        assert_approx_eq(*tf.last().unwrap(), 1.0);
        assert_approx_eq(*scope.last().unwrap(), 1.0);
    }

    #[test]
    fn simulates_second_order_system_fixture_with_strictly_proper_feedback() {
        let dag = parse_project_json(&second_order_system_fixture_json())
            .expect("second-order system fixture should parse");
        let simulation = simulate_validated_dag(&dag)
            .expect("second-order system fixture should simulate");

        let tf = simulation
            .values_by_node_id
            .get("transferFunction-9")
            .expect("transferFunction-9 trace must exist");
        let scope = simulation
            .values_by_node_id
            .get("scope-10")
            .expect("scope-10 trace must exist");

        assert_eq!(simulation.times.len(), 101);
        assert_eq!(tf.len(), simulation.times.len());
        assert_eq!(scope.len(), simulation.times.len());
        assert!(tf.iter().all(|value| value.is_finite()));
        assert!(scope.iter().all(|value| value.is_finite()));
    }

    #[test]
    fn simulates_constant_into_second_order_transfer_function() {
        let dag = parse_value(second_order_transfer_function_fixture_value())
            .expect("second-order transfer function fixture should parse");
        let simulation = simulate_validated_dag(&dag)
            .expect("second-order transfer function fixture should simulate");

        assert_eq!(simulation.times.len(), 11);

        let tf = simulation
            .values_by_node_id
            .get("transferFunction-2")
            .expect("transfer function trace must exist");
        let scope = simulation
            .values_by_node_id
            .get("scope-3")
            .expect("scope trace must exist");

        assert_approx_eq(tf[0], 0.0);
        assert_approx_eq(tf[1], 0.0);
        assert_approx_eq(tf[2], 0.01);
        assert_approx_eq(tf[3], 0.029);
        assert_approx_eq(*tf.last().unwrap(), 0.33231044);
        assert_approx_eq(*scope.last().unwrap(), 0.33231044);
    }

    #[test]
    fn simulates_second_order_transfer_function_with_first_order_numerator() {
        let mut value = second_order_transfer_function_fixture_value();
        value["nodes"][1]["properties"]["numerator"] = json!("1.0 1.0");

        let dag = parse_value(value)
            .expect("second-order transfer function with first-order numerator should parse");
        let simulation = simulate_validated_dag(&dag)
            .expect("second-order transfer function with first-order numerator should simulate");

        let tf = simulation
            .values_by_node_id
            .get("transferFunction-2")
            .expect("transfer function trace must exist");

        assert_approx_eq(tf[0], 0.0);
        assert_approx_eq(tf[1], 0.1);
        assert_approx_eq(tf[2], 0.2);
        assert_approx_eq(*tf.last().unwrap(), 0.9008019901);
    }

    #[test]
    fn simulates_step_source_values() {
        let dag = parse_value(switch_fixture_value("0")).expect("step fixture should parse");
        let simulation = simulate_validated_dag(&dag).expect("step fixture should simulate");

        let step = simulation
            .values_by_node_id
            .get("step-1")
            .expect("step trace must exist");

        assert_approx_eq(step[0], 2.0);
        assert_approx_eq(step[1], 2.0);
        assert_approx_eq(step[2], 5.0);
        assert_approx_eq(*step.last().unwrap(), 5.0);
    }

    #[test]
    fn simulates_delay_block_values() {
        let dag = parse_value(delay_fixture_value("0.2")).expect("delay fixture should parse");
        let simulation = simulate_validated_dag(&dag).expect("delay fixture should simulate");

        let delay = simulation
            .values_by_node_id
            .get("delay-2")
            .expect("delay trace must exist");

        assert_approx_eq(delay[0], 0.0);
        assert_approx_eq(delay[1], 0.0);
        assert_approx_eq(delay[2], 1.0);
        assert_approx_eq(*delay.last().unwrap(), 1.0);
    }

    #[test]
    fn simulates_constant_through_gain() {
        let dag = parse_value(gain_fixture_value()).expect("gain fixture should parse");
        let simulation = simulate_validated_dag(&dag).expect("gain fixture should simulate");

        let gain = simulation
            .values_by_node_id
            .get("gain-2")
            .expect("gain trace must exist");

        assert!(gain.iter().all(|value| (*value - 6.0).abs() <= 1e-9));
    }

    #[test]
    fn simulates_square_wave_source_values() {
        let dag =
            parse_value(square_wave_fixture_value()).expect("square wave fixture should parse");
        let simulation = simulate_validated_dag(&dag).expect("square wave fixture should simulate");

        let wave = simulation
            .values_by_node_id
            .get("squareWave-1")
            .expect("square wave trace must exist");

        assert_approx_eq(wave[0], 2.0);
        assert_approx_eq(wave[1], 0.0);
        assert_approx_eq(wave[2], 0.0);
        assert_approx_eq(wave[3], 0.0);
        assert_approx_eq(wave[4], 2.0);
    }

    #[test]
    fn compiles_stateful_feedback_loop_and_simulates_deterministically() {
        let dag = parse_value(stateful_feedback_fixture_value())
            .expect("stateful feedback fixture should parse");
        let simulation =
            simulate_validated_dag(&dag).expect("stateful feedback fixture should simulate");

        let order_index: HashMap<_, _> = dag
            .topological_order
            .iter()
            .enumerate()
            .map(|(index, node_id)| (node_id.as_str(), index))
            .collect();
        assert!(order_index["constant-1"] < order_index["sum-3"]);
        assert!(order_index["integrator-2"] < order_index["sum-3"]);
        assert!(order_index["integrator-2"] < order_index["scope-4"]);
        assert_eq!(
            dag.block_behaviors.get("integrator-2"),
            Some(&BlockBehavior {
                is_stateful: true,
                is_direct_feedthrough: false,
            })
        );
        assert_eq!(
            dag.block_behaviors.get("sum-3"),
            Some(&BlockBehavior {
                is_stateful: false,
                is_direct_feedthrough: true,
            })
        );

        let integrator = simulation
            .values_by_node_id
            .get("integrator-2")
            .expect("integrator trace must exist");
        let sum = simulation
            .values_by_node_id
            .get("sum-3")
            .expect("sum trace must exist");

        assert_approx_eq(integrator[0], 0.0);
        assert_approx_eq(integrator[1], 0.1);
        assert_approx_eq(integrator[2], 0.21);
        assert_approx_eq(sum[0], 1.0);
        assert_approx_eq(sum[1], 1.1);
        assert_approx_eq(sum[2], 1.21);
    }

    #[test]
    fn classifies_same_order_transfer_function_as_direct_feedthrough() {
        let dag = parse_value(same_order_transfer_function_fixture_value())
            .expect("same-order transfer function fixture should parse");

        assert_eq!(
            dag.block_behaviors.get("transferFunction-2"),
            Some(&BlockBehavior {
                is_stateful: true,
                is_direct_feedthrough: true,
            })
        );
    }

    #[test]
    fn rejects_algebraic_loop_between_sum_and_gain() {
        assert_eq!(
            parse_value(algebraic_loop_fixture_value()),
            Err(ParseError::AlgebraicLoopDetected)
        );
    }

    #[test]
    fn delay_uses_global_step_size_when_delay_time_is_negative_one() {
        let dag = parse_value(delay_fixture_value("-1")).expect("delay fixture should parse");
        let simulation = simulate_validated_dag(&dag).expect("delay fixture should simulate");

        let delay = simulation
            .values_by_node_id
            .get("delay-2")
            .expect("delay trace must exist");

        assert_approx_eq(delay[0], 0.0);
        assert_approx_eq(delay[1], 1.0);
        assert_approx_eq(delay[2], 1.0);
        assert_approx_eq(*delay.last().unwrap(), 1.0);
    }

    #[test]
    fn rejects_negative_delay_time() {
        let dag = parse_value(delay_fixture_value("-0.1")).expect("delay fixture should parse");

        assert_eq!(
            simulate_validated_dag(&dag),
            Err(SimulationError::InvalidDelayTime {
                node_id: "delay-2".to_string(),
                delay_time: -0.1,
            })
        );
    }

    #[test]
    fn switch_routes_input_a_when_selector_is_zero() {
        let dag = parse_value(switch_fixture_value("0")).expect("switch fixture should parse");
        let simulation = simulate_validated_dag(&dag).expect("switch fixture should simulate");

        let switch = simulation
            .values_by_node_id
            .get("switch-4")
            .expect("switch trace must exist");

        assert_approx_eq(switch[0], 2.0);
        assert_approx_eq(switch[1], 2.0);
        assert_approx_eq(switch[2], 5.0);
        assert_approx_eq(*switch.last().unwrap(), 5.0);
    }

    #[test]
    fn switch_routes_input_b_when_selector_is_one() {
        let dag = parse_value(switch_fixture_value("1")).expect("switch fixture should parse");
        let simulation = simulate_validated_dag(&dag).expect("switch fixture should simulate");

        let switch = simulation
            .values_by_node_id
            .get("switch-4")
            .expect("switch trace must exist");

        assert!(switch.iter().all(|value| (*value - 20.0).abs() <= 1e-9));
    }

    #[test]
    fn switch_rejects_invalid_selector_values() {
        let dag = parse_value(switch_fixture_value("2")).expect("switch fixture should parse");

        assert_eq!(
            simulate_validated_dag(&dag),
            Err(SimulationError::InvalidSwitchSelector {
                node_id: "switch-4".to_string(),
                value: 2.0,
            })
        );
    }

    #[test]
    fn rejects_invalid_transfer_function_coefficients() {
        let mut value = transfer_function_fixture_value();
        value["nodes"][1]["properties"]["numerator"] = json!("1.0 nope");

        let dag = parse_value(value).expect("graph shape should still validate");
        assert_eq!(
            simulate_validated_dag(&dag),
            Err(SimulationError::InvalidCoefficientList {
                node_id: "transferFunction-2".to_string(),
                property: "numerator".to_string(),
                value: "1.0 nope".to_string(),
            })
        );
    }

    #[test]
    fn rejects_unsupported_transfer_function_shape() {
        let mut value = transfer_function_fixture_value();
        value["nodes"][1]["properties"]["numerator"] = json!("1.0 0.5 0.25");

        let dag = parse_value(value).expect("graph shape should still validate");
        assert_eq!(
            simulate_validated_dag(&dag),
            Err(SimulationError::UnsupportedTransferFunctionShape {
                node_id: "transferFunction-2".to_string(),
                numerator_len: 3,
                denominator_len: 2,
            })
        );
    }

    #[test]
    fn rejects_unsupported_node_type_during_simulation() {
        let mut value = fixture_value();
        value["nodes"][0]["type"] = json!("mystery");
        value["graphIndex"]["nodesById"]["constant-1"]["type"] = json!("mystery");

        let dag = parse_value(value).expect("graph shape should still validate");
        assert_eq!(
            simulate_validated_dag(&dag),
            Err(SimulationError::UnsupportedNodeType {
                node_id: "constant-1".to_string(),
                node_type: "mystery".to_string(),
            })
        );
    }

    #[test]
    fn rejects_duplicate_node_id() {
        let mut value = fixture_value();
        let duplicate = value["nodes"][0].clone();
        value["nodes"].as_array_mut().unwrap().push(duplicate);

        assert_eq!(
            parse_value(value),
            Err(ParseError::DuplicateNodeId {
                node_id: "constant-1".to_string(),
            })
        );
    }

    #[test]
    fn rejects_duplicate_edge_id() {
        let mut value = fixture_value();
        let duplicate = value["edges"][0].clone();
        value["edges"].as_array_mut().unwrap().push(duplicate);

        assert_eq!(
            parse_value(value),
            Err(ParseError::DuplicateEdgeId {
                edge_id: "xy-edge__constant-1out-integrator-2in".to_string(),
            })
        );
    }

    #[test]
    fn rejects_missing_source_node_reference() {
        let mut value = fixture_value();
        value["edges"][0]["sourceNodeId"] = Value::String("missing-source".to_string());

        assert_eq!(
            parse_value(value),
            Err(ParseError::SourceNodeMissing {
                edge_id: "xy-edge__constant-1out-integrator-2in".to_string(),
                node_id: "missing-source".to_string(),
            })
        );
    }

    #[test]
    fn rejects_missing_target_node_reference() {
        let mut value = fixture_value();
        value["edges"][0]["targetNodeId"] = Value::String("missing-target".to_string());

        assert_eq!(
            parse_value(value),
            Err(ParseError::TargetNodeMissing {
                edge_id: "xy-edge__constant-1out-integrator-2in".to_string(),
                node_id: "missing-target".to_string(),
            })
        );
    }

    #[test]
    fn rejects_invalid_source_port_reference() {
        let mut value = fixture_value();
        value["edges"][0]["sourcePortId"] = Value::String("bad-out".to_string());

        assert_eq!(
            parse_value(value),
            Err(ParseError::InvalidSourcePort {
                edge_id: "xy-edge__constant-1out-integrator-2in".to_string(),
                node_id: "constant-1".to_string(),
                port_id: "bad-out".to_string(),
            })
        );
    }

    #[test]
    fn rejects_invalid_target_port_reference() {
        let mut value = fixture_value();
        value["edges"][0]["targetPortId"] = Value::String("bad-in".to_string());

        assert_eq!(
            parse_value(value),
            Err(ParseError::InvalidTargetPort {
                edge_id: "xy-edge__constant-1out-integrator-2in".to_string(),
                node_id: "integrator-2".to_string(),
                port_id: "bad-in".to_string(),
            })
        );
    }

    #[test]
    fn rejects_unconnected_required_input() {
        let mut value = fixture_value();
        value["edges"]
            .as_array_mut()
            .unwrap()
            .retain(|edge| edge["targetNodeId"] != "scope-6");
        value["graphIndex"]["incomingEdgesByNodeId"]["scope-6"] = json!([]);
        value["graphIndex"]["outgoingEdgesByNodeId"]["sum-3"] = json!([]);

        assert_eq!(
            parse_value(value),
            Err(ParseError::UnconnectedRequiredInput {
                node_id: "scope-6".to_string(),
                port_id: "in".to_string(),
            })
        );
    }

    #[test]
    fn rejects_multiple_incoming_edges_on_required_port() {
        let mut value = fixture_value();
        let extra_edge = json!({
            "id": "xy-edge__constant-5out-sum-3a-duplicate",
            "sourceNodeId": "constant-5",
            "sourcePortId": "out",
            "targetNodeId": "sum-3",
            "targetPortId": "a"
        });
        value["edges"].as_array_mut().unwrap().push(extra_edge);
        value["graphIndex"]["incomingEdgesByNodeId"]["sum-3"] = json!([
            "xy-edge__integrator-2out-sum-3a",
            "xy-edge__integrator-4out-sum-3b",
            "xy-edge__constant-5out-sum-3a-duplicate"
        ]);
        value["graphIndex"]["outgoingEdgesByNodeId"]["constant-5"] = json!([
            "xy-edge__constant-5out-integrator-4in",
            "xy-edge__constant-5out-sum-3a-duplicate"
        ]);

        assert_eq!(
            parse_value(value),
            Err(ParseError::MultipleIncomingEdges {
                node_id: "sum-3".to_string(),
                port_id: "a".to_string(),
                edge_ids: vec![
                    "xy-edge__integrator-2out-sum-3a".to_string(),
                    "xy-edge__constant-5out-sum-3a-duplicate".to_string(),
                ],
            })
        );
    }

    #[test]
    fn rejects_cycle() {
        let mut value = fixture_value();
        let cycle_edge = json!({
            "id": "xy-edge__sum-3out-integrator-2in-feedback",
            "sourceNodeId": "sum-3",
            "sourcePortId": "out",
            "targetNodeId": "integrator-2",
            "targetPortId": "in"
        });
        value["edges"].as_array_mut().unwrap().push(cycle_edge);
        value["graphIndex"]["incomingEdgesByNodeId"]["integrator-2"] = json!([
            "xy-edge__constant-1out-integrator-2in",
            "xy-edge__sum-3out-integrator-2in-feedback"
        ]);
        value["graphIndex"]["outgoingEdgesByNodeId"]["sum-3"] = json!([
            "xy-edge__sum-3out-scope-6in",
            "xy-edge__sum-3out-integrator-2in-feedback"
        ]);

        assert_eq!(
            parse_value(value),
            Err(ParseError::MultipleIncomingEdges {
                node_id: "integrator-2".to_string(),
                port_id: "in".to_string(),
                edge_ids: vec![
                    "xy-edge__constant-1out-integrator-2in".to_string(),
                    "xy-edge__sum-3out-integrator-2in-feedback".to_string(),
                ],
            })
        );
    }

    #[test]
    fn rejects_algebraic_loop_without_duplicate_target_port_use() {
        let mut value = fixture_value();

        let nodes = value["nodes"].as_array_mut().unwrap();
        nodes.push(json!({
            "id": "sum-7",
            "type": "sum",
            "label": "Summing Node",
            "role": "sum-02",
            "position": { "x": 768, "y": 360 },
            "properties": {
                "equation": "+ +",
                "dataType": "f32"
            }
        }));

        value["graphIndex"]["nodesById"]["sum-7"] = json!({
            "type": "sum",
            "role": "sum-02",
            "inputPortIds": ["a", "b"],
            "outputPortIds": ["out"]
        });

        let edges = value["edges"].as_array_mut().unwrap();
        edges.retain(|edge| {
            edge["id"] != "xy-edge__integrator-2out-sum-3a"
                && edge["id"] != "xy-edge__sum-3out-scope-6in"
        });
        edges.push(json!({
            "id": "xy-edge__sum-3out-sum-7a",
            "sourceNodeId": "sum-3",
            "sourcePortId": "out",
            "targetNodeId": "sum-7",
            "targetPortId": "a"
        }));
        edges.push(json!({
            "id": "xy-edge__integrator-2out-sum-7b",
            "sourceNodeId": "integrator-2",
            "sourcePortId": "out",
            "targetNodeId": "sum-7",
            "targetPortId": "b"
        }));
        edges.push(json!({
            "id": "xy-edge__sum-7out-scope-6in",
            "sourceNodeId": "sum-7",
            "sourcePortId": "out",
            "targetNodeId": "scope-6",
            "targetPortId": "in"
        }));
        edges.push(json!({
            "id": "xy-edge__scope-6out-sum-3a",
            "sourceNodeId": "scope-6",
            "sourcePortId": "out",
            "targetNodeId": "sum-3",
            "targetPortId": "a"
        }));

        value["graphIndex"]["nodesById"]["scope-6"]["outputPortIds"] = json!(["out"]);
        value["graphIndex"]["incomingEdgesByNodeId"]["sum-3"] = json!([
            "xy-edge__integrator-4out-sum-3b",
            "xy-edge__scope-6out-sum-3a"
        ]);
        value["graphIndex"]["outgoingEdgesByNodeId"]["sum-3"] = json!(["xy-edge__sum-3out-sum-7a"]);
        value["graphIndex"]["incomingEdgesByNodeId"]["sum-7"] = json!([
            "xy-edge__sum-3out-sum-7a",
            "xy-edge__integrator-2out-sum-7b"
        ]);
        value["graphIndex"]["outgoingEdgesByNodeId"]["sum-7"] =
            json!(["xy-edge__sum-7out-scope-6in"]);
        value["graphIndex"]["incomingEdgesByNodeId"]["scope-6"] =
            json!(["xy-edge__sum-7out-scope-6in"]);
        value["graphIndex"]["outgoingEdgesByNodeId"]["scope-6"] =
            json!(["xy-edge__scope-6out-sum-3a"]);
        value["graphIndex"]["outgoingEdgesByNodeId"]["integrator-2"] =
            json!(["xy-edge__integrator-2out-sum-7b"]);
        value["graphIndex"]["outgoingEdgesByNodeId"]["integrator-4"] =
            json!(["xy-edge__integrator-4out-sum-3b"]);

        assert_eq!(parse_value(value), Err(ParseError::AlgebraicLoopDetected));
    }

    #[test]
    fn rejects_algebraic_loop_with_same_order_transfer_function() {
        assert_eq!(
            parse_value(algebraic_loop_with_same_order_transfer_function_fixture_value()),
            Err(ParseError::AlgebraicLoopDetected)
        );
    }

    #[test]
    fn rejects_missing_graph_index() {
        let mut value = fixture_value();
        value.as_object_mut().unwrap().remove("graphIndex");

        assert_eq!(parse_value(value), Err(ParseError::MissingGraphIndex));
    }

    #[test]
    fn rejects_graph_index_node_mismatch() {
        let mut value = fixture_value();
        value["graphIndex"]["nodesById"]
            .as_object_mut()
            .unwrap()
            .remove("scope-6");

        assert_eq!(
            parse_value(value),
            Err(ParseError::GraphIndexNodeMissing {
                node_id: "scope-6".to_string(),
            })
        );
    }

    #[test]
    fn rejects_graph_index_extra_node() {
        let mut value = fixture_value();
        value["graphIndex"]["nodesById"]["extra-node"] = json!({
            "type": "constant",
            "role": "const-extra",
            "inputPortIds": [],
            "outputPortIds": ["out"]
        });
        value["graphIndex"]["incomingEdgesByNodeId"]["extra-node"] = json!([]);
        value["graphIndex"]["outgoingEdgesByNodeId"]["extra-node"] = json!([]);

        assert_eq!(
            parse_value(value),
            Err(ParseError::GraphIndexExtraNode {
                node_id: "extra-node".to_string(),
            })
        );
    }
}
