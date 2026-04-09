export type PortId = string;
export type NodeId = string;

export type BlockType =
  | "constant"
  | "input"
  | "output"
  | "sum"
  | "gain"
  | "unitDelay"
  | "differenceEq";

export interface BlockNode<TParams = Record<string, unknown>> {
  id: NodeId;
  type: BlockType;
  position: {
    x: number;
    y: number;
  };
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
