import { useEffect, useState } from "react";
import TerminalView from "../terminal/TerminalView";
import DeveloperDogfoodWorkspace from "../settings/DeveloperDogfoodWorkspace";
import { terminalWorkspace } from "../terminal/TerminalWorkspace";
import { terminalApplicationEvidence } from "../terminal/TerminalApplicationEvidence";
import { fixtureTranscript } from "./terminalFixture";

/** Synthetic transport; DOM by default, real WebGL with gpu=enabled. */
const WORKER_COUNT = 15;

export default function TerminalPoolFixture() {
  const [worker, setWorker] = useState(1);
  const [snapshotBytes] = useState(() => new TextEncoder().encode(fixtureTranscript(new URLSearchParams(window.location.search).get("history") === "large")).byteLength);
  const [generations, setGenerations] = useState(Array<number>(WORKER_COUNT).fill(0));
  const [retained, setRetained] = useState<ReturnType<typeof inspectRetention>>();
  const [gpuEvidence, setGpuEvidence] = useState("Not inspected");
  const gpuEnabled = new URLSearchParams(window.location.search).get("gpu") === "enabled";
  function inspectGpu(lose: boolean) {
    const canvases = document.querySelectorAll<HTMLCanvasElement>(".terminal-surface canvas");
    let contexts = 0;
    let requested = 0;
    for (const canvas of canvases) {
      const context = canvas.getContext("webgl2");
      if (!context || context.isContextLost()) continue;
      contexts++;
      if (lose) {
        const extension = context.getExtension("WEBGL_lose_context");
        if (extension) { extension.loseContext(); requested++; }
      }
    }
    setGpuEvidence(`${contexts} live WebGL contexts; ${requested} loss requests; ${document.querySelectorAll(".terminal-surface .xterm-rows").length} DOM renderers`);
  }
  useEffect(() => {
    terminalWorkspace.reconcileSessions(generations.map((generation, index) => `fixture-pool-${index + 1}-${generation}`));
  }, [generations]);
  return <main>
    <h2>Terminal pool lifecycle fixture</h2>
    <p>Fifteen synthetic workers. Visit every worker twice and replace a selected session to check lifecycle retention. Repeat with the five-renderer experiment below. This is not a production performance benchmark or a memory measurement.</p>
    <p aria-label="Fixture snapshot workload">Snapshot payload: {snapshotBytes} bytes · synthetic transport, no network latency.</p>
    <nav aria-label="Fixture workers">
      {Array.from({ length: WORKER_COUNT }, (_, index) => index + 1).map((number) => <button type="button" key={number} aria-pressed={number === worker} onClick={() => setWorker(number)}>Fixture worker {number}</button>)}
    </nav>
    {gpuEnabled && <section aria-label="GPU lifecycle fixture">
      <p>Real WebGL, synthetic workers. Lose the selected context, inspect fallback, switch workers and return, then inspect recovery. No Hive requests.</p>
      <button onClick={() => inspectGpu(true)}>Lose selected GPU context</button>
      <button onClick={() => inspectGpu(false)}>Inspect GPU renderer</button>
      <p role="status">{gpuEvidence}</p>
    </section>}
    <button onClick={() => setGenerations((current) => current.map((generation, index) => index === worker - 1 ? generation + 1 : generation))}>Replace selected session</button>
    <button onClick={() => setRetained(inspectRetention())}>Inspect retained renderers</button>
    {retained !== undefined && <p role="status">{retained.retained} retained browser renderers · {retained.attached} attached · {retained.inactive} inactive · {retained.evictions} evicted · page {retained.visibility} · {retained.focused ? "focused" : "unfocused"}</p>}
    {retained !== undefined && <pre aria-label="Snapshot application evidence">{JSON.stringify(retained.application, null, 2)}</pre>}
    <div style={{ height: 480, display: "flex", flexDirection: "column" }}>
      <TerminalView session={{ session_id: `fixture-pool-${worker}-${generations[worker - 1]}`, running: true }} operatorToken="fixture-only" busy={false} />
    </div>
    <DeveloperDogfoodWorkspace runtime={{ enabled: true, version: "fixture-dev", state: "idle", reload_available: false, source_revision: "fixture-revision", source_dirty: false, deployed_source_published: false }} version="fixture-dev" reachable />
  </main>;
}

function inspectRetention() {
  return { ...terminalWorkspace.rendererRetention, visibility: document.visibilityState, focused: document.hasFocus(), application: terminalApplicationEvidence.snapshot() };
}
