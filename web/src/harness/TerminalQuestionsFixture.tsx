import { useEffect, useRef, useState } from "react";
import { XtermSurface } from "../terminal/XtermSurface";
import "@xterm/xterm/css/xterm.css";

const CSI = "\u001b[";
const questions = [
  ["Question 1: Choose a destination", "", "  1. Orchard preview", "  2. Keep the current destination", "", "First question only: APPLE", "A longer description wraps across the narrow terminal. Nothing here reaches a worker."],
  ["Question 2: Choose a check", "", "  1. Verify the small change ✓", "  2. Check the full flow 🐝", "", "Second question only: CLOVER"],
  ["Question 3: Confirm the next step", "", "  1. Continue the existing task", "  2. Leave it waiting for input", "", "Third question only: HONEY"],
];
function screenBytes(question: number) {
  return new TextEncoder().encode(`${CSI}0m${CSI}H${CSI}2J${questions[question].join("\r\n")}`);
}

function repaintBytes(previous: number, next: number, columns: number) {
  // These fixed strings use ASCII, a single-width check mark, and a two-cell
  // bee (two UTF-16 units). This is fixture-specific, not a Unicode width API.
  const physicalRows = questions[previous].reduce((total, line) => total + Math.max(1, Math.ceil(line.length / columns)), 0);
  return new TextEncoder().encode(`\r${CSI}${physicalRows - 1}A${CSI}J${questions[next].join("\r\n")}`);
}

/** Synthetic ANSI only: not a capture of Claude's AskUser output or proof of that bug. */
export default function TerminalQuestionsFixture() {
  const mount = useRef<HTMLDivElement>(null);
  const surface = useRef<XtermSurface | undefined>(undefined);
  const [question, setQuestion] = useState(0);
  const [columns, setColumns] = useState(36);
  const [status, setStatus] = useState("Opening fixture");
  const [serialized, setSerialized] = useState<string>();
  const [busy, setBusy] = useState(false);
  const current = useRef({ question, columns });
  current.current = { question, columns };

  useEffect(() => {
    const renderer = new XtermSurface();
    renderer.observeGeometryOwnership(() => false);
    surface.current = renderer;
    renderer.open(mount.current!);
    let active = true;
    void renderer.restore({ sequence: 1, rows: 24, columns: current.current.columns, truncated: false, reason: "attached", bytes: screenBytes(current.current.question) })
      .then(() => { if (active) setStatus("Ready · synthetic screen 1"); });
    return () => { active = false; surface.current = undefined; renderer.dispose(); };
  }, []);

  async function paint(next: number, width: number, snapshot: boolean) {
    if (busy || !surface.current) return;
    const renderer = surface.current;
    setBusy(true);
    setStatus("Rendering synthetic screen");
    setSerialized(undefined);
    try {
      if (snapshot) await renderer.restore({ sequence: next + 1, rows: 24, columns: width, truncated: false, reason: "attached", bytes: screenBytes(next) });
      else {
        const bytes = repaintBytes(question, next, columns);
        // Split control sequences as a WebSocket/PTY stream is allowed to do.
        for (let offset = 0; offset < bytes.length; offset += 7) {
          await renderer.write(bytes.subarray(offset, offset + 7));
          if (surface.current !== renderer) return;
        }
      }
      if (surface.current !== renderer) return;
      setQuestion(next);
      setColumns(width);
      setStatus(`Ready · synthetic screen ${next + 1} · ${width} columns`);
    } catch (error) {
      if (surface.current === renderer) setStatus(`Fixture render failed: ${error instanceof Error ? error.message : "unknown error"}`);
    } finally { if (surface.current === renderer) setBusy(false); }
  }

  return <main style={{ padding: 20, maxWidth: 1100, margin: "auto" }}>
    <h1>Terminal question transitions</h1>
    <p>Synthetic relative-cursor repaint with wrapped text and split control sequences. No provider, worker, socket, or Hive. Passing this does not establish that Claude AskUser is fixed.</p>
    <div style={{ display: "flex", flexWrap: "wrap", gap: 12, marginBottom: 16 }}>
      <label>Canonical width <select value={columns} disabled={busy} onChange={(event) => void paint(question, Number(event.target.value), true)}>
        <option value={36}>Phone · 36 columns</option><option value={100}>Desktop · 100 columns</option>
      </select></label>
      <button disabled={busy || question === questions.length - 1} onClick={() => void paint(question + 1, columns, false)}>Next question</button>
      <button disabled={busy} onClick={() => void paint(question, columns, true)}>Replay canonical snapshot</button>
      <button disabled={busy} onClick={() => void paint(0, columns, true)}>Reset fixture</button>
      <button disabled={busy} onClick={() => setSerialized(surface.current?.serialize())}>Inspect fixture buffer</button>
    </div>
    <p role="status">{status}</p>
    <div style={{ overflowX: "auto", maxWidth: "100%" }}><div ref={mount} style={{ width: columns * 8.45 + 24, height: 430, background: "#101a15" }} /></div>
    {serialized !== undefined && <pre aria-label="Synthetic buffer contents" style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{JSON.stringify(serialized)}</pre>}
  </main>;
}
