import { useEffect, useState } from "react";

type Health = { status: "ok"; version: string };
type LoadState = { kind: "loading" } | { kind: "ready"; health: Health } | { kind: "unavailable" };

export function App() {
  const [loadState, setLoadState] = useState<LoadState>({ kind: "loading" });

  useEffect(() => {
    const controller = new AbortController();
    void fetch("/health", { signal: controller.signal })
      .then((response) => {
        if (!response.ok) throw new Error(`Health returned ${response.status}`);
        return response.json() as Promise<Health>;
      })
      .then((health) => setLoadState({ kind: "ready", health }))
      .catch((error: unknown) => {
        if (!(error instanceof DOMException && error.name === "AbortError")) setLoadState({ kind: "unavailable" });
      });
    return () => controller.abort();
  }, []);

  return (
    <main className="app-shell">
      <aside className="worker-rail" aria-label="Workers">
        <div className="brand-mark" aria-hidden="true">S</div>
        <div><p className="eyebrow">Swarm Next</p><h1>Workers</h1></div>
        <div className="empty-rail">No workers running</div>
      </aside>
      <section className="workspace">
        <header className="workspace-header">
          <div><p className="eyebrow">Terminal foundation</p><h2>Durable sessions begin here</h2></div>
          <RuntimeStatus state={loadState} />
        </header>
        <div className="terminal-empty">
          <span className="terminal-prompt" aria-hidden="true">›_</span>
          <h3>No terminal attached</h3>
          <p>The first milestone will keep provider sessions alive across browser reloads and API restarts.</p>
        </div>
      </section>
    </main>
  );
}

function RuntimeStatus({ state }: { state: LoadState }) {
  if (state.kind === "ready") return <span className="status status-ready">Runtime {state.health.version}</span>;
  if (state.kind === "unavailable") return <span className="status status-error">Runtime unavailable</span>;
  return <span className="status">Connecting…</span>;
}
