import { useEffect, useState } from "react";
import type { CommandCardScript } from "../api";

export type FetchDecisionScript = (decisionId: string, signal: AbortSignal) => Promise<CommandCardScript | null>;

type Loaded = { kind: "loading" } | { kind: "loaded"; script: CommandCardScript | null } | { kind: "failed" };

/**
 * The script a command card runs, as Swarm kept it when the card was filed
 * (ADR 0187). Read when the operator opens it, so the list of cards does not
 * carry every script. The text is the copy the card's sha256 pins: an Allow
 * lets exactly these bytes run, or nothing.
 */
export default function CommandScript({ decisionId, onFetchScript }: {
  decisionId: string;
  onFetchScript?: FetchDecisionScript;
}) {
  const [open, setOpen] = useState(false);
  const [loaded, setLoaded] = useState<Loaded>({ kind: "loading" });

  useEffect(() => {
    if (!open || !onFetchScript) return;
    const request = new AbortController();
    setLoaded({ kind: "loading" });
    onFetchScript(decisionId, request.signal).then(
      (script) => { if (!request.signal.aborted) setLoaded({ kind: "loaded", script }); },
      () => { if (!request.signal.aborted) setLoaded({ kind: "failed" }); },
    );
    return () => request.abort();
  }, [open, decisionId, onFetchScript]);

  if (!onFetchScript) return null;
  return (
    <details className="decision-command-script-text" onToggle={(event) => setOpen(event.currentTarget.open)}>
      <summary>Read the script</summary>
      {loaded.kind === "loading" ? <p className="muted" role="status">Loading the script…</p>
        : loaded.kind === "failed" ? <p className="muted">The script could not be loaded here. Read it at its path.</p>
          : loaded.script?.text !== undefined ? <ScriptText text={loaded.script.text} />
            : <p className="decision-command-script-withheld">{loaded.script?.withheld ?? "This card runs no script."}</p>}
    </details>
  );
}

function ScriptText({ text }: { text: string }) {
  const lines = text.split("\n").length - (text.endsWith("\n") ? 1 : 0);
  return (
    <>
      <p className="muted">{lines} {lines === 1 ? "line" : "lines"}, as Swarm read them when this was filed. Allow lets these exact bytes run, or nothing.</p>
      <pre tabIndex={0} aria-label="The script's text"><code>{text}</code></pre>
    </>
  );
}
