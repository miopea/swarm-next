import { useEffect, useRef, useState } from "react";
import { TerminalConnection } from "../terminal/TerminalConnection";
import { XtermSurface } from "../terminal/XtermSurface";
import { FixtureWebSocket } from "./terminalFixture";
import "@xterm/xterm/css/xterm.css";

const payload = new TextEncoder().encode(Array.from({ length: 256 }, (_, index) =>
  `\r\n\u001b[32mOrchard ${String(index + 1).padStart(3, "0")}\u001b[0m · synthetic 🐝`).join("") + "\r\nBURST COMPLETE\r\n");
const PACKET_BYTES = 32;

/** Real connection/parser, invented in-memory transport. No Hive or provider. */
export default function TerminalBurstFixture() {
  const mount = useRef<HTMLDivElement>(null);
  const run = useRef<(() => void) | undefined>(undefined);
  const [ready, setReady] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("Opening synthetic terminal");
  const [buffer, setBuffer] = useState<string>();
  const inspect = useRef<(() => string | undefined) | undefined>(undefined);

  useEffect(() => {
    let active = true;
    let socket: FixtureWebSocket | undefined;
    let pending: { started: number; received: number; writes: number } | undefined;
    let deadline: ReturnType<typeof setTimeout> | undefined;
    const surface = new XtermSurface();
    surface.observeGeometryOwnership(() => false);
    surface.open(mount.current!);
    inspect.current = () => surface.serialize();
    const connection = new TerminalConnection({
      sessionId: "synthetic-burst", operatorToken: "fixture-only", deviceId: "fixture-device",
      retryDelaysMs: [],
      fetch: (async () => new Response(JSON.stringify({ grant: "fixture", protocol: "swarm-terminal.v4", websocket_path: "/synthetic-burst", expires_in_ms: 30_000 }))) as typeof fetch,
      websocketFactory: (url, protocols) => {
        socket = new FixtureWebSocket(url, protocols, false);
        return socket as unknown as WebSocket;
      },
    });
    connection.resize(24, 36);
    connection.start({
      onSnapshot: (snapshot) => surface.restore(snapshot),
      onOutput: async (bytes) => {
        await surface.write(bytes);
        if (!active || !pending) return;
        pending.received += bytes.byteLength;
        pending.writes++;
        if (pending.received === payload.byteLength) {
          setStatus(`${Math.ceil(payload.byteLength / PACKET_BYTES)} packets · ${pending.writes} parser ${pending.writes === 1 ? "write" : "writes"} · ${pending.received} bytes · ${Math.round(performance.now() - pending.started)} ms to apply`);
          pending = undefined;
          clearTimeout(deadline);
          setBusy(false);
        }
      },
      onState: (state, detail) => {
        if (!active) return;
        if (state === "connected") { setReady(true); if (!pending) setStatus("Ready for a synthetic burst"); }
        if (state === "error" || state === "recovery_required" || state === "disconnected") {
          setStatus(`Fixture stopped: ${detail ?? state}`);
          setReady(false);
          setBusy(false);
          pending = undefined;
          clearTimeout(deadline);
        }
      },
      onRunningChange: () => undefined,
    });
    run.current = () => {
      if (!socket || pending) return;
      pending = { started: performance.now(), received: 0, writes: 0 };
      setBusy(true);
      setBuffer(undefined);
      setStatus("Applying a bounded synthetic burst");
      deadline = setTimeout(() => {
        if (!active || !pending) return;
        pending = undefined;
        connection.dispose();
        setReady(false);
        setBusy(false);
        setStatus("Fixture exceeded its eight-second deadline; reload to retry");
      }, 8_000);
      for (let offset = 0; offset < payload.byteLength; offset += PACKET_BYTES) socket.emitOutput(payload.subarray(offset, offset + PACKET_BYTES));
    };
    return () => {
      active = false;
      clearTimeout(deadline);
      run.current = undefined;
      inspect.current = undefined;
      connection.dispose();
      surface.dispose();
    };
  }, []);

  return <main style={{ padding: 20, maxWidth: 1000, margin: "auto" }}>
    <h1>Terminal burst fixture</h1>
    <p>In-memory packets through the production connection and xterm parser. Synthetic data only; no worker, provider or Hive requests. The harness uses the DOM renderer.</p>
    <p>One burst contains 256 invented lines in 32-byte packets, splitting ANSI and UTF-8 sequences. Results measure application, not screen paint, network latency or whole-app CPU.</p>
    <button disabled={!ready || busy} onClick={() => run.current?.()}>Run synthetic burst</button>{" "}
    <button disabled={!ready || busy} onClick={() => setBuffer(inspect.current?.())}>Inspect fixture buffer</button>
    <p role="status">{status}</p>
    <div style={{ overflowX: "auto", maxWidth: "100%" }}><div ref={mount} style={{ width: 330, height: 430, background: "#101a15" }} /></div>
    {buffer !== undefined && <pre aria-label="Synthetic burst buffer" style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{JSON.stringify(buffer)}</pre>}
  </main>;
}
