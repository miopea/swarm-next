import { useState } from "react";
import TerminalView from "../terminal/TerminalView";
import DeveloperDogfoodWorkspace from "../settings/DeveloperDogfoodWorkspace";

/** Lifecycle smoke test only: synthetic transport and the harness's DOM renderer. */
export default function TerminalPoolFixture() {
  const [worker, setWorker] = useState(1);
  return <main>
    <h2>Terminal pool lifecycle fixture</h2>
    <p>Eight synthetic workers. Enable the experiment below, visit six workers, then return to the first. This is not a production performance benchmark.</p>
    <nav aria-label="Fixture workers">
      {Array.from({ length: 8 }, (_, index) => index + 1).map((number) => <button type="button" key={number} aria-pressed={number === worker} onClick={() => setWorker(number)}>Fixture worker {number}</button>)}
    </nav>
    <div style={{ height: 480, display: "flex", flexDirection: "column" }}>
      <TerminalView session={{ session_id: `fixture-pool-${worker}`, running: true }} operatorToken="fixture-only" busy={false} />
    </div>
    <DeveloperDogfoodWorkspace runtime={{ enabled: true, version: "fixture-dev", state: "idle", reload_available: false, source_revision: "fixture-revision", source_dirty: false, deployed_source_published: false }} version="fixture-dev" reachable />
  </main>;
}
