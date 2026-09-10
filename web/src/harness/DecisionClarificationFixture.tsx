import { useEffect, useState } from "react";
import type { DecisionClarification } from "../api";
import { applyColorTheme, type ColorTheme } from "../brand/theme";
import DecisionClarificationPanel from "../decisions/DecisionClarificationPanel";

/** Local fictional state only. No transport, worker, or operator request is touched. */
export default function DecisionClarificationFixture() {
  const [theme, setTheme] = useState<ColorTheme>("light");
  const [history, setHistory] = useState<DecisionClarification[]>([]);
  const [failNext, setFailNext] = useState(false);
  const [finalChoice, setFinalChoice] = useState("");
  useEffect(() => applyColorTheme(theme), [theme]);
  const waiting = history.some((round) => round.reply === null && round.delivery_state !== "cancelled");
  function choose(value: string) {
    setFinalChoice(value);
    setHistory((rounds) => rounds.map((round) => round.reply === null ? { ...round, delivery_state: "cancelled" } : round));
  }
  return <main className="attention-workspace">
    <h1>Fictional Needs You clarification</h1>
    <p>No Hive request or worker message is sent.</p>
    <details><summary>Fixture controls</summary>
      <button type="button" onClick={() => setTheme(theme === "light" ? "dark" : "light")}>Switch to {theme === "light" ? "dark" : "light"} theme</button>
      <label><input type="checkbox" checked={failNext} onChange={(event) => setFailNext(event.target.checked)} />Fail the next question send</label>
      <button type="button" disabled={!waiting || Boolean(finalChoice)} onClick={() => setHistory((rounds) => rounds.map((round) => round.reply === null ? {
        ...round, reply: "Petal checked the export path. Pausing the export avoids incomplete results; it does not pause the rest of your workers. I recommend waiting for the missing source, then retrying this export.",
        replied_at: Math.floor(Date.now() / 1000), replying_worker_id: "queen", replying_session_id: "fictional-queen-session", delivery_state: "delivered",
      } : round))}>Simulate Queen reply</button>
    </details>
    <article className="decision-card">
      <p className="eyebrow">Petal · Fictional orchard</p>
      <h2>Should the export wait for its missing source?</h2>
      <p>Petal recommends pausing this export until the source is available.</p>
      <DecisionClarificationPanel requester="Petal" pending={!finalChoice} waiting={waiting} history={history}
        workerNames={new Map([["queen", "Queen"], ["petal", "Petal"]])} onReload={() => {}}
        onAsk={async (id, question) => {
          if (failNext) { setFailNext(false); throw new Error("The fictional connection failed. Your question is still here; try again."); }
          const saved: DecisionClarification = { id, question, decision_id: "fictional-decision", operator_id: "fictional-operator", asked_at: Math.floor(Date.now() / 1000), reply: null, replied_at: null, replying_worker_id: null, replying_session_id: null, delivery_state: "queued" };
          setHistory((rounds) => [...rounds, saved]);
          return saved;
        }} />
      {!finalChoice ? <div className="decision-actions">
        <button type="button" onClick={() => choose("Wait for the source")}>Wait for the source</button>
        <button type="button" onClick={() => choose("Use partial results")}>Use partial results</button>
      </div> : <p role="status">Fictional final answer: {finalChoice}</p>}
    </article>
  </main>;
}
