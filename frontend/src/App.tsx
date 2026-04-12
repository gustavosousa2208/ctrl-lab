import { Component, createContext, type CSSProperties, type ReactNode } from "react";
import { useContext, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import {
  addEdge,
  applyNodeChanges,
  BaseEdge,
  getSmoothStepPath,
  Handle,
  MarkerType,
  Position,
  ReactFlow,
  ReactFlowProvider,
  useEdgesState,
  useNodesState,
  useReactFlow,
  type Connection,
  type Edge,
  type EdgeProps,
  type Node,
  type NodeChange,
  type NodeProps,
} from "@xyflow/react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";
import "@xyflow/react/dist/style.css";
import type { BlockGraph, BlockType } from "./types/graph";

type PropertyField = {
  key: string;
  label: string;
  inputMode?: "text" | "decimal" | "select";
  options?: string[];
  step?: string;
};

type BlockDefinition = {
  label: string;
  role: string;
  description: string;
  accent: string;
  inputs: string[];
  outputs: string[];
  propertyFields: PropertyField[];
  defaultProperties: Record<string, string>;
};

type CanvasNodeData = BlockDefinition & {
  blockType: BlockType;
  detail: string;
  properties: Record<string, string>;
  inputSignals: Record<string, number | null>;
  signalValue: number | null;
};

type CanvasNode = Node<CanvasNodeData, "controlBlock">;
type CanvasEdge = Edge;

type SerializedNode = {
  id: string;
  type: BlockType;
  label: string;
  role: string;
  position: {
    x: number;
    y: number;
  };
  properties: Record<string, string>;
};

type SerializedEdge = {
  id: string;
  sourceNodeId: string;
  sourcePortId: string | null;
  targetNodeId: string;
  targetPortId: string | null;
};

type ProjectGraphIndex = {
  nodesById: Record<
    string,
    {
      type: BlockType;
      role: string;
      inputPortIds: string[];
      outputPortIds: string[];
    }
  >;
  incomingEdgesByNodeId: Record<string, string[]>;
  outgoingEdgesByNodeId: Record<string, string[]>;
};

type RecentProject = {
  path: string;
  name: string;
  openedAt: string;
};

type ProjectDocument = {
  version: 1;
  kind: "ctrl-lab-project";
  generatedAt: string;
  title: string;
  simulation: {
    endTime: number;
    stepSize: number;
    zoomStep?: number;
    zoom?: number;
    viewportX?: number;
    viewportY?: number;
  };
  nodes: SerializedNode[];
  edges: SerializedEdge[];
  graphIndex?: ProjectGraphIndex;
};

type EditorSnapshot = {
  nodes: SerializedNode[];
  edges: SerializedEdge[];
  endTime: string;
  stepSize: string;
  inspectorNodeId: string | null;
  selectedNodeId: string | null;
  selectedEdgeId: string | null;
  nextNodeNumber: number;
  projectFilePath: string | null;
  isSimulationRunning: boolean;
  timeSeconds: number;
  zoomStep: number;
  zoomLevel: number;
  viewportX: number;
  viewportY: number;
};

const gridSize = 24;
const dataTypeOptions = ["f32", "uint8", "uint16", "uint32", "f64", "char"] as const;

const blockCatalog: Record<BlockType, BlockDefinition> = {
  constant: {
    label: "Constant",
    role: "Constant",
    description: "Fixed source for setpoints and bias injection.",
    accent: "#6b8452",
    inputs: [],
    outputs: ["out"],
    propertyFields: [
      { key: "value", label: "Value", inputMode: "decimal", step: "0.1" },
      { key: "dataType", label: "Data Type", inputMode: "select", options: [...dataTypeOptions] },
    ],
    defaultProperties: {
      value: "1.0",
      dataType: "f32",
    },
  },
  integrator: {
    label: "Integrator",
    role: "Integrator",
    description: "Accumulates the incoming signal over simulation time.",
    accent: "#5d7461",
    inputs: ["in"],
    outputs: ["out"],
    propertyFields: [
      { key: "initialValue", label: "Initial Value", inputMode: "decimal", step: "0.1" },
      { key: "dataType", label: "Data Type", inputMode: "select", options: [...dataTypeOptions] },
    ],
    defaultProperties: {
      initialValue: "0",
      dataType: "f32",
    },
  },
  transferFunction: {
    label: "Transfer Function",
    role: "Transfer Function",
    description: "Linear dynamic block with numerator and denominator coefficients.",
    accent: "#6d6a8a",
    inputs: ["in"],
    outputs: ["out"],
    propertyFields: [
      { key: "numerator", label: "Numerator Coefficients", inputMode: "text" },
      { key: "denominator", label: "Denominator Coefficients", inputMode: "text" },
      { key: "stateName", label: "State Name", inputMode: "text" },
      { key: "dataType", label: "Data Type", inputMode: "select", options: [...dataTypeOptions] },
    ],
    defaultProperties: {
      numerator: "1.0",
      denominator: "1.0 1.0",
      stateName: "x",
      dataType: "f32",
    },
  },
  squareWave: {
    label: "Square Wave Generator",
    role: "Square Wave",
    description: "Pulsed test source for switching logic and timing checks.",
    accent: "#9a7f35",
    inputs: [],
    outputs: ["out"],
    propertyFields: [
      { key: "amplitude", label: "Amplitude", inputMode: "decimal", step: "0.1" },
      { key: "frequency", label: "Frequency (Hz)", inputMode: "decimal", step: "0.1" },
      { key: "duty", label: "Duty Cycle (%)", inputMode: "decimal", step: "1" },
      { key: "dataType", label: "Data Type", inputMode: "select", options: [...dataTypeOptions] },
    ],
    defaultProperties: {
      amplitude: "1.0",
      frequency: "1.0",
      duty: "50",
      dataType: "f32",
    },
  },
  sum: {
    label: "Summing Node",
    role: "Sum",
    description: "Adds or subtracts incoming signals before routing forward.",
    accent: "#58706d",
    inputs: ["a", "b"],
    outputs: ["out"],
    propertyFields: [
      { key: "equation", label: "Equation", inputMode: "text" },
      { key: "dataType", label: "Data Type", inputMode: "select", options: [...dataTypeOptions] },
    ],
    defaultProperties: {
      equation: "+ +",
      dataType: "f32",
    },
  },
  scope: {
    label: "Scope",
    role: "Scope",
    description: "Visual sink for watching the live signal at this point.",
    accent: "#4f6686",
    inputs: ["in"],
    outputs: [],
    propertyFields: [
      { key: "channel", label: "Channel", inputMode: "text" },
      { key: "timebase", label: "Timebase", inputMode: "text" },
      { key: "dataType", label: "Data Type", inputMode: "select", options: [...dataTypeOptions] },
    ],
    defaultProperties: {
      channel: "CH-1",
      timebase: "1 s/div",
      dataType: "f32",
    },
  },
  display: {
    label: "Display",
    role: "Display",
    description: "Numeric readout for the current signal value at this point.",
    accent: "#7b5d4d",
    inputs: ["in"],
    outputs: [],
    propertyFields: [
      { key: "decimals", label: "Decimals", inputMode: "decimal", step: "1" },
      { key: "unit", label: "Unit", inputMode: "text" },
      { key: "dataType", label: "Data Type", inputMode: "select", options: [...dataTypeOptions] },
    ],
    defaultProperties: {
      decimals: "2",
      unit: "",
      dataType: "f32",
    },
  },
};

const emptyGraph: BlockGraph = {
  nodes: [],
  edges: [],
};

const starterBlocks: BlockType[] = ["constant", "integrator", "transferFunction", "squareWave", "sum", "display", "scope"];
const workspaceTitle = "";
const tagPrefixByType: Record<BlockType, string> = {
  constant: "const",
  integrator: "int",
  transferFunction: "tf",
  squareWave: "wave",
  sum: "sum",
  scope: "scope",
  display: "disp",
};

type RackDragState = {
  blockType: BlockType;
  x: number;
  y: number;
};

type FlowSignalsContextValue = {
  signalValues: Map<string, number | null>;
  edges: CanvasEdge[];
};

const defaultMarker = {
  type: MarkerType.ArrowClosed,
  width: 18,
  height: 18,
  color: "#344537",
};

const defaultViewport = { x: 0, y: 0, zoom: 1 };
const defaultFitViewOptions = { padding: 0.16 };
const defaultSnapGrid: [number, number] = [gridSize, gridSize];
const defaultEdgeOptions = { type: "smoothstep", markerEnd: defaultMarker };
const proOptions = { hideAttribution: true };
const emptySignalContext: FlowSignalsContextValue = {
  signalValues: new Map<string, number | null>(),
  edges: [],
};
const FlowSignalsContext = createContext<FlowSignalsContextValue>(emptySignalContext);
const recentProjectsStorageKey = "ctrl-lab-recent-projects";

function parseNumber(value: string | undefined, fallback: number) {
  const parsed = Number.parseFloat(value ?? "");
  return Number.isFinite(parsed) ? parsed : fallback;
}

function containsDecimalComma(value: string | undefined) {
  return (value ?? "").includes(",");
}

function formatSignalValue(value: number | null, decimalsText = "2", unit = "") {
  if (value === null || Number.isNaN(value)) {
    return "--";
  }

  const decimals = Math.max(0, Math.min(6, Math.round(parseNumber(decimalsText, 2))));
  const suffix = unit.trim() ? ` ${unit.trim()}` : "";

  return `${value.toFixed(decimals)}${suffix}`;
}

function formatConstantValue(value: string | undefined) {
  const normalized = (value ?? "").trim();

  if (!normalized) {
    return "-";
  }

  return normalized.length <= 6 ? normalized : "-";
}

const supportedEquationOperators = new Set(["+", "-", "*", "/"]);

function normalizeEquation(equation: string | undefined) {
  const tokens = (equation ?? "").match(/[+\-*/]/g) ?? [];
  const first = supportedEquationOperators.has(tokens[0] ?? "") ? tokens[0] : "+";
  const second = supportedEquationOperators.has(tokens[1] ?? "") ? tokens[1] : "+";

  return `${first} ${second}`;
}

function parseEquationTokens(equation: string | undefined) {
  const [leftOperator, rightOperator] = normalizeEquation(equation).split(" ");
  return { leftOperator, rightOperator };
}

function divideSafely(dividend: number, divisor: number) {
  if (divisor === 0) {
    return 0;
  }

  return dividend / divisor;
}

function clampZoomLevel(zoom: number, zoomStep: number) {
  if (!Number.isFinite(zoom)) {
    return 1;
  }

  const safeStep = Number.isFinite(zoomStep) && zoomStep > 0 ? zoomStep : 0.05;
  return Math.min(1.5, Math.max(0.55, Math.round(zoom / safeStep) * safeStep));
}

function arraysEqual(left: string[], right: string[]) {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

function setIfChanged(setter: React.Dispatch<React.SetStateAction<string[]>>, nextValues: string[]) {
  setter((currentValues) => (arraysEqual(currentValues, nextValues) ? currentValues : nextValues));
}

function formatDefaultProjectFilename(date = new Date()) {
  const monthNames = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
  const day = String(date.getDate()).padStart(2, "0");
  const month = monthNames[date.getMonth()] ?? "jan";
  return `ctrl-lab-${day}-${month}.json`;
}

function formatBlockKind(blockType: BlockType) {
  return blockCatalog[blockType].role.toLowerCase();
}

function formatDeletionStatus(nodeCount: number, edgeCount: number) {
  if (nodeCount > 0 && edgeCount > 0) {
    return `Deleted ${nodeCount} block${nodeCount === 1 ? "" : "s"} and ${edgeCount} connection${edgeCount === 1 ? "" : "s"}`;
  }

  if (nodeCount > 0) {
    return `Deleted ${nodeCount} block${nodeCount === 1 ? "" : "s"}`;
  }

  return `Deleted ${edgeCount} connection${edgeCount === 1 ? "" : "s"}`;
}

function evaluateEquation(equation: string, a: number, b: number) {
  const { leftOperator, rightOperator } = parseEquationTokens(equation);
  const combination = `${leftOperator} ${rightOperator}`;

  switch (combination) {
    case "+ +":
      return a + b;
    case "+ -":
      return a - b;
    case "- +":
      return b - a;
    case "* *":
      return a * b;
    case "* /":
      return divideSafely(a, b);
    case "/ *":
      return divideSafely(b, a);
    default:
      return a + b;
  }
}

function buildNodeDetail(
  blockType: BlockType,
  properties: Record<string, string>,
  signalValue: number | null = null,
) {
  switch (blockType) {
    case "constant":
      return `Value ${properties.value}`;
    case "integrator":
      return `Initial ${properties.initialValue}`;
    case "transferFunction":
      return properties.stateName?.trim() ? `State ${properties.stateName.trim()}` : "Transfer function";
    case "squareWave":
      return `${properties.frequency} Hz / ${properties.duty}%`;
    case "sum":
      return normalizeEquation(properties.equation);
    case "scope":
      return `${properties.channel} / ${properties.timebase}`;
    case "display":
      return formatSignalValue(signalValue, properties.decimals, properties.unit);
    default:
      return "";
  }
}

function getNextRoleTag(nodes: Pick<CanvasNode, "data">[], blockType: BlockType) {
  const prefix = tagPrefixByType[blockType];
  const nextNumber =
    nodes.reduce((maxNumber, node) => {
      if (node.data.blockType !== blockType) {
        return maxNumber;
      }

      const match = node.data.role.match(/-(\d+)$/);
      const currentNumber = match ? Number.parseInt(match[1], 10) : 0;
      return Math.max(maxNumber, currentNumber);
    }, 0) + 1;

  return `${prefix}-${String(nextNumber).padStart(2, "0")}`;
}

function isBlockType(value: unknown): value is BlockType {
  return typeof value === "string" && value in blockCatalog;
}

function toCanvasNode(
  id: string,
  blockType: BlockType,
  x: number,
  y: number,
  role = blockCatalog[blockType].role,
): CanvasNode {
  const definition = blockCatalog[blockType];
  const properties = { ...definition.defaultProperties };

  return {
    id,
    type: "controlBlock",
    position: { x, y },
    data: {
      ...definition,
      blockType,
      role,
      properties,
      inputSignals: {},
      signalValue: null,
      detail: buildNodeDetail(blockType, properties),
    },
  };
}

function toSerializedNode(node: CanvasNode): SerializedNode {
  return {
    id: node.id,
    type: node.data.blockType,
    label: node.data.label,
    role: node.data.role,
    position: {
      x: node.position.x,
      y: node.position.y,
    },
    properties: { ...node.data.properties },
  };
}

function toSerializedEdge(edge: CanvasEdge): SerializedEdge {
  return {
    id: edge.id,
    sourceNodeId: edge.source,
    sourcePortId: edge.sourceHandle ?? null,
    targetNodeId: edge.target,
    targetPortId: edge.targetHandle ?? null,
  };
}

function fromSerializedNode(node: SerializedNode): CanvasNode {
  const definition = blockCatalog[node.type];
  const properties = {
    ...definition.defaultProperties,
    ...node.properties,
  };

  return {
    id: node.id,
    type: "controlBlock",
    position: {
      x: node.position.x,
      y: node.position.y,
    },
    data: {
      ...definition,
      blockType: node.type,
      label: node.label,
      role: node.role,
      properties,
      inputSignals: {},
      signalValue: null,
      detail: buildNodeDetail(node.type, properties),
    },
  };
}

function toCanvasEdge(edge: BlockGraph["edges"][number]): CanvasEdge {
  return {
    id: edge.id,
    source: edge.sourceNodeId,
    sourceHandle: edge.sourcePortId,
    target: edge.targetNodeId,
    targetHandle: edge.targetPortId,
    type: "controlEdge",
    markerEnd: defaultMarker,
  };
}

function fromSerializedEdge(edge: SerializedEdge): CanvasEdge {
  return {
    id: edge.id,
    source: edge.sourceNodeId,
    sourceHandle: edge.sourcePortId ?? undefined,
    target: edge.targetNodeId,
    targetHandle: edge.targetPortId ?? undefined,
    type: "controlEdge",
    markerEnd: defaultMarker,
  };
}

function handleOffset(index: number, total: number, nodeHeight: number) {
  const handleSize = 12;
  const centerOffset = handleSize / 2;

  if (total <= 1) {
    return `${nodeHeight / 2 - centerOffset}px`;
  }

  if (total === 2) {
    const centers = [nodeHeight / 2 - gridSize, nodeHeight / 2 + gridSize];
    return `${centers[index] - centerOffset}px`;
  }

  const startCenter = nodeHeight / 2 - ((total - 1) * gridSize) / 2;
  return `${startCenter + index * gridSize - centerOffset}px`;
}

function buildProjectGraphIndex(nodes: CanvasNode[], edges: CanvasEdge[]): ProjectGraphIndex {
  const nodesById = Object.fromEntries(
    nodes.map((node) => [
      node.id,
      {
        type: node.data.blockType,
        role: node.data.role,
        inputPortIds: [...node.data.inputs],
        outputPortIds: [...node.data.outputs],
      },
    ]),
  );

  const incomingEdgesByNodeId = Object.fromEntries(nodes.map((node) => [node.id, [] as string[]]));
  const outgoingEdgesByNodeId = Object.fromEntries(nodes.map((node) => [node.id, [] as string[]]));

  for (const edge of edges) {
    if (incomingEdgesByNodeId[edge.target]) {
      incomingEdgesByNodeId[edge.target].push(edge.id);
    }

    if (outgoingEdgesByNodeId[edge.source]) {
      outgoingEdgesByNodeId[edge.source].push(edge.id);
    }
  }

  return {
    nodesById,
    incomingEdgesByNodeId,
    outgoingEdgesByNodeId,
  };
}

function getIncomingEdge(edges: CanvasEdge[], nodeId: string, handleId: string) {
  return edges.find((edge) => edge.target === nodeId && edge.targetHandle === handleId);
}

function evaluateSignalGraph(nodes: CanvasNode[], edges: CanvasEdge[], timeSeconds: number) {
  const nodeMap = new Map(nodes.map((node) => [node.id, node]));
  const cache = new Map<string, number | null>();

  function readNode(nodeId: string, stack = new Set<string>()): number | null {
    if (cache.has(nodeId)) {
      return cache.get(nodeId) ?? null;
    }

    if (stack.has(nodeId)) {
      return null;
    }

    const node = nodeMap.get(nodeId);

    if (!node) {
      return null;
    }

    const nextStack = new Set(stack);
    nextStack.add(nodeId);

    let result: number | null = null;

    switch (node.data.blockType) {
      case "constant":
        result = parseNumber(node.data.properties.value, 0);
        break;
      case "integrator": {
        const inputEdge = getIncomingEdge(edges, nodeId, "in");
        const inputValue = inputEdge ? readNode(inputEdge.source, nextStack) ?? 0 : 0;
        const initialValue = parseNumber(node.data.properties.initialValue, 0);

        result = initialValue + inputValue * timeSeconds;
        break;
      }
      case "transferFunction": {
        const inputEdge = getIncomingEdge(edges, nodeId, "in");
        result = inputEdge ? readNode(inputEdge.source, nextStack) : null;
        break;
      }
      case "squareWave": {
        const amplitude = parseNumber(node.data.properties.amplitude, 1);
        const frequency = Math.max(parseNumber(node.data.properties.frequency, 1), 0.001);
        const duty = Math.min(100, Math.max(0, parseNumber(node.data.properties.duty, 50)));
        const phase = (timeSeconds * frequency) % 1;

        result = phase < duty / 100 ? amplitude : 0;
        break;
      }
      case "sum": {
        const edgeA = getIncomingEdge(edges, nodeId, "a");
        const edgeB = getIncomingEdge(edges, nodeId, "b");
        const inputA = edgeA ? readNode(edgeA.source, nextStack) ?? 0 : 0;
        const inputB = edgeB ? readNode(edgeB.source, nextStack) ?? 0 : 0;

        result = evaluateEquation(node.data.properties.equation, inputA, inputB);
        break;
      }
      case "scope":
      case "display": {
        const inputEdge = getIncomingEdge(edges, nodeId, "in");
        result = inputEdge ? readNode(inputEdge.source, nextStack) : null;
        break;
      }
      default:
        result = null;
    }

    cache.set(nodeId, result);
    return result;
  }

  for (const node of nodes) {
    readNode(node.id);
  }

  return cache;
}

function getNextNodeNumber(nodes: SerializedNode[]) {
  return (
    nodes.reduce((maxNumber, node) => {
      const match = node.id.match(/-(\d+)$/);
      const currentNumber = match ? Number.parseInt(match[1], 10) : 0;

      return Math.max(maxNumber, currentNumber);
    }, 0) + 1
  );
}

function isProjectDocument(value: unknown): value is ProjectDocument {
  if (!value || typeof value !== "object") {
    return false;
  }

  const candidate = value as Partial<ProjectDocument>;

  return (
    candidate.kind === "ctrl-lab-project" &&
    candidate.version === 1 &&
    !!candidate.simulation &&
    typeof candidate.simulation.endTime === "number" &&
    typeof candidate.simulation.stepSize === "number" &&
    (candidate.simulation.zoomStep === undefined || typeof candidate.simulation.zoomStep === "number") &&
    (candidate.simulation.zoom === undefined || typeof candidate.simulation.zoom === "number") &&
    (candidate.simulation.viewportX === undefined || typeof candidate.simulation.viewportX === "number") &&
    (candidate.simulation.viewportY === undefined || typeof candidate.simulation.viewportY === "number") &&
    Array.isArray(candidate.nodes) &&
    Array.isArray(candidate.edges)
  );
}

function buildProjectDocument(
  nodes: CanvasNode[],
  edges: CanvasEdge[],
  endTime: number,
  stepSize: number,
  zoomStep: number,
  viewport: { x: number; y: number; zoom: number },
): ProjectDocument {
  return {
    version: 1,
    kind: "ctrl-lab-project",
    generatedAt: new Date().toISOString(),
    title: workspaceTitle,
    simulation: {
      endTime,
      stepSize,
      zoomStep,
      zoom: viewport.zoom,
      viewportX: viewport.x,
      viewportY: viewport.y,
    },
    nodes: nodes.map(toSerializedNode),
    edges: edges.map(toSerializedEdge),
    graphIndex: buildProjectGraphIndex(nodes, edges),
  };
}

function createCanvasState(graph: BlockGraph) {
  return {
    nodes: graph.nodes.map((node) => toCanvasNode(node.id, node.type, node.position.x, node.position.y)),
    edges: graph.edges.map(toCanvasEdge),
  };
}

function getNodeLiveData(
  node: CanvasNode,
  edges: CanvasEdge[],
  signalValues: Map<string, number | null>,
) {
  const signalValue = signalValues.get(node.id) ?? null;
  const inputSignals = Object.fromEntries(
    node.data.inputs.map((inputId) => {
      const incomingEdge = getIncomingEdge(edges, node.id, inputId);
      const inputSignal = incomingEdge ? signalValues.get(incomingEdge.source) ?? null : null;
      return [inputId, inputSignal];
    }),
  );

  return {
    inputSignals,
    signalValue,
    detail: buildNodeDetail(node.data.blockType, node.data.properties, signalValue),
  };
}

function ControlBlockNode({ id, data, selected }: NodeProps<CanvasNode>) {
  const { signalValues } = useContext(FlowSignalsContext);
  const signalValue = signalValues.get(id) ?? null;
  const detail = buildNodeDetail(data.blockType, data.properties, signalValue);
  const accentStyle = { "--node-accent": data.accent } as CSSProperties;
  const isSumNode = data.blockType === "sum";
  const isConstantNode = data.blockType === "constant";
  const isIntegratorNode = data.blockType === "integrator";
  const isTransferFunctionNode = data.blockType === "transferFunction";
  const isSquareWaveNode = data.blockType === "squareWave";
  const isCompactNode = isConstantNode || isIntegratorNode || isTransferFunctionNode;
  const nodeHeight =
    isSumNode || isConstantNode || isIntegratorNode || isTransferFunctionNode ? 96 : isSquareWaveNode ? 144 : 192;

  return (
    <article
      className={`flow-node flow-node--${data.blockType}${isCompactNode ? " flow-node--compact" : ""}${selected ? " is-selected" : ""}`}
      style={accentStyle}
      data-state-name={isTransferFunctionNode ? (data.properties.stateName ?? "").trim() : undefined}
    >
      {data.inputs.map((portId, index) => {
        const handlePosition = isSumNode && portId === "b" ? Position.Bottom : Position.Left;
        const handleStyle =
          isSumNode && portId === "b"
            ? { left: "42px" }
            : isSumNode
              ? { top: "42px" }
              : { top: handleOffset(index, data.inputs.length, nodeHeight) };

        return (
          <Handle
            key={`input-${portId}`}
            id={portId}
            type="target"
            position={handlePosition}
            className={`flow-node__handle${isCompactNode ? " flow-node__handle--compact" : ""}`}
            style={handleStyle}
          />
        );
      })}

      {data.outputs.map((portId, index) => (
        <Handle
          key={`output-${portId}`}
          id={portId}
          type="source"
          position={Position.Right}
          className={`flow-node__handle${isCompactNode ? " flow-node__handle--compact" : ""}`}
          style={isSumNode ? { top: "42px" } : { top: handleOffset(index, data.outputs.length, nodeHeight) }}
        />
      ))}

      <BlockNodeBody data={data} signalValue={signalValue} detail={detail} />
    </article>
  );
}

function BlockNodeBody({
  data,
  signalValue = null,
  detail,
}: {
  data: CanvasNodeData;
  signalValue?: number | null;
  detail?: string;
}) {
  const displayReadout =
    data.blockType === "display"
      ? formatSignalValue(signalValue, data.properties.decimals, data.properties.unit)
      : null;
  const isSumNode = data.blockType === "sum";
  const isConstantNode = data.blockType === "constant";
  const isIntegratorNode = data.blockType === "integrator";
  const isTransferFunctionNode = data.blockType === "transferFunction";
  const equationTokens = isSumNode ? parseEquationTokens(data.properties.equation) : null;

  if (isSumNode) {
    return (
      <div className="flow-node__sum-layout">
        <div className="flow-node__sum-slot flow-node__sum-slot--left">
          <strong>{equationTokens?.leftOperator}</strong>
        </div>
        <div className="flow-node__sum-slot flow-node__sum-slot--bottom">
          <strong>{equationTokens?.rightOperator}</strong>
        </div>
        <div className="flow-node__sum-slot flow-node__sum-slot--right">
          <strong>=</strong>
        </div>
      </div>
    );
  }

  if (isConstantNode) {
    return <div className="flow-node__constant-value">{formatConstantValue(data.properties.value)}</div>;
  }

  if (isIntegratorNode) {
    return <div className="flow-node__integrator-value">INT</div>;
  }

  if (isTransferFunctionNode) {
    return <div className="flow-node__transfer-function-value">TF</div>;
  }

  return (
    <>
      <h3>{data.label}</h3>
      {displayReadout ? <div className="flow-node__display">{displayReadout}</div> : null}
      <div className="flow-node__meta">
        <span>{detail ?? data.detail}</span>
      </div>
    </>
  );
}

function ControlEdge({
  id,
  sourceX,
  sourceY,
  sourcePosition,
  targetX,
  targetY,
  targetPosition,
  targetHandleId,
  markerEnd,
}: EdgeProps) {
  const [edgePath] = getSmoothStepPath({
    sourceX,
    sourceY,
    sourcePosition,
    targetX,
    targetY,
    targetPosition,
    borderRadius: 0,
    offset: targetHandleId === "b" ? 56 : 24,
  });

  return <BaseEdge id={id} path={edgePath} markerEnd={markerEnd} />;
}

const nodeTypes = {
  controlBlock: ControlBlockNode,
};

const edgeTypes = {
  controlEdge: ControlEdge,
};

function buildPreviewNodeData(blockType: BlockType): CanvasNodeData {
  const definition = blockCatalog[blockType];
  const properties = { ...definition.defaultProperties };

  return {
    ...definition,
    blockType,
    detail: buildNodeDetail(blockType, properties),
    properties,
    inputSignals: {},
    signalValue: null,
  };
}

type AppErrorBoundaryState = {
  errorMessage: string | null;
};

class AppErrorBoundary extends Component<{ children: ReactNode }, AppErrorBoundaryState> {
  state: AppErrorBoundaryState = {
    errorMessage: null,
  };

  static getDerivedStateFromError(error: unknown): AppErrorBoundaryState {
    const message = error instanceof Error ? error.stack ?? error.message : String(error);
    return { errorMessage: message };
  }

  componentDidCatch(error: unknown) {
    console.error("ctrl-lab runtime error", error);
  }

  render() {
    if (this.state.errorMessage) {
      return (
        <main className="app-crash-screen">
          <section className="app-crash-screen__panel">
            <strong>Runtime Error</strong>
            <pre>{this.state.errorMessage}</pre>
          </section>
        </main>
      );
    }

    return this.props.children;
  }
}

function ControlRoom() {
  const initialCanvas = createCanvasState(emptyGraph);
  const [nodes, setNodes] = useNodesState(initialCanvas.nodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState(initialCanvas.edges);
  const [inspectorNodeId, setInspectorNodeId] = useState<string | null>(null);
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [selectedEdgeId, setSelectedEdgeId] = useState<string | null>(null);
  const [selectedNodeIds, setSelectedNodeIds] = useState<string[]>([]);
  const [selectedEdgeIds, setSelectedEdgeIds] = useState<string[]>([]);
  const [endTime, setEndTime] = useState("10");
  const [stepSize, setStepSize] = useState("0.1");
  const [simulationStatus, setSimulationStatus] = useState("Idle");
  const [projectFilePath, setProjectFilePath] = useState<string | null>(null);
  const [recentProjects, setRecentProjects] = useState<RecentProject[]>(() => {
    if (typeof window === "undefined") {
      return [];
    }

    try {
      const stored = window.localStorage.getItem(recentProjectsStorageKey);
      if (!stored) {
        return [];
      }

      const parsed = JSON.parse(stored) as unknown;
      if (!Array.isArray(parsed)) {
        return [];
      }

      return parsed.filter(
        (entry): entry is RecentProject =>
          !!entry &&
          typeof entry === "object" &&
          typeof (entry as RecentProject).path === "string" &&
          typeof (entry as RecentProject).name === "string" &&
          typeof (entry as RecentProject).openedAt === "string",
      );
    } catch {
      return [];
    }
  });
  const [isHomeVisible, setIsHomeVisible] = useState(true);
  const [activeMenu, setActiveMenu] = useState<"file" | "settings" | null>(null);
  const [isSimulationRunning, setIsSimulationRunning] = useState(false);
  const [timeSeconds, setTimeSeconds] = useState(0);
  const [zoomLevel, setZoomLevel] = useState(1);
  const [zoomStep, setZoomStep] = useState(0.05);
  const [inputErrorMessage, setInputErrorMessage] = useState<string | null>(null);
  const [compileReport, setCompileReport] = useState<string | null>(null);
  const [rackDrag, setRackDrag] = useState<RackDragState | null>(null);
  const rackDragRef = useRef<RackDragState | null>(null);
  const rackDragHandledRef = useRef(false);
  const historyRef = useRef<EditorSnapshot[]>([]);
  const isRestoringHistoryRef = useRef(false);
  const simulationStartTimeRef = useRef<number | null>(null);
  const nextNodeNumber = useRef(1);
  const fileMenuRef = useRef<HTMLDivElement | null>(null);
  const canvasSurfaceRef = useRef<HTMLDivElement | null>(null);
  const createNodeRef = useRef<(blockType: BlockType, clientX: number, clientY: number) => void>(() => undefined);
  const dragDuplicateRef = useRef<{
    nodeId: string;
    startPosition: { x: number; y: number };
    sourceRole: string;
    duplicateId: string | null;
  } | null>(null);
  const suppressedDuplicateReleaseRef = useRef<{
    originalNodeId: string;
    originalPosition: { x: number; y: number };
    duplicateNodeId: string;
    duplicatePosition: { x: number; y: number };
  } | null>(null);
  const viewportRef = useRef(defaultViewport);
  const pendingViewportRef = useRef<typeof defaultViewport | null>(defaultViewport);
  const { getViewport, screenToFlowPosition, setViewport } = useReactFlow();

  useEffect(() => {
    if (!isSimulationRunning) {
      simulationStartTimeRef.current = null;
      return;
    }

    if (simulationStartTimeRef.current === null) {
      simulationStartTimeRef.current = Date.now() - timeSeconds * 1000;
    }

    const intervalId = window.setInterval(() => {
      const startedAt = simulationStartTimeRef.current ?? Date.now();
      setTimeSeconds((Date.now() - startedAt) / 1000);
    }, 200);

    return () => window.clearInterval(intervalId);
  }, [isSimulationRunning]);

  useEffect(() => {
    try {
      window.localStorage.setItem(recentProjectsStorageKey, JSON.stringify(recentProjects));
    } catch {
      // Ignore local persistence failures.
    }
  }, [recentProjects]);

  useEffect(() => {
    if (!activeMenu) {
      return;
    }

    function handlePointerDown(event: PointerEvent) {
      if (
        fileMenuRef.current &&
        event.target instanceof Node &&
        !fileMenuRef.current.contains(event.target)
      ) {
        setActiveMenu(null);
      }
    }

    function handleWindowKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setActiveMenu(null);
      }
    }

    window.addEventListener("pointerdown", handlePointerDown);
    window.addEventListener("keydown", handleWindowKeyDown);

    return () => {
      window.removeEventListener("pointerdown", handlePointerDown);
      window.removeEventListener("keydown", handleWindowKeyDown);
    };
  }, [activeMenu]);

  const signalValues = useMemo(
    () => evaluateSignalGraph(nodes as CanvasNode[], edges, timeSeconds),
    [nodes, edges, timeSeconds],
  );
  const signalContextValue = useMemo(
    () => ({ signalValues, edges }),
    [signalValues, edges],
  );
  const inspectorNode = (nodes as CanvasNode[]).find((node) => node.id === inspectorNodeId) ?? null;
  const inspectorLiveData = inspectorNode ? getNodeLiveData(inspectorNode, edges, signalValues) : null;

  useEffect(() => {
    if (!pendingViewportRef.current) {
      return;
    }

    const targetViewport = pendingViewportRef.current;
    const frameId = window.requestAnimationFrame(() => {
      viewportRef.current = targetViewport;
      void setViewport(targetViewport);
      pendingViewportRef.current = null;
    });

    return () => window.cancelAnimationFrame(frameId);
  }, [nodes, edges, setViewport]);

  function syncSelection(nextNodeIds: string[], nextEdgeIds: string[]) {
    setIfChanged(setSelectedNodeIds, nextNodeIds);
    setIfChanged(setSelectedEdgeIds, nextEdgeIds);
    setSelectedNodeId(nextNodeIds[0] ?? null);
    setSelectedEdgeId(nextEdgeIds[0] ?? null);
  }

  function handleNodesChange(changes: NodeChange<CanvasNode>[]) {
    const suppressedDuplicateRelease = suppressedDuplicateReleaseRef.current;
    const nextChanges = suppressedDuplicateRelease
      ? changes.map((change) =>
          change.type === "position"
            ? change.id === suppressedDuplicateRelease.originalNodeId
              ? {
                  ...change,
                  position: suppressedDuplicateRelease.originalPosition,
                  dragging: false,
                }
              : change.id === suppressedDuplicateRelease.duplicateNodeId
                ? {
                    ...change,
                    position: suppressedDuplicateRelease.duplicatePosition,
                    dragging: false,
                  }
                : change
            : change,
        )
      : changes;

    setNodes((currentNodes) => applyNodeChanges(nextChanges, currentNodes));
  }

  function createSnapshot(): EditorSnapshot {
    const viewport = viewportRef.current;
    return {
      nodes: (nodes as CanvasNode[]).map(toSerializedNode),
      edges: edges.map(toSerializedEdge),
      endTime,
      stepSize,
      inspectorNodeId,
      selectedNodeId,
      selectedEdgeId,
      nextNodeNumber: nextNodeNumber.current,
      projectFilePath,
      isSimulationRunning,
      timeSeconds,
      zoomStep,
      zoomLevel: viewport.zoom,
      viewportX: viewport.x,
      viewportY: viewport.y,
    };
  }

  function restoreSnapshot(snapshot: EditorSnapshot) {
    isRestoringHistoryRef.current = true;
    setNodes(snapshot.nodes.map((node) => fromSerializedNode(node)));
    setEdges(snapshot.edges.map((edge) => fromSerializedEdge(edge)));
    setEndTime(snapshot.endTime);
    setStepSize(snapshot.stepSize);
    setInspectorNodeId(snapshot.inspectorNodeId);
    setSelectedNodeId(snapshot.selectedNodeId);
    setSelectedEdgeId(snapshot.selectedEdgeId);
    setSelectedNodeIds(snapshot.selectedNodeId ? [snapshot.selectedNodeId] : []);
    setSelectedEdgeIds(snapshot.selectedEdgeId ? [snapshot.selectedEdgeId] : []);
    nextNodeNumber.current = snapshot.nextNodeNumber;
    setProjectFilePath(snapshot.projectFilePath);
    setIsSimulationRunning(snapshot.isSimulationRunning);
    setTimeSeconds(snapshot.timeSeconds);
    setZoomStep(snapshot.zoomStep);
    setZoomLevel(snapshot.zoomLevel);
    pendingViewportRef.current = {
      x: snapshot.viewportX,
      y: snapshot.viewportY,
      zoom: snapshot.zoomLevel,
    };
    window.setTimeout(() => {
      isRestoringHistoryRef.current = false;
    }, 0);
  }

  function pushHistorySnapshot() {
    if (isRestoringHistoryRef.current) {
      return;
    }

    historyRef.current.push(createSnapshot());
    if (historyRef.current.length > 100) {
      historyRef.current.shift();
    }
  }

  function undoLastChange() {
    const snapshot = historyRef.current.pop();
    if (!snapshot) {
      return;
    }

    restoreSnapshot(snapshot);
    setSimulationStatus("Undid last change");
  }

  function resetWorkspace(graph: BlockGraph = emptyGraph) {
    const nextCanvas = createCanvasState(graph);

    setNodes(nextCanvas.nodes);
    setEdges(nextCanvas.edges);
    setInspectorNodeId(null);
    setSelectedNodeId(null);
    setSelectedEdgeId(null);
    setSelectedNodeIds([]);
    setSelectedEdgeIds([]);
    dragDuplicateRef.current = null;
    nextNodeNumber.current = getNextNodeNumber(nextCanvas.nodes.map(toSerializedNode));
    setIsSimulationRunning(false);
    setTimeSeconds(0);
    setZoomStep(0.05);
    setZoomLevel(defaultViewport.zoom);
    viewportRef.current = defaultViewport;
    pendingViewportRef.current = defaultViewport;
  }

  function restoreProject(project: ProjectDocument) {
    const restoredNodes = project.nodes
      .filter((node) => isBlockType(node.type))
      .map((node) => fromSerializedNode(node));
    const restoredEdges = project.edges.map((edge) => fromSerializedEdge(edge));

    setNodes(restoredNodes);
    setEdges(restoredEdges);
    setEndTime(String(project.simulation.endTime));
    setStepSize(String(project.simulation.stepSize));
    setInspectorNodeId(null);
    setSelectedNodeId(null);
    setSelectedEdgeId(null);
    setSelectedNodeIds([]);
    setSelectedEdgeIds([]);
    dragDuplicateRef.current = null;
    nextNodeNumber.current = getNextNodeNumber(project.nodes);
    setProjectFilePath(null);
    setIsSimulationRunning(false);
    setTimeSeconds(0);
    setZoomStep(project.simulation.zoomStep ?? 0.05);
    const restoredZoom = clampZoomLevel(project.simulation.zoom ?? defaultViewport.zoom, project.simulation.zoomStep ?? 0.05);
    const restoredViewport = {
      x: project.simulation.viewportX ?? defaultViewport.x,
      y: project.simulation.viewportY ?? defaultViewport.y,
      zoom: restoredZoom,
    };
    setZoomLevel(restoredZoom);
    viewportRef.current = restoredViewport;
    pendingViewportRef.current = restoredViewport;
  }

  function getProjectName(projectPath: string) {
    return projectPath.split(/[/\\]/).pop() ?? projectPath;
  }

  function rememberRecentProject(projectPath: string) {
    const nextEntry: RecentProject = {
      path: projectPath,
      name: getProjectName(projectPath),
      openedAt: new Date().toISOString(),
    };

    setRecentProjects((currentProjects) => {
      const dedupedProjects = currentProjects.filter((project) => project.path !== projectPath);
      return [nextEntry, ...dedupedProjects].slice(0, 8);
    });
  }

  function createNode(blockType: BlockType, clientX: number, clientY: number) {
    pushHistorySnapshot();
    const position = screenToFlowPosition({ x: clientX, y: clientY });
    const x = Math.round((position.x - 92) / gridSize) * gridSize;
    const y = Math.round((position.y - 54) / gridSize) * gridSize;
    const id = `${blockType}-${nextNodeNumber.current}`;
    const role = getNextRoleTag(nodes as CanvasNode[], blockType);

    nextNodeNumber.current += 1;

    const nextNode = toCanvasNode(id, blockType, x, y, role);

    setNodes((currentNodes) => currentNodes.concat(nextNode));
    syncSelection([id], []);
    setInspectorNodeId(id);
    setSimulationStatus(`Inserted ${formatBlockKind(blockType)}`);
  }

  createNodeRef.current = createNode;

  useEffect(() => {
    if (!rackDrag) {
      return;
    }

    function handlePointerMove(event: PointerEvent) {
      const currentDrag = rackDragRef.current;
      if (!currentDrag) {
        return;
      }
      const nextDrag = { ...currentDrag, x: event.clientX, y: event.clientY };
      rackDragRef.current = nextDrag;
      setRackDrag(nextDrag);
    }

    function finishDrag(clientX: number, clientY: number) {
      if (rackDragHandledRef.current) {
        return;
      }
      rackDragHandledRef.current = true;

      const currentDrag = rackDragRef.current;
      rackDragRef.current = null;
      setRackDrag(null);

      if (!currentDrag) {
        return;
      }

      const canvasBounds = canvasSurfaceRef.current?.getBoundingClientRect();
      if (
        canvasBounds &&
        clientX >= canvasBounds.left &&
        clientX <= canvasBounds.right &&
        clientY >= canvasBounds.top &&
        clientY <= canvasBounds.bottom
      ) {
        createNodeRef.current(currentDrag.blockType, clientX, clientY);
      }
    }

    function handlePointerUp(event: PointerEvent) {
      finishDrag(event.clientX, event.clientY);
    }

    function handlePointerCancel() {
      rackDragHandledRef.current = true;
      rackDragRef.current = null;
      setRackDrag(null);
    }

    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    window.addEventListener("pointercancel", handlePointerCancel);

    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
      window.removeEventListener("pointercancel", handlePointerCancel);
    };
  }, [rackDrag]);

  function updateInspectorNode(updater: (node: CanvasNode) => CanvasNode) {
    if (!inspectorNodeId) {
      return;
    }

    pushHistorySnapshot();
    setNodes((currentNodes) =>
      currentNodes.map((node) => (node.id === inspectorNodeId ? updater(node as CanvasNode) : node)),
    );
  }

  function handleConnect(connection: Connection) {
    pushHistorySnapshot();
    const sourceNode = (nodes as CanvasNode[]).find((node) => node.id === connection.source);
    const targetNode = (nodes as CanvasNode[]).find((node) => node.id === connection.target);
    setEdges((currentEdges) =>
      addEdge(
        {
          ...connection,
          type: "controlEdge",
          markerEnd: defaultMarker,
        },
        currentEdges,
      ),
    );
    if (sourceNode && targetNode) {
      setSimulationStatus(`Connected ${sourceNode.data.role} to ${targetNode.data.role}`);
    } else {
      setSimulationStatus("Connected blocks");
    }
  }

  function handleRackPointerDown(event: React.PointerEvent<HTMLButtonElement>, blockType: BlockType) {
    if (event.button !== 0) {
      return;
    }

    event.preventDefault();
    rackDragHandledRef.current = false;
    const nextDrag = { blockType, x: event.clientX, y: event.clientY };
    rackDragRef.current = nextDrag;
    setRackDrag(nextDrag);
  }

  function showDecimalSeparatorError() {
    setInputErrorMessage("Use periods for decimals, for example 1.0 instead of 1,0.");
    setSimulationStatus("Invalid decimal separator");
  }

  function handleTopBarDecimalChange(
    setter: React.Dispatch<React.SetStateAction<string>>,
    value: string,
  ) {
    if (containsDecimalComma(value)) {
      showDecimalSeparatorError();
      return;
    }

    setter(value);
  }

  function handlePropertyChange(field: PropertyField, value: string) {
    if (field.inputMode === "decimal" && containsDecimalComma(value)) {
      showDecimalSeparatorError();
      return;
    }

    updateInspectorNode((node) => {
      const properties = {
        ...node.data.properties,
        [field.key]: value,
      };

      return {
        ...node,
        data: {
          ...node.data,
          properties,
        },
      };
    });
  }

  function handleNewProject() {
    historyRef.current = [];
    resetWorkspace();
    setEndTime("10");
    setStepSize("0.1");
    setProjectFilePath(null);
    setIsHomeVisible(false);
    setSimulationStatus("Started a new blank project");
  }

  function handleStartSimulation() {
    if (containsDecimalComma(endTime) || containsDecimalComma(stepSize)) {
      showDecimalSeparatorError();
      return;
    }

    const parsedEndTime = parseNumber(endTime, Number.NaN);
    const parsedStepSize = parseNumber(stepSize, Number.NaN);

    if (!Number.isFinite(parsedEndTime) || parsedEndTime <= 0) {
      setSimulationStatus("End time must be greater than zero");
      return;
    }

    if (!Number.isFinite(parsedStepSize) || parsedStepSize <= 0) {
      setSimulationStatus("Step size must be greater than zero");
      return;
    }

    if (isSimulationRunning) {
      setIsSimulationRunning(false);
      setSimulationStatus("Simulation stopped");
      return;
    }

    setTimeSeconds(0);
    setIsSimulationRunning(true);
    setSimulationStatus("Simulation running");
  }

  async function handleCompileProject() {
    try {
      if (containsDecimalComma(endTime) || containsDecimalComma(stepSize)) {
        showDecimalSeparatorError();
        return;
      }

      const hasCommaInProperties = (nodes as CanvasNode[]).some((node) =>
        node.data.propertyFields.some(
          (field) => field.inputMode === "decimal" && containsDecimalComma(node.data.properties[field.key]),
        ),
      );

      if (hasCommaInProperties) {
        showDecimalSeparatorError();
        return;
      }

      const parsedEndTime = parseNumber(endTime, 10);
      const parsedStepSize = parseNumber(stepSize, 0.1);
      const projectDocument = buildProjectDocument(
        nodes as CanvasNode[],
        edges,
        parsedEndTime,
        parsedStepSize,
        zoomStep,
        viewportRef.current,
      );
      setSimulationStatus("Compiling project");
      const report = await invoke<string>("compile_project_report", {
        projectJson: JSON.stringify(projectDocument, null, 2),
      });
      setCompileReport(report);
      setSimulationStatus("Compile completed");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setCompileReport(message);
      setSimulationStatus("Compile failed");
    }
  }

  async function handleSaveProject(saveAs = false) {
    try {
      if (containsDecimalComma(endTime) || containsDecimalComma(stepSize)) {
        showDecimalSeparatorError();
        return;
      }

      const hasCommaInProperties = (nodes as CanvasNode[]).some((node) =>
        node.data.propertyFields.some(
          (field) => field.inputMode === "decimal" && containsDecimalComma(node.data.properties[field.key]),
        ),
      );

      if (hasCommaInProperties) {
        showDecimalSeparatorError();
        return;
      }

      const needsPathSelection = saveAs || !projectFilePath;
      const parsedEndTime = parseNumber(endTime, 10);
      const parsedStepSize = parseNumber(stepSize, 0.1);
      const viewport = viewportRef.current;
      const projectDocument = buildProjectDocument(
        nodes as CanvasNode[],
        edges,
        parsedEndTime,
        parsedStepSize,
        zoomStep,
        viewport,
      );
      const suggestedPath = projectFilePath ?? formatDefaultProjectFilename();
      const targetPath = needsPathSelection
        ? await save({
            title: "Save ctrl-lab project",
            defaultPath: suggestedPath,
            filters: [
              {
                name: "ctrl-lab project",
                extensions: ["json"],
              },
            ],
          })
        : projectFilePath;

      if (!targetPath) {
        setSimulationStatus(saveAs || !projectFilePath ? "Save cancelled" : "Unable to save the project");
        return;
      }

      await writeTextFile(targetPath, JSON.stringify(projectDocument, null, 2));
      setProjectFilePath(targetPath);
      rememberRecentProject(targetPath);
      setSimulationStatus(`Saved project to ${targetPath}`);
    } catch {
      setSimulationStatus("Unable to save the project");
    }
  }

  async function openProjectAtPath(selectedPath: string) {
    const contents = await readTextFile(selectedPath);
    const parsed = JSON.parse(contents) as unknown;

    if (!isProjectDocument(parsed)) {
      throw new Error("invalid-project");
    }

    historyRef.current = [];
    restoreProject(parsed);
    setProjectFilePath(selectedPath);
    setIsHomeVisible(false);
    rememberRecentProject(selectedPath);
    setSimulationStatus(`Opened ${selectedPath}`);
  }

  async function handleOpenProject() {
    try {
      const selectedPath = await open({
        title: "Open ctrl-lab project",
        multiple: false,
        directory: false,
        filters: [
          {
            name: "ctrl-lab project",
            extensions: ["json"],
          },
        ],
      });

      if (!selectedPath || Array.isArray(selectedPath)) {
        setSimulationStatus("Open cancelled");
        return;
      }

      await openProjectAtPath(selectedPath);
    } catch {
      setSimulationStatus("Unable to open that file");
    }
  }

  async function handleOpenRecentProject(projectPath: string) {
    setActiveMenu(null);

    try {
      await openProjectAtPath(projectPath);
    } catch {
      setSimulationStatus(`Unable to open recent project: ${projectPath}`);
    }
  }

  function handleCloseProject() {
    historyRef.current = [];
    resetWorkspace();
    setEndTime("10");
    setStepSize("0.1");
    setProjectFilePath(null);
    setIsHomeVisible(true);
    setSimulationStatus("Closed current project");
  }

  function handleFileMenuAction(action: "close" | "new" | "open" | "save" | "saveAs") {
    setActiveMenu(null);

    if (action === "close") {
      handleCloseProject();
      return;
    }

    if (action === "new") {
      handleNewProject();
      return;
    }

    if (action === "open") {
      void handleOpenProject();
      return;
    }

    if (action === "save") {
      void handleSaveProject();
      return;
    }

    void handleSaveProject(true);
  }

  function deleteNodeById(nodeId: string) {
    pushHistorySnapshot();
    const deletedNode = (nodes as CanvasNode[]).find((node) => node.id === nodeId);
    const relatedEdgeCount = edges.filter((edge) => edge.source === nodeId || edge.target === nodeId).length;
    setNodes((currentNodes) => currentNodes.filter((node) => node.id !== nodeId));
    setEdges((currentEdges) => currentEdges.filter((edge) => edge.source !== nodeId && edge.target !== nodeId));
    setSelectedNodeId((currentId) => (currentId === nodeId ? null : currentId));
    setSelectedNodeIds((currentIds) => currentIds.filter((currentId) => currentId !== nodeId));
    setSelectedEdgeId(null);
    setSelectedEdgeIds([]);
    setInspectorNodeId((currentId) => (currentId === nodeId ? null : currentId));
    setSimulationStatus(
      relatedEdgeCount > 0
        ? formatDeletionStatus(1, relatedEdgeCount)
        : `Deleted ${deletedNode ? deletedNode.data.role : "block"}`,
    );
  }

  function deleteSelection(nodeIds: string[], edgeIds: string[]) {
    if (nodeIds.length === 0 && edgeIds.length === 0) {
      return;
    }

    pushHistorySnapshot();
    const nodeIdSet = new Set(nodeIds);
    const edgeIdSet = new Set(edgeIds);
    const linkedEdgeCount = edges.filter(
      (edge) => nodeIdSet.has(edge.source) || nodeIdSet.has(edge.target),
    ).length;
    const edgeTotal = Math.max(edgeIds.length, linkedEdgeCount + edgeIds.filter((edgeId) => !linkedEdgeCount || !edgeIdSet.has(edgeId)).length);

    setNodes((currentNodes) => currentNodes.filter((node) => !nodeIdSet.has(node.id)));
    setEdges((currentEdges) =>
      currentEdges.filter(
        (edge) => !edgeIdSet.has(edge.id) && !nodeIdSet.has(edge.source) && !nodeIdSet.has(edge.target),
      ),
    );
    setSelectedNodeId(null);
    setSelectedEdgeId(null);
    setSelectedNodeIds([]);
    setSelectedEdgeIds([]);
    setInspectorNodeId((currentId) => (currentId && nodeIdSet.has(currentId) ? null : currentId));
    setSimulationStatus(formatDeletionStatus(nodeIds.length, edgeTotal));
  }

  function handleDeleteInspectorNode() {
    if (!inspectorNodeId) {
      return;
    }

    deleteNodeById(inspectorNodeId);
  }

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      const target = event.target;

      if (
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement ||
        (target instanceof HTMLElement && target.isContentEditable)
      ) {
        return;
      }

      if ((event.ctrlKey || event.metaKey) && !event.shiftKey && event.key.toLowerCase() === "z") {
        event.preventDefault();
        undoLastChange();
        return;
      }

      if (event.key !== "Delete") {
        return;
      }

      if (selectedNodeIds.length > 0 || selectedEdgeIds.length > 0) {
        event.preventDefault();
        deleteSelection(selectedNodeIds, selectedEdgeIds);
      }
    }

    window.addEventListener("keydown", handleKeyDown);

    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [selectedEdgeIds, selectedNodeIds]);

  function handleSelectionChange(params: { nodes: Node[]; edges: Edge[] }) {
    const nextNodeIds = params.nodes.map((node) => node.id);
    const nextEdgeIds = params.edges.map((edge) => edge.id);
    syncSelection(nextNodeIds, nextEdgeIds);
    if (nextNodeIds.length > 0) {
      setInspectorNodeId(nextNodeIds[0]);
      return;
    }
    if (nextEdgeIds.length > 0) {
      setInspectorNodeId(null);
    }
  }

  function handleNodeDragStart(event: MouseEvent | React.MouseEvent, node: Node) {
    suppressedDuplicateReleaseRef.current = null;
    if (!(event.ctrlKey || event.metaKey)) {
      dragDuplicateRef.current = null;
      return;
    }

    const sourceNode = node as CanvasNode;
    dragDuplicateRef.current = {
      nodeId: sourceNode.id,
      startPosition: { ...sourceNode.position },
      sourceRole: sourceNode.data.role,
      duplicateId: null,
    };
  }

  function handleNodeDrag(_: MouseEvent | React.MouseEvent, node: Node) {
    const dragState = dragDuplicateRef.current;
    if (!dragState || dragState.nodeId !== node.id) {
      return;
    }

    const draggedNode = node as CanvasNode;
    const movedEnough =
      Math.abs(draggedNode.position.x - dragState.startPosition.x) >= 1 ||
      Math.abs(draggedNode.position.y - dragState.startPosition.y) >= 1;

    if (!movedEnough) {
      return;
    }

    if (!dragState.duplicateId) {
      pushHistorySnapshot();
      const sourceNode = (nodes as CanvasNode[]).find((entry) => entry.id === draggedNode.id);
      if (!sourceNode) {
        return;
      }

      const duplicateId = `${sourceNode.data.blockType}-${nextNodeNumber.current}`;
      nextNodeNumber.current += 1;
      const duplicateRole = getNextRoleTag(nodes as CanvasNode[], sourceNode.data.blockType);
      const duplicateNode: CanvasNode = {
        ...sourceNode,
        id: duplicateId,
        position: { ...draggedNode.position },
        selected: false,
        data: {
          ...sourceNode.data,
          role: duplicateRole,
          properties: { ...sourceNode.data.properties },
        },
      };

      dragDuplicateRef.current = {
        ...dragState,
        duplicateId,
      };
      setNodes((currentNodes) =>
        currentNodes.map((currentNode) =>
          currentNode.id === draggedNode.id
            ? { ...currentNode, position: { ...dragState.startPosition } }
            : currentNode,
        ).concat(duplicateNode),
      );
      syncSelection([duplicateId], []);
      setInspectorNodeId(duplicateId);
      return;
    }

    setNodes((currentNodes) =>
      currentNodes.map((currentNode) => {
        if (currentNode.id === draggedNode.id) {
          return { ...currentNode, position: { ...dragState.startPosition } };
        }

        if (currentNode.id === dragState.duplicateId) {
          return { ...currentNode, position: { ...draggedNode.position } };
        }

        return currentNode;
      }),
    );
  }

  function handleNodeDragStop(_: MouseEvent | React.MouseEvent, node: Node) {
    const dragState = dragDuplicateRef.current;
    dragDuplicateRef.current = null;
    if (!dragState || dragState.nodeId !== node.id || !dragState.duplicateId) {
      return;
    }

    const droppedNode = node as CanvasNode;
    suppressedDuplicateReleaseRef.current = {
      originalNodeId: dragState.nodeId,
      originalPosition: dragState.startPosition,
      duplicateNodeId: dragState.duplicateId,
      duplicatePosition: { ...droppedNode.position },
    };
    setNodes((currentNodes) =>
      currentNodes.map((currentNode) => {
        if (currentNode.id === dragState.nodeId) {
          return { ...currentNode, position: { ...dragState.startPosition } };
        }

        if (currentNode.id === dragState.duplicateId) {
          return { ...currentNode, position: { ...droppedNode.position } };
        }

        return currentNode;
      }),
    );
    window.setTimeout(() => {
      suppressedDuplicateReleaseRef.current = null;
    }, 200);
    syncSelection([dragState.duplicateId], []);
    setInspectorNodeId(dragState.duplicateId);
    setSimulationStatus(`Duplicated ${dragState.sourceRole}`);
  }

  function snapZoomLevel(zoom: number) {
    return clampZoomLevel(zoom, zoomStep);
  }

  function handleZoomStep(direction: 1 | -1) {
    const nextZoom = snapZoomLevel(zoomLevel + direction * zoomStep);
    if (Math.abs(nextZoom - zoomLevel) <= 0.001) {
      return;
    }
    const currentViewport = getViewport();
    void setViewport({
      x: currentViewport.x,
      y: currentViewport.y,
      zoom: nextZoom,
    });
    viewportRef.current = {
      x: currentViewport.x,
      y: currentViewport.y,
      zoom: nextZoom,
    };
    setZoomLevel(nextZoom);
  }

  function handleCanvasWheel(event: React.WheelEvent<HTMLDivElement>) {
    event.preventDefault();

    const direction: 1 | -1 = event.deltaY < 0 ? 1 : -1;
    handleZoomStep(direction);
  }

  return (
    <main className="control-room">
      <section className="simulation-strip" aria-label="simulation controls">
        <div className="simulation-strip__section simulation-strip__section--editor">
          <div className="simulation-strip__brand">
            <span className="chrome-bar__badge">CTRL-LAB</span>
            {workspaceTitle ? <strong>{workspaceTitle}</strong> : null}
          </div>

          <nav className="simulation-strip__menu-bar" aria-label="application menu">
            <div className="simulation-strip__menu" ref={fileMenuRef}>
              <button
                type="button"
                className={`simulation-strip__menu-button${activeMenu === "file" ? " is-active" : ""}`}
                aria-haspopup="menu"
                aria-expanded={activeMenu === "file"}
                onClick={() => setActiveMenu((currentMenu) => (currentMenu === "file" ? null : "file"))}
              >
                File
              </button>

              {activeMenu === "file" ? (
                <div className="simulation-strip__menu-panel" role="menu" aria-label="file actions">
                  <button
                    type="button"
                    className="simulation-strip__menu-item"
                    role="menuitem"
                    onClick={() => handleFileMenuAction("close")}
                  >
                    Close Project
                  </button>
                  <button
                    type="button"
                    className="simulation-strip__menu-item"
                    role="menuitem"
                    onClick={() => handleFileMenuAction("new")}
                  >
                    New
                  </button>
                  <button
                    type="button"
                    className="simulation-strip__menu-item"
                    role="menuitem"
                    onClick={() => handleFileMenuAction("open")}
                  >
                    Open
                  </button>
                  <button
                    type="button"
                    className="simulation-strip__menu-item"
                    role="menuitem"
                    onClick={() => handleFileMenuAction("save")}
                  >
                    Save
                  </button>
                  <button
                    type="button"
                    className="simulation-strip__menu-item"
                    role="menuitem"
                    onClick={() => handleFileMenuAction("saveAs")}
                  >
                    Save As
                  </button>
                  {recentProjects.length > 0 ? <div className="simulation-strip__menu-divider" /> : null}
                  {recentProjects.length > 0 ? (
                    <div className="simulation-strip__menu-label">Open Recent</div>
                  ) : null}
                  {recentProjects.map((project) => (
                    <button
                      key={project.path}
                      type="button"
                      className="simulation-strip__menu-item simulation-strip__menu-item--recent"
                      role="menuitem"
                      onClick={() => void handleOpenRecentProject(project.path)}
                    >
                      <strong>{project.name}</strong>
                      <span>{project.path}</span>
                    </button>
                  ))}
                </div>
              ) : null}
            </div>

            <button type="button" className="simulation-strip__menu-button">
              Edit
            </button>
            <button type="button" className="simulation-strip__menu-button">
              View
            </button>
            <div className="simulation-strip__menu">
              <button
                type="button"
                className={`simulation-strip__menu-button${activeMenu === "settings" ? " is-active" : ""}`}
                aria-haspopup="menu"
                aria-expanded={activeMenu === "settings"}
                onClick={() => setActiveMenu((currentMenu) => (currentMenu === "settings" ? null : "settings"))}
              >
                Settings
              </button>

              {activeMenu === "settings" ? (
                <div className="simulation-strip__menu-panel" role="menu" aria-label="settings">
                  <div className="simulation-strip__menu-label">Locale Settings</div>
                  <div className="simulation-strip__menu-note" role="presentation">
                    <strong>Decimal Separator</strong>
                    <span>Prefer period `.` or comma `,`</span>
                    <em>WIP</em>
                  </div>
                </div>
              ) : null}
            </div>
          </nav>
        </div>

        <div className="simulation-strip__section simulation-strip__section--simulation">
          <div className="simulation-strip__simulation-group">
          <button type="button" className="simulation-strip__button" onClick={() => void handleCompileProject()}>
            Compile Project
          </button>
          <button type="button" className="simulation-strip__button" onClick={handleStartSimulation}>
            {isSimulationRunning ? "Stop Simulation" : "Start Simulation"}
          </button>

          <label className="simulation-strip__field">
            <span>End Time</span>
            <input
              type="text"
              inputMode="decimal"
              value={endTime}
              onChange={(event) => handleTopBarDecimalChange(setEndTime, event.target.value)}
            />
          </label>

          <label className="simulation-strip__field">
            <span>Step Size</span>
            <input
              type="text"
              inputMode="decimal"
              value={stepSize}
              onChange={(event) => handleTopBarDecimalChange(setStepSize, event.target.value)}
            />
          </label>
          </div>
        </div>
      </section>

      {isHomeVisible ? (
        <section className="workspace-home" aria-label="home">
          <article className="workspace-home__hero">
            <span className="workspace-home__eyebrow">CTRL-LAB</span>
            <h1>Open control-system projects and continue where you left off.</h1>
            <p>
              The project file already carries the node list, edge list, block ids, and input/output port
              mappings the backend needs to start graph execution.
            </p>
            <div className="workspace-home__actions">
              <button type="button" className="workspace-home__button" onClick={handleNewProject}>
                New Project
              </button>
              <button type="button" className="workspace-home__button" onClick={() => void handleOpenProject()}>
                Open Project
              </button>
            </div>
          </article>

          <article className="workspace-home__recent">
            <div className="panel__title">Recent Projects</div>
            {recentProjects.length > 0 ? (
              <div className="workspace-home__recent-list">
                {recentProjects.map((project) => (
                  <button
                    key={project.path}
                    type="button"
                    className="workspace-home__recent-item"
                    onClick={() => void handleOpenRecentProject(project.path)}
                  >
                    <strong>{project.name}</strong>
                    <span>{project.path}</span>
                  </button>
                ))}
              </div>
            ) : (
              <div className="workspace-home__empty">
                <strong>No recent projects yet</strong>
                <p>Open or save a project once and it will appear here for quick access.</p>
              </div>
            )}
          </article>
        </section>
      ) : (
      <section className="workspace-grid">
        <aside className="panel panel--left">
          <div className="panel__title">Block Rack</div>
          <div className="library-list">
            {starterBlocks.map((blockType) => {
              const block = blockCatalog[blockType];
              const detail = buildNodeDetail(blockType, block.defaultProperties);

              return (
                <button
                  key={blockType}
                  type="button"
                  className={`library-card library-card--${blockType}`}
                  onPointerDown={(event) => handleRackPointerDown(event, blockType)}
                >
                  <strong>{block.label}</strong>
                  <p>{detail}</p>
                </button>
              );
            })}
          </div>
        </aside>

        <section className="canvas-frame" aria-label="block diagram canvas">
          <div className="canvas-frame__header">
            <span className="panel__title">Process Canvas</span>
          </div>

          <div
            ref={canvasSurfaceRef}
            className="canvas-surface canvas-surface--flow"
            onWheel={handleCanvasWheel}
          >
            <FlowSignalsContext.Provider value={signalContextValue}>
            <ReactFlow
              nodes={nodes}
              edges={edges}
              nodeTypes={nodeTypes}
              edgeTypes={edgeTypes}
              onNodesChange={handleNodesChange}
              onEdgesChange={onEdgesChange}
              onConnect={handleConnect}
              onSelectionChange={handleSelectionChange}
              onNodeDragStart={handleNodeDragStart}
              onNodeDrag={handleNodeDrag}
              onNodeDragStop={handleNodeDragStop}
              onMove={(_, viewport) => {
                viewportRef.current = viewport;
              }}
              onPaneClick={() => {
                syncSelection([], []);
                setInspectorNodeId(null);
              }}
              onNodeClick={(_, node) => {
                setInspectorNodeId(node.id);
                syncSelection([node.id], []);
              }}
              onEdgeClick={(_, edge) => {
                setInspectorNodeId(null);
                syncSelection([], [edge.id]);
              }}
              defaultViewport={defaultViewport}
              fitView={false}
              fitViewOptions={defaultFitViewOptions}
              snapToGrid
              snapGrid={defaultSnapGrid}
              minZoom={0.55}
              maxZoom={1.5}
              zoomOnScroll={false}
              zoomOnPinch={false}
              zoomOnDoubleClick={false}
              defaultEdgeOptions={defaultEdgeOptions}
              proOptions={proOptions}
            >
            </ReactFlow>
            </FlowSignalsContext.Provider>
          </div>
        </section>

        <aside className="panel panel--right">
          <div className="panel__title">Properties</div>
          {inspectorNode ? (
            <form className="property-sheet" onSubmit={(event) => event.preventDefault()}>
              <div className="property-sheet__header">
                <div>
                  <span>{inspectorNode.data.blockType.toUpperCase()}</span>
                  <strong>{inspectorNode.data.label}</strong>
                </div>
                <div className="property-sheet__actions">
                  <button
                    type="button"
                    className="property-sheet__delete"
                    onClick={handleDeleteInspectorNode}
                  >
                    Delete
                  </button>
                  <button
                    type="button"
                    className="property-sheet__close"
                    onClick={() => setInspectorNodeId(null)}
                  >
                    Close
                  </button>
                </div>
              </div>

              <label className="property-field">
                <span>Name</span>
                <input
                  type="text"
                  value={inspectorNode.data.label}
                  readOnly
                />
              </label>

              <label className="property-field">
                <span>Tag</span>
                <input
                  type="text"
                  value={inspectorNode.data.role}
                  readOnly
                />
              </label>

              {inspectorNode.data.propertyFields.map((field) => (
                <label key={field.key} className="property-field">
                  <span>{field.label}</span>
                  {field.inputMode === "select" ? (
                    <select
                      value={inspectorNode.data.properties[field.key] ?? ""}
                      onChange={(event) => handlePropertyChange(field, event.target.value)}
                    >
                      {field.options?.map((option) => (
                        <option key={option} value={option}>
                          {option}
                        </option>
                      ))}
                    </select>
                  ) : (
                    <input
                      type="text"
                      inputMode={field.inputMode}
                      step={field.step}
                      value={inspectorNode.data.properties[field.key] ?? ""}
                      onChange={(event) => handlePropertyChange(field, event.target.value)}
                    />
                  )}
                </label>
              ))}

              <div className="property-sheet__summary">
                <span>Live Value</span>
                <strong>
                  {formatSignalValue(
                    inspectorLiveData?.signalValue ?? null,
                    inspectorNode.data.properties.decimals,
                    inspectorNode.data.properties.unit,
                  )}
                </strong>
              </div>
            </form>
          ) : (
            <div className="property-sheet property-sheet--empty">
              <strong>Inspector idle</strong>
              <p>Click any block on the canvas to open its properties here.</p>
            </div>
          )}
        </aside>
      </section>
      )}
      <section className="workspace-footer" aria-label="workspace status">
        <div className="workspace-footer__status">
          <span>Status</span>
          <strong>{simulationStatus}</strong>
        </div>
        <div className="workspace-footer__zoom">
          <span>Zoom</span>
          <strong>{Math.round(zoomLevel * 100)}%</strong>
        </div>
      </section>
      {rackDrag
        ? createPortal(
            <article
              className={`rack-drag-preview flow-node flow-node--${rackDrag.blockType}${rackDrag.blockType === "constant" ? " flow-node--compact" : ""}`}
              style={{ left: rackDrag.x + 18, top: rackDrag.y + 18 }}
            >
              <BlockNodeBody data={buildPreviewNodeData(rackDrag.blockType)} />
            </article>,
            document.body,
          )
        : null}
      {inputErrorMessage ? (
        <div className="input-error-modal" role="dialog" aria-modal="true" aria-label="invalid number format">
          <div className="input-error-modal__panel">
            <strong>Invalid Number Format</strong>
            <p>{inputErrorMessage}</p>
            <button type="button" className="simulation-strip__button" onClick={() => setInputErrorMessage(null)}>
              OK
            </button>
          </div>
        </div>
      ) : null}
      {compileReport ? (
        <div className="input-error-modal" role="dialog" aria-modal="true" aria-label="compile report">
          <div className="input-error-modal__panel input-error-modal__panel--report">
            <strong>Compile Report</strong>
            <pre>{compileReport}</pre>
            <button type="button" className="simulation-strip__button" onClick={() => setCompileReport(null)}>
              Close
            </button>
          </div>
        </div>
      ) : null}
    </main>
  );
}

export default function App() {
  return (
    <AppErrorBoundary>
      <ReactFlowProvider>
        <ControlRoom />
      </ReactFlowProvider>
    </AppErrorBoundary>
  );
}

























