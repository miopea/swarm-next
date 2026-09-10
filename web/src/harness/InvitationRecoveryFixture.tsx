import { useEffect, useRef, useState } from "react";
import KeeperInvitationManager from "../settings/KeeperInvitationManager";

/** Fictional transport only; never creates a capability or contacts a Hive. */
export default function InvitationRecoveryFixture() {
  const [ready, setReady] = useState(false);
  const [available, setAvailable] = useState(false);
  const restored = useRef(false);
  useEffect(() => {
    const previous = window.fetch;
    window.fetch = async (input, init) => {
      if (String(input) !== "/api/v1/apiary/join-links") return previous(input, init);
      if (init?.method === "POST") return new Promise<Response>((_resolve, reject) => {
        const signal = init.signal;
        if (signal?.aborted) reject(signal.reason);
        else signal?.addEventListener("abort", () => reject(signal.reason), { once: true });
      });
      if (init?.method && init.method !== "GET") return new Response("Fixture does not change membership", { status: 405 });
      return new Response(JSON.stringify(restored.current ? [] : { message: "Fictional status unavailable" }), {
        status: restored.current ? 200 : 503, headers: { "content-type": "application/json" },
      });
    };
    setReady(true);
    return () => { window.fetch = previous; };
  }, []);
  return <main className="settings-card">
    <h1>Fictional invitation recovery</h1>
    <p>No invitation or membership can be created here. Create invitation link simulates a stalled request.</p>
    <button disabled={available} onClick={() => { restored.current = true; setAvailable(true); }}>Restore fictional invitation service</button>
    {ready && <KeeperInvitationManager busy={false} operatorToken="fixture" onInvitationCreated={async () => undefined} />}
  </main>;
}
