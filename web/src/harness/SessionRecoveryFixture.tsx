import { useState } from "react";
import { App } from "../App";

let available = false;
// App updates its navigation URL during mount; retain the selected fixture.
const enabled = new URLSearchParams(window.location.search).get("surface") === "session-recovery";

/** Isolated session-check failure. No request leaves the harness transport. */
export function sessionRecoveryResponse(path: string): Response | undefined {
  if (!enabled || path !== "/api/v1/auth/session" || available) return undefined;
  return new Response(JSON.stringify({ message: "Fictional session service unavailable" }), {
    status: 500, headers: { "content-type": "application/json" },
  });
}

export default function SessionRecoveryFixture() {
  const [ready, setReady] = useState(available);
  return <>
    <aside aria-label="Fictional connection controls">
      <button type="button" disabled={ready} onClick={() => { available = true; setReady(true); }}>
        Restore fictional API
      </button>
      <span>{ready ? "Fictional API available; retry in Swarm." : "Fictional session check unavailable. No Hive is contacted."}</span>
    </aside>
    <App />
  </>;
}
