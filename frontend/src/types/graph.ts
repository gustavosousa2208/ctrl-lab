export type PortId = string;
export type NodeId = string;

export type BlockType =
  | "constant"
  | "step"
  | "delay"
  | "gain"
  | "integrator"
  | "transferFunction"
  | "switch"
  | "sum"
  | "display"
  | "scope"
  | "squareWave";

export interface BlockNode<TParams = Record<string, unknown>> {
  id: NodeId;
  type: BlockType;
  position: {
    x: number;
    y: number;
  };
  rotation?: number;
  params?: TParams;
}

export interface BlockEdge {
  id: string;
  sourceNodeId: NodeId;
  sourcePortId: PortId;
  targetNodeId: NodeId;
  targetPortId: PortId;
}

export interface BlockGraph {
  nodes: BlockNode[];
  edges: BlockEdge[];
}
