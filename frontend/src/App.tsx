import { lazy, Suspense, useEffect, useState } from "react";

const EditorApp = lazy(() => import("./EditorApp"));
const MonitorApp = lazy(() => import("./MonitorApp"));

type View = "editor" | "monitor";

function LaunchScreen() {
  return (
    <main className="app-launch-screen" aria-busy="true" aria-live="polite">
      <section className="app-launch-screen__panel">
        <span className="app-launch-screen__eyebrow">CTRL-LAB</span>
        <strong>Loading workspace</strong>
        <p>Preparing the editor runtime and block canvas.</p>
      </section>
    </main>
  );
}

export default function App() {
  const [view, setView] = useState<View>("editor");

  useEffect(() => {
    const preloadEditor = () => {
      void import("./EditorApp");
    };

    if ("requestIdleCallback" in window) {
      const idleId = window.requestIdleCallback(preloadEditor);
      return () => window.cancelIdleCallback(idleId);
    }

    const timeoutId = globalThis.setTimeout(preloadEditor, 0);
    return () => globalThis.clearTimeout(timeoutId);
  }, []);

  return (
    <div className="app-shell">
      {/* A separate view rather than a panel inside the editor. The editor's
          scope plots a simulated model; the monitor plots a board. Keeping
          them apart is the point - see
          .internal/specs/2026-09-10-live-monitor-design.md. */}
      <nav className="app-views" aria-label="workspace">
        <button type="button" aria-pressed={view === "editor"}
                className={view === "editor" ? "is-active" : undefined}
                onClick={() => setView("editor")}>
          editor
        </button>
        <button type="button" aria-pressed={view === "monitor"}
                className={view === "monitor" ? "is-active" : undefined}
                onClick={() => setView("monitor")}>
          monitor
        </button>
      </nav>

      <Suspense fallback={<LaunchScreen />}>
        {/* The editor stays mounted so switching views does not throw away an
            unsaved diagram or a simulation result. */}
        <div hidden={view !== "editor"} className="app-view">
          <EditorApp />
        </div>
        {view === "monitor" && (
          <div className="app-view">
            <MonitorApp />
          </div>
        )}
      </Suspense>
    </div>
  );
}
