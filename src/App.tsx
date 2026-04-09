import type { BlockGraph } from "./types/graph";

const starterGraph: BlockGraph = {
  nodes: [],
  edges: [],
};

export default function App() {
  return (
    <main className="app-shell">
      <header className="app-header">
        <div>
          <p className="eyebrow">ctrl-lab</p>
          <h1>Block Diagram Studio</h1>
        </div>
        <p className="header-copy">
          Minimal workspace for building the first control blocks and the graph
          interpreter behind them.
        </p>
      </header>

      <section className="workspace-card">
        <div className="workspace-card__header">
          <h2>Project Status</h2>
          <span>{starterGraph.nodes.length} blocks</span>
        </div>
        <p>
          The canvas comes next. This shell is here so we can verify the
          toolchain and start adding node types without reworking the structure.
        </p>
      </section>
    </main>
  );
}
