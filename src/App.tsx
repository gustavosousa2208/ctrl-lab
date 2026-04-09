import type { CSSProperties, ChangeEvent, DragEvent } from "react";
import { useEffect, useRef, useState } from "react";
import {
  addEdge,
  Controls,
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
  type Node,
  type NodeProps,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import type { BlockGraph, BlockType } from "./types/graph";

type PropertyField = {
  key: string;
  label: string;
  inputMode?: "text" | "decimal";
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

type SimulationDocument = {
  version: 1;
  generatedAt: string;
  simulation: {
    startTime: number;
    endTime: number;
    stepSize: number;
  };
  nodes: SerializedNode[];
  edges: SerializedEdge[];
  relationships: Array<{
    from: {
      nodeId: string;
      portId: string | null;
    };
    to: {
      nodeId: string;
      portId: string | null;
    };
  }>;
};

type ProjectDocument = {
  version: 1;
  kind: "ctrl-lab-project";
  generatedAt: string;
  title: string;
  simulation: {
    endTime: number;
    stepSize: number;
  };
  nodes: SerializedNode[];
  edges: SerializedEdge[];
};

const gridSize = 24;

const blockCatalog: Record<BlockType, BlockDefinition> = {
  constant: {
    label: "Constant",
    role: "SRC-01",
    description: "Fixed source for setpoints and bias injection.",
    accent: "#6b8452",
    inputs: [],
    outputs: ["out"],
    propertyFields: [{ key: "value", label: "Value", inputMode: "decimal", step: "0.1" }],
    defaultProperties: {
      value: "1.0",
    },
  },
  squareWave: {
    label: "Square Wave Generator",
    role: "OSC-02",
    description: "Pulsed test source for switching logic and timing checks.",
    accent: "#9a7f35",
    inputs: [],
    outputs: ["out"],
    propertyFields: [
      { key: "amplitude", label: "Amplitude", inputMode: "decimal", step: "0.1" },
      { key: "frequency", label: "Frequency (Hz)", inputMode: "decimal", step: "0.1" },
      { key: "duty", label: "Duty Cycle (%)", inputMode: "decimal", step: "1" },
    ],
    defaultProperties: {
      amplitude: "1.0",
      frequency: "1.0",
      duty: "50",
    },
  },
  sum: {
    label: "Summing Node",
    role: "SUM-03",
    description: "Adds or subtracts incoming signals before routing forward.",
    accent: "#58706d",
    inputs: ["a", "b"],
    outputs: ["out"],
    propertyFields: [{ key: "equation", label: "Equation", inputMode: "text" }],
    defaultProperties: {
      equation: "A + B",
    },
  },
  scope: {
    label: "Scope",
    role: "MON-04",
    description: "Visual sink for watching the live signal at this point.",
    accent: "#4f6686",
    inputs: ["in"],
    outputs: [],
    propertyFields: [
      { key: "channel", label: "Channel", inputMode: "text" },
      { key: "timebase", label: "Timebase", inputMode: "text" },
    ],
    defaultProperties: {
      channel: "CH-1",
      timebase: "1 s/div",
    },
  },
  display: {
    label: "Display",
    role: "DSP-05",
    description: "Numeric readout for the current signal value at this point.",
    accent: "#7b5d4d",
    inputs: ["in"],
    outputs: [],
    propertyFields: [
      { key: "decimals", label: "Decimals", inputMode: "decimal", step: "1" },
      { key: "unit", label: "Unit", inputMode: "text" },
    ],
    defaultProperties: {
      decimals: "2",
      unit: "",
    },
  },
};

const starterGraph: BlockGraph = {
  nodes: [
    {
      id: "constant-1",
      type: "constant",
      position: { x: 84, y: 122 },
    },
    {
      id: "square-wave-1",
      type: "squareWave",
      position: { x: 84, y: 286 },
    },
    {
      id: "sum-1",
      type: "sum",
      position: { x: 358, y: 198 },
    },
    {
      id: "display-1",
      type: "display",
      position: { x: 670, y: 118 },
    },
    {
      id: "scope-1",
      type: "scope",
      position: { x: 670, y: 272 },
    },
  ],
  edges: [
    {
      id: "edge-1",
      sourceNodeId: "constant-1",
      sourcePortId: "out",
      targetNodeId: "sum-1",
      targetPortId: "a",
    },
    {
      id: "edge-2",
      sourceNodeId: "square-wave-1",
      sourcePortId: "out",
      targetNodeId: "sum-1",
      targetPortId: "b",
    },
    {
      id: "edge-3",
      sourceNodeId: "sum-1",
      sourcePortId: "out",
      targetNodeId: "display-1",
      targetPortId: "in",
    },
    {
      id: "edge-4",
      sourceNodeId: "sum-1",
      sourcePortId: "out",
      targetNodeId: "scope-1",
      targetPortId: "in",
    },
  ],
};

const starterBlocks: BlockType[] = ["constant", "squareWave", "sum", "display", "scope"];
const workspaceTitle = "inserir texto";

const defaultMarker = {
  type: MarkerType.ArrowClosed,
  width: 18,
  height: 18,
  color: "#344537",
};

function parseNumber(value: string | undefined, fallback: number) {
  const parsed = Number.parseFloat(value ?? "");
  return Number.isFinite(parsed) ? parsed : fallback;
}

function formatSignalValue(value: number | null, decimalsText = "2", unit = "") {
  if (value === null || Number.isNaN(value)) {
    return "--";
  }

  const decimals = Math.max(0, Math.min(6, Math.round(parseNumber(decimalsText, 2))));
  const suffix = unit.trim() ? ` ${unit.trim()}` : "";

  return `${value.toFixed(decimals)}${suffix}`;
}

function evaluateEquation(equation: string, a: number, b: number) {
  const normalized = equation.toUpperCase().replace(/\s+/g, "");

  switch (normalized) {
    case "A-B":
      return a - b;
    case "B-A":
      return b - a;
    case "B+A":
      return b + a;
    case "A+B":
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
    case "squareWave":
      return `${properties.frequency} Hz / ${properties.duty}%`;
    case "sum":
      return properties.equation;
    case "scope":
      return `${properties.channel} / ${properties.timebase}`;
    case "display":
      return formatSignalValue(signalValue, properties.decimals, properties.unit);
    default:
      return "";
  }
}

function isBlockType(value: unknown): value is BlockType {
  return typeof value === "string" && value in blockCatalog;
}

function toCanvasNode(id: string, blockType: BlockType, x: number, y: number): CanvasNode {
  const definition = blockCatalog[blockType];
  const properties = { ...definition.defaultProperties };

  return {
    id,
    type: "controlBlock",
    position: { x, y },
    data: {
      ...definition,
      blockType,
      properties,
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
      signalValue: null,
      detail: buildNodeDetail(node.type, properties),
    },
  };
}

function cloneCanvasNode(sourceNode: CanvasNode, id: string, x: number, y: number): CanvasNode {
  return {
    ...sourceNode,
    id,
    position: { x, y },
    selected: false,
    dragging: false,
    data: {
      ...sourceNode.data,
      properties: { ...sourceNode.data.properties },
      signalValue: null,
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
    type: "smoothstep",
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
    type: "smoothstep",
    markerEnd: defaultMarker,
  };
}

function handleOffset(index: number, total: number) {
  return `${((index + 1) / (total + 1)) * 100}%`;
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
    Array.isArray(candidate.nodes) &&
    Array.isArray(candidate.edges)
  );
}

function buildSimulationDocument(
  nodes: CanvasNode[],
  edges: CanvasEdge[],
  endTime: number,
  stepSize: number,
): SimulationDocument {
  return {
    version: 1,
    generatedAt: new Date().toISOString(),
    simulation: {
      startTime: 0,
      endTime,
      stepSize,
    },
    nodes: nodes.map(toSerializedNode),
    edges: edges.map(toSerializedEdge),
    relationships: edges.map((edge) => ({
      from: {
        nodeId: edge.source,
        portId: edge.sourceHandle ?? null,
      },
      to: {
        nodeId: edge.target,
        portId: edge.targetHandle ?? null,
      },
    })),
  };
}

function buildProjectDocument(
  nodes: CanvasNode[],
  edges: CanvasEdge[],
  endTime: number,
  stepSize: number,
): ProjectDocument {
  return {
    version: 1,
    kind: "ctrl-lab-project",
    generatedAt: new Date().toISOString(),
    title: workspaceTitle,
    simulation: {
      endTime,
      stepSize,
    },
    nodes: nodes.map(toSerializedNode),
    edges: edges.map(toSerializedEdge),
  };
}

function downloadJsonDocument(filename: string, document: ProjectDocument | SimulationDocument) {
  const blob = new Blob([JSON.stringify(document, null, 2)], {
    type: "application/json",
  });
  const url = URL.createObjectURL(blob);
  const link = window.document.createElement("a");

  link.href = url;
  link.download = filename;
  link.click();

  URL.revokeObjectURL(url);
}

function ControlBlockNode({ data, selected }: NodeProps<CanvasNode>) {
  const accentStyle = { "--node-accent": data.accent } as CSSProperties;
  const displayReadout =
    data.blockType === "display"
      ? formatSignalValue(data.signalValue, data.properties.decimals, data.properties.unit)
      : null;

  return (
    <article
      className={`flow-node flow-node--${data.blockType}${selected ? " is-selected" : ""}`}
      style={accentStyle}
    >
      {data.inputs.map((portId, index) => (
        <Handle
          key={`input-${portId}`}
          id={portId}
          type="target"
          position={Position.Left}
          className="flow-node__handle"
          style={{ top: handleOffset(index, data.inputs.length) }}
        />
      ))}

      {data.outputs.map((portId, index) => (
        <Handle
          key={`output-${portId}`}
          id={portId}
          type="source"
          position={Position.Right}
          className="flow-node__handle"
          style={{ top: handleOffset(index, data.outputs.length) }}
        />
      ))}

      <div className="flow-node__head">
        <span>{data.role}</span>
        <strong>{data.blockType.toUpperCase()}</strong>
      </div>
      <h3>{data.label}</h3>
      <p>{data.description}</p>
      {displayReadout ? <div className="flow-node__display">{displayReadout}</div> : null}
      <div className="flow-node__meta">
        <span>{data.detail}</span>
      </div>
    </article>
  );
}

const nodeTypes = {
  controlBlock: ControlBlockNode,
};

function ControlRoom() {
  const [nodes, setNodes, onNodesChange] = useNodesState(
    starterGraph.nodes.map((node) => toCanvasNode(node.id, node.type, node.position.x, node.position.y)),
  );
  const [edges, setEdges, onEdgesChange] = useEdgesState(starterGraph.edges.map(toCanvasEdge));
  const [inspectorNodeId, setInspectorNodeId] = useState<string | null>(null);
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [selectedEdgeId, setSelectedEdgeId] = useState<string | null>(null);
  const [endTime, setEndTime] = useState("10");
  const [stepSize, setStepSize] = useState("0.1");
  const [simulationStatus, setSimulationStatus] = useState("Idle");
  const [activeMenu, setActiveMenu] = useState<"file" | null>(null);
  const [timeSeconds, setTimeSeconds] = useState(0);
  const nextNodeNumber = useRef(starterGraph.nodes.length + 1);
  const openFileInputRef = useRef<HTMLInputElement | null>(null);
  const fileMenuRef = useRef<HTMLDivElement | null>(null);
  const dragDuplicateRef = useRef<{
    nodeId: string;
    duplicateId: string;
    startPosition: { x: number; y: number };
  } | null>(null);
  const { screenToFlowPosition } = useReactFlow();

  useEffect(() => {
    const startedAt = Date.now();
    const intervalId = window.setInterval(() => {
      setTimeSeconds((Date.now() - startedAt) / 1000);
    }, 200);

    return () => window.clearInterval(intervalId);
  }, []);

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

  const signalValues = evaluateSignalGraph(nodes as CanvasNode[], edges, timeSeconds);
  const renderedNodes = nodes.map((node) => {
    const signalValue = signalValues.get(node.id) ?? null;

    return {
      ...node,
      data: {
        ...node.data,
        signalValue,
        detail: buildNodeDetail(node.data.blockType, node.data.properties, signalValue),
      },
    };
  });
  const inspectorNode = renderedNodes.find((node) => node.id === inspectorNodeId) ?? null;

  function createNode(blockType: BlockType, clientX: number, clientY: number) {
    const position = screenToFlowPosition({ x: clientX, y: clientY });
    const x = Math.round((position.x - 92) / gridSize) * gridSize;
    const y = Math.round((position.y - 54) / gridSize) * gridSize;
    const id = `${blockType}-${nextNodeNumber.current}`;

    nextNodeNumber.current += 1;

    const nextNode = toCanvasNode(id, blockType, x, y);

    setNodes((currentNodes) => currentNodes.concat(nextNode));
  }

  function updateInspectorNode(updater: (node: CanvasNode) => CanvasNode) {
    if (!inspectorNodeId) {
      return;
    }

    setNodes((currentNodes) =>
      currentNodes.map((node) => (node.id === inspectorNodeId ? updater(node as CanvasNode) : node)),
    );
  }

  function handleConnect(connection: Connection) {
    setEdges((currentEdges) =>
      addEdge(
        {
          ...connection,
          type: "smoothstep",
          markerEnd: defaultMarker,
        },
        currentEdges,
      ),
    );
  }

  function handleDragStart(event: DragEvent<HTMLButtonElement>, blockType: BlockType) {
    event.dataTransfer.setData("application/ctrl-lab-block", blockType);
    event.dataTransfer.effectAllowed = "move";
  }

  function handleDragOver(event: DragEvent<HTMLDivElement>) {
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
  }

  function handleDrop(event: DragEvent<HTMLDivElement>) {
    event.preventDefault();

    const blockType = event.dataTransfer.getData("application/ctrl-lab-block") as BlockType;

    if (!blockType || !(blockType in blockCatalog)) {
      return;
    }

    createNode(blockType, event.clientX, event.clientY);
  }

  function handleNodeDragStart(event: { ctrlKey?: boolean; metaKey?: boolean }, node: Node) {
    if (event.ctrlKey || event.metaKey) {
      const duplicateId = `${node.data.blockType}-${nextNodeNumber.current}`;
      nextNodeNumber.current += 1;

      setNodes((currentNodes) => {
        const sourceNode = currentNodes.find((currentNode) => currentNode.id === node.id) as CanvasNode | undefined;

        if (!sourceNode) {
          return currentNodes;
        }

        const duplicateNode = cloneCanvasNode(sourceNode, duplicateId, node.position.x, node.position.y);

        return currentNodes.concat(duplicateNode);
      });

      dragDuplicateRef.current = {
        nodeId: node.id,
        duplicateId,
        startPosition: { x: node.position.x, y: node.position.y },
      };
      setSelectedNodeId(duplicateId);
      setSelectedEdgeId(null);
      return;
    }

    dragDuplicateRef.current = null;
  }

  function handleNodeDrag(_: unknown, node: Node) {
    const duplicateState = dragDuplicateRef.current;

    if (!duplicateState || duplicateState.nodeId !== node.id) {
      return;
    }

    setNodes((currentNodes) =>
      currentNodes.map((currentNode) => {
        if (currentNode.id === duplicateState.nodeId) {
          return {
            ...currentNode,
            position: duplicateState.startPosition,
          };
        }

        if (currentNode.id === duplicateState.duplicateId) {
          return {
            ...currentNode,
            position: { x: node.position.x, y: node.position.y },
          };
        }

        return currentNode;
      }),
    );
  }

  function handleNodeDragStop(_: unknown, node: Node) {
    const duplicateState = dragDuplicateRef.current;
    dragDuplicateRef.current = null;

    if (!duplicateState || duplicateState.nodeId !== node.id) {
      return;
    }

    const moved =
      duplicateState.startPosition.x !== node.position.x || duplicateState.startPosition.y !== node.position.y;

    if (!moved) {
      setNodes((currentNodes) =>
        currentNodes.filter((currentNode) => currentNode.id !== duplicateState.duplicateId),
      );
      setSelectedNodeId((currentId) => (currentId === duplicateState.duplicateId ? duplicateState.nodeId : currentId));
      return;
    }

    setNodes((currentNodes) => {
      return currentNodes.map((currentNode) => {
        if (currentNode.id === duplicateState.nodeId) {
          return {
            ...currentNode,
            position: duplicateState.startPosition,
          };
        }

        if (currentNode.id === duplicateState.duplicateId) {
          return {
            ...currentNode,
            position: { x: node.position.x, y: node.position.y },
          };
        }

        return currentNode;
      });
    });

    setSelectedNodeId(duplicateState.duplicateId);
    setSelectedEdgeId(null);
  }

  function handleBaseFieldChange(field: "label" | "role", value: string) {
    updateInspectorNode((node) => ({
      ...node,
      data: {
        ...node.data,
        [field]: value,
      },
    }));
  }

  function handlePropertyChange(key: string, value: string) {
    updateInspectorNode((node) => {
      const properties = {
        ...node.data.properties,
        [key]: value,
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

  function handleStartSimulation() {
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

    const simulationDocument = buildSimulationDocument(nodes as CanvasNode[], edges, parsedEndTime, parsedStepSize);
    downloadJsonDocument(
      `ctrl-lab-simulation-${simulationDocument.generatedAt.replace(/[:.]/g, "-")}.json`,
      simulationDocument,
    );
    setSimulationStatus(
      `Exported ${simulationDocument.nodes.length} blocks and ${simulationDocument.edges.length} links`,
    );
  }

  function handleSaveProject() {
    const parsedEndTime = parseNumber(endTime, 10);
    const parsedStepSize = parseNumber(stepSize, 0.1);
    const projectDocument = buildProjectDocument(nodes as CanvasNode[], edges, parsedEndTime, parsedStepSize);

    downloadJsonDocument(
      `ctrl-lab-project-${projectDocument.generatedAt.replace(/[:.]/g, "-")}.json`,
      projectDocument,
    );
    setSimulationStatus(`Saved project with ${projectDocument.nodes.length} blocks`);
  }

  function handleOpenProject() {
    openFileInputRef.current?.click();
  }

  function handleFileMenuAction(action: "save" | "open") {
    setActiveMenu(null);

    if (action === "save") {
      handleSaveProject();
      return;
    }

    handleOpenProject();
  }

  async function handleOpenFileChange(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];

    if (!file) {
      return;
    }

    try {
      const contents = await file.text();
      const parsed = JSON.parse(contents) as unknown;

      if (!isProjectDocument(parsed)) {
        throw new Error("invalid-project");
      }

      const restoredNodes = parsed.nodes
        .filter((node) => isBlockType(node.type))
        .map((node) => fromSerializedNode(node));
      const restoredEdges = parsed.edges.map((edge) => fromSerializedEdge(edge));

      setNodes(restoredNodes);
      setEdges(restoredEdges);
      setEndTime(String(parsed.simulation.endTime));
      setStepSize(String(parsed.simulation.stepSize));
      setInspectorNodeId(null);
      setSelectedNodeId(null);
      setSelectedEdgeId(null);
      nextNodeNumber.current = getNextNodeNumber(parsed.nodes);
      setSimulationStatus(`Opened ${file.name}`);
    } catch {
      setSimulationStatus("Unable to open that file");
    } finally {
      event.target.value = "";
    }
  }

  function deleteNodeById(nodeId: string) {
    setNodes((currentNodes) => currentNodes.filter((node) => node.id !== nodeId));
    setEdges((currentEdges) => currentEdges.filter((edge) => edge.source !== nodeId && edge.target !== nodeId));
    setSelectedNodeId((currentId) => (currentId === nodeId ? null : currentId));
    setSelectedEdgeId(null);
    setInspectorNodeId((currentId) => (currentId === nodeId ? null : currentId));
  }

  function deleteEdgeById(edgeId: string) {
    setEdges((currentEdges) => currentEdges.filter((edge) => edge.id !== edgeId));
    setSelectedEdgeId((currentId) => (currentId === edgeId ? null : currentId));
  }

  function handleDeleteInspectorNode() {
    if (!inspectorNodeId) {
      return;
    }

    deleteNodeById(inspectorNodeId);
  }

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key !== "Delete") {
        return;
      }

      const target = event.target;

      if (
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement ||
        (target instanceof HTMLElement && target.isContentEditable)
      ) {
        return;
      }

      if (selectedNodeId) {
        event.preventDefault();
        deleteNodeById(selectedNodeId);
        return;
      }

      if (selectedEdgeId) {
        event.preventDefault();
        deleteEdgeById(selectedEdgeId);
      }
    }

    window.addEventListener("keydown", handleKeyDown);

    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [selectedEdgeId, selectedNodeId]);

  function handleSelectionChange({
    nodes: selectedNodes,
    edges: selectedEdges,
  }: {
    nodes: Node[];
    edges: Edge[];
  }) {
    setSelectedNodeId(selectedNodes[0]?.id ?? null);
    setSelectedEdgeId(selectedEdges[0]?.id ?? null);
  }

  return (
    <main className="control-room">
      <section className="simulation-strip" aria-label="simulation controls">
        <div className="simulation-strip__menus">
          <div className="simulation-strip__brand">
            <span className="chrome-bar__badge">CTRL-LAB</span>
            <strong>{workspaceTitle}</strong>
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
                    onClick={() => handleFileMenuAction("save")}
                  >
                    Save
                  </button>
                  <button
                    type="button"
                    className="simulation-strip__menu-item"
                    role="menuitem"
                    onClick={() => handleFileMenuAction("open")}
                  >
                    Open
                  </button>
                </div>
              ) : null}
            </div>

            <button type="button" className="simulation-strip__menu-button">
              Edit
            </button>
            <button type="button" className="simulation-strip__menu-button">
              View
            </button>
            <button type="button" className="simulation-strip__menu-button">
              Setting
            </button>
          </nav>
        </div>

        <div className="simulation-strip__simulation-group">
          <button type="button" className="simulation-strip__button" onClick={handleStartSimulation}>
            Start Simulation
          </button>

          <label className="simulation-strip__field">
            <span>End Time</span>
            <input
              type="number"
              inputMode="decimal"
              step="0.1"
              min="0"
              value={endTime}
              onChange={(event) => setEndTime(event.target.value)}
            />
          </label>

          <label className="simulation-strip__field">
            <span>Step Size</span>
            <input
              type="number"
              inputMode="decimal"
              step="0.01"
              min="0"
              value={stepSize}
              onChange={(event) => setStepSize(event.target.value)}
            />
          </label>
        </div>

        <div className="simulation-strip__status">
          <span>Status</span>
          <strong>{simulationStatus}</strong>
        </div>

        <input
          ref={openFileInputRef}
          type="file"
          accept=".json,application/json"
          className="simulation-strip__file-input"
          onChange={handleOpenFileChange}
        />
      </section>

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
                  draggable
                  className={`library-card library-card--${blockType}`}
                  onDragStart={(event) => handleDragStart(event, blockType)}
                >
                  <span>{block.role}</span>
                  <strong>{block.label}</strong>
                  <p>{detail}</p>
                </button>
              );
            })}
          </div>
        </aside>

        <section className="canvas-frame" aria-label="block diagram canvas">
          <div className="canvas-frame__header">
            <div>
              <span className="panel__title">Process Canvas</span>
              <p>Live editing surface for the current loop. Drag from the left rack to add more blocks.</p>
            </div>
            <div className="canvas-frame__tag">LINE A / BAY 03</div>
          </div>

          <div className="canvas-surface canvas-surface--flow" onDragOver={handleDragOver} onDrop={handleDrop}>
            <ReactFlow
              nodes={renderedNodes}
              edges={edges}
              nodeTypes={nodeTypes}
              onNodesChange={onNodesChange}
              onEdgesChange={onEdgesChange}
              onConnect={handleConnect}
              onNodeDragStart={handleNodeDragStart}
              onNodeDrag={handleNodeDrag}
              onNodeDragStop={handleNodeDragStop}
              onPaneClick={() => {
                setSelectedNodeId(null);
                setSelectedEdgeId(null);
              }}
              onSelectionChange={handleSelectionChange}
              onNodeDoubleClick={(_, node) => setInspectorNodeId(node.id)}
              fitView
              fitViewOptions={{ padding: 0.16 }}
              snapToGrid
              snapGrid={[gridSize, gridSize]}
              minZoom={0.55}
              maxZoom={1.5}
              defaultEdgeOptions={{ type: "smoothstep", markerEnd: defaultMarker }}
              proOptions={{ hideAttribution: true }}
            >
              <Controls showInteractive={false} />
            </ReactFlow>
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
                  onChange={(event) => handleBaseFieldChange("label", event.target.value)}
                />
              </label>

              <label className="property-field">
                <span>Tag</span>
                <input
                  type="text"
                  value={inspectorNode.data.role}
                  onChange={(event) => handleBaseFieldChange("role", event.target.value)}
                />
              </label>

              {inspectorNode.data.propertyFields.map((field) => (
                <label key={field.key} className="property-field">
                  <span>{field.label}</span>
                  <input
                    type={field.inputMode === "decimal" ? "number" : "text"}
                    inputMode={field.inputMode}
                    step={field.step}
                    value={inspectorNode.data.properties[field.key] ?? ""}
                    onChange={(event) => handlePropertyChange(field.key, event.target.value)}
                  />
                </label>
              ))}

              <div className="property-sheet__summary">
                <span>Live Value</span>
                <strong>
                  {formatSignalValue(
                    inspectorNode.data.signalValue,
                    inspectorNode.data.properties.decimals,
                    inspectorNode.data.properties.unit,
                  )}
                </strong>
              </div>
            </form>
          ) : (
            <div className="property-sheet property-sheet--empty">
              <strong>Inspector idle</strong>
              <p>Double-click any block on the canvas to open its properties here.</p>
            </div>
          )}
        </aside>
      </section>
    </main>
  );
}

export default function App() {
  return (
    <ReactFlowProvider>
      <ControlRoom />
    </ReactFlowProvider>
  );
}
