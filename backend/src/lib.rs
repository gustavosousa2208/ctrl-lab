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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    CycleDetected,
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
            Self::CycleDetected => write!(f, "graph is not a DAG"),
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
    let mut incoming_edges_per_port: HashMap<(NodeId, PortId), Vec<EdgeId>> = HashMap::new();
    let mut adjacency: HashMap<NodeId, Vec<NodeId>> = node_order
        .iter()
        .cloned()
        .map(|node_id| (node_id, Vec::new()))
        .collect();
    let mut indegree: HashMap<NodeId, usize> = node_order
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
        adjacency
            .get_mut(&edge.source_node_id)
            .expect("validated node ids must exist")
            .push(edge.target_node_id.clone());
        *indegree
            .get_mut(&edge.target_node_id)
            .expect("validated node ids must exist") += 1;
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
    for targets in adjacency.values_mut() {
        targets.sort_by_key(|node_id| {
            order_index
                .get(node_id)
                .copied()
                .expect("validated node ids must exist")
        });
    }

    let mut queue = VecDeque::new();
    for node_id in &node_order {
        if indegree.get(node_id).copied().unwrap_or_default() == 0 {
            queue.push_back(node_id.clone());
        }
    }

    let mut topological_order = Vec::with_capacity(node_order.len());
    while let Some(node_id) = queue.pop_front() {
        topological_order.push(node_id.clone());

        if let Some(targets) = adjacency.get(&node_id) {
            for target_id in targets {
                let count = indegree
                    .get_mut(target_id)
                    .expect("validated node ids must exist");
                *count -= 1;
                if *count == 0 {
                    queue.push_back(target_id.clone());
                }
            }
        }
    }

    if topological_order.len() != node_order.len() {
        return Err(ParseError::CycleDetected);
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
    let mut transfer_function_state: HashMap<NodeId, f64> = HashMap::new();
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
        if node.node_type == "transferFunction" {
            transfer_function_state.insert(node.id.clone(), 0.0);
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
                "integrator" => *integrator_state
                    .get(node_id)
                    .expect("integrator state must exist after initialization"),
                "transferFunction" => *transfer_function_state
                    .get(node_id)
                    .expect("transfer function state must exist after initialization"),
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
            for node_id in &dag.topological_order {
                let node = dag
                    .nodes
                    .get(node_id)
                    .expect("validated dag must contain raw nodes");
                if node.node_type == "integrator" {
                    let input =
                        read_input_value(&current_values, &incoming_edges_by_port, node_id, "in")?;
                    let state = integrator_state
                        .get_mut(node_id)
                        .expect("integrator state must exist");
                    *state += input * step_size;
                }
                if node.node_type == "transferFunction" {
                    let input =
                        read_input_value(&current_values, &incoming_edges_by_port, node_id, "in")?;
                    let state = transfer_function_state
                        .get_mut(node_id)
                        .expect("transfer function state must exist");
                    let params = parse_transfer_function(node)?;
                    *state += step_size
                        * ((params.numerator_gain * input - params.output_gain * *state)
                            / params.derivative_gain);
                }
            }
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

#[derive(Debug, Clone, Copy)]
struct FirstOrderTransferFunction {
    derivative_gain: f64,
    output_gain: f64,
    numerator_gain: f64,
}

fn parse_transfer_function(
    node: &SerializedNode,
) -> Result<FirstOrderTransferFunction, SimulationError> {
    let numerator = parse_numeric_list_property(node, "numerator")?;
    let denominator = parse_numeric_list_property(node, "denominator")?;

    if numerator.len() != 1 || denominator.len() != 2 {
        return Err(SimulationError::UnsupportedTransferFunctionShape {
            node_id: node.id.clone(),
            numerator_len: numerator.len(),
            denominator_len: denominator.len(),
        });
    }

    let derivative_gain = denominator[0];
    if !derivative_gain.is_finite() || derivative_gain.abs() <= f64::EPSILON {
        return Err(SimulationError::InvalidTransferFunctionDenominator {
            node_id: node.id.clone(),
        });
    }

    Ok(FirstOrderTransferFunction {
        derivative_gain,
        output_gain: denominator[1],
        numerator_gain: numerator[0],
    })
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

    fn fixture_json() -> String {
        std::fs::read_to_string(fixture_path()).expect("fixture must be readable")
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
        assert_eq!(
            dag.topological_order,
            vec![
                "constant-1".to_string(),
                "constant-5".to_string(),
                "integrator-2".to_string(),
                "integrator-4".to_string(),
                "sum-3".to_string(),
                "scope-6".to_string(),
            ]
        );
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
        value["nodes"][1]["properties"]["numerator"] = json!("1.0 0.5");

        let dag = parse_value(value).expect("graph shape should still validate");
        assert_eq!(
            simulate_validated_dag(&dag),
            Err(SimulationError::UnsupportedTransferFunctionShape {
                node_id: "transferFunction-2".to_string(),
                numerator_len: 2,
                denominator_len: 2,
            })
        );
    }

    #[test]
    fn rejects_unsupported_node_type_during_simulation() {
        let mut value = fixture_value();
        value["nodes"][0]["type"] = json!("squareWave");
        value["graphIndex"]["nodesById"]["constant-1"]["type"] = json!("squareWave");

        let dag = parse_value(value).expect("graph shape should still validate");
        assert_eq!(
            simulate_validated_dag(&dag),
            Err(SimulationError::UnsupportedNodeType {
                node_id: "constant-1".to_string(),
                node_type: "squareWave".to_string(),
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
    fn rejects_non_dag_cycle_without_duplicate_target_port_use() {
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

        assert_eq!(parse_value(value), Err(ParseError::CycleDetected));
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
