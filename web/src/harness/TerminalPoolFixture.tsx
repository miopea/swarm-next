import { useEffect, useState } from "react";
import TerminalView from "../terminal/TerminalView";
import DeveloperDogfoodWorkspace from "../settings/DeveloperDogfoodWorkspace";
import { terminalWorkspace } from "../terminal/TerminalWorkspace";

/** Lifecycle smoke test only: synthetic transport and the harness's DOM renderer. */
const WORKER_COUNT = 15;

export default function TerminalPoolFixture() {
  const [worker, setWorker] = useState(1);
  const [generations, setGenerations] = useState(Array<number>(WORKER_COUNT).fill(0));
  const [retained, setRetained] = useState<ReturnType<typeof inspectRetention>>();
  useEffect(() => {
    terminalWorkspace.reconcileSessions(generations.map((generation, index) => `fixture-pool-${index + 1}-${generation}`));
  }, [generations]);
  return <main>
    <h2>Terminal pool lifecycle fixture</h2>
    <p>Fifteen synthetic workers. Visit every worker twice and replace a selected session to check lifecycle retention. Repeat with the five-renderer experiment below. This is not a production performance benchmark or a memory measurement.</p>
    <nav aria-label="Fixture workers">
      {Array.from({ length: WORKER_COUNT }, (_, index) => index + 1).map((number) => <button type="button" key={number} aria-pressed={number === worker} onClick={() => setWorker(number)}>Fixture worker {number}</button>)}
    </nav>
    <button onClick={() => setGenerations((current) => current.map((generation, index) => index === worker - 1 ? generation + 1 : generation))}>Replace selected session</button>
    <button onClick={() => setRetained(inspectRetention())}>Inspect retained renderers</button>
    {retained !== undefined && <p role="status">{retained.retained} retained browser renderers · {retained.attached} attached · {retained.inactive} inactive · {retained.evictions} evicted</p>}
    <div style={{ height: 480, display: "flex", flexDirection: "column" }}>
      <TerminalView session={{ session_id: `fixture-pool-${worker}-${generations[worker - 1]}`, running: true }} operatorToken="fixture-only" busy={false} />
    </div>
    <DeveloperDogfoodWorkspace runtime={{ enabled: true, version: "fixture-dev", state: "idle", reload_available: false, source_revision: "fixture-revision", source_dirty: false, deployed_source_published: false }} version="fixture-dev" reachable />
  </main>;
}

function inspectRetention() { return terminalWorkspace.rendererRetention; }
