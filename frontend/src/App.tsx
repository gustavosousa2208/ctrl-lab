import { lazy, Suspense, useEffect } from "react";

const EditorApp = lazy(() => import("./EditorApp"));

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
    <Suspense fallback={<LaunchScreen />}>
      <EditorApp />
    </Suspense>
  );
}
