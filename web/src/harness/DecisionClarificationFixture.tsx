import { useEffect, useState } from "react";
import type { DecisionClarification, DecisionRequest, Worker } from "../api";
import { applyColorTheme, type ColorTheme } from "../brand/theme";
import DecisionInbox from "../decisions/DecisionInbox";

/** Local fictional state only. No transport, worker, or operator request is touched. */
export default function DecisionClarificationFixture() {
  const [theme, setTheme] = useState<ColorTheme>("light");
  const [history, setHistory] = useState<DecisionClarification[]>([]);
  const [failNext, setFailNext] = useState(false);
  const [noPreference, setNoPreference] = useState(false);
  const [finalChoice, setFinalChoice] = useState("");
  useEffect(() => applyColorTheme(theme), [theme]);
  const waiting = history.some((round) => round.reply === null && round.delivery_state !== "cancelled");
  function choose(value: string) {
    setFinalChoice(value);
    setHistory((rounds) => rounds.map((round) => round.reply === null ? { ...round, delivery_state: "cancelled" } : round));
  }
  const latestReplyAt = history.reduce<number | null>((latest, round) => round.replied_at === null ? latest : Math.max(latest ?? 0, round.replied_at), null);
  const outstanding = history.find(round => round.reply === null && round.delivery_state !== "cancelled");
  const decision: DecisionRequest = {
    id: "fictional-decision", hive_id: "fictional-hive", requesting_worker_id: "petal", task_id: null,
    kind: "input", urgency: "normal", title: "Should the export wait for its missing source?",
    summary: noPreference ? "Choose how to handle this fictional export; Petal has no preference." : "Petal recommends pausing this export until the source is available.",
    reason: "The source has not arrived.", risk: "Partial results could be mistaken for the complete export.",
    evidence: "Fictional export fixture only.", suggested_action: noPreference ? "" : "Wait for the source",
    allowed_actions: ["Wait for the source", "Use partial results"], deadline: null,
    state: finalChoice ? "resolved" : "pending", resolution_action: finalChoice || null,
    resolution_note: "", resolved_by_operator_id: finalChoice ? "fictional-operator" : null,
    created_at: 1, updated_at: 1, resolved_at: finalChoice ? 2 : null, delivery_state: null,
    clarification: history.length ? { round_count: history.length,
      waiting_clarification_id: outstanding?.id ?? null, delivery_state: outstanding?.delivery_state ?? null,
      latest_reply_at: latestReplyAt, next_move: finalChoice ? "none" : waiting ? "requester" : "operator" } : null,
  };
  return <main className="attention-workspace">
    <h1>Fictional Needs You clarification</h1>
    <p>No Hive request or worker message is sent.</p>
    <details><summary>Fixture controls</summary>
      <label><input type="checkbox" checked={noPreference} onChange={event => setNoPreference(event.target.checked)} />No meaningful preference</label>
      <button type="button" onClick={() => setTheme(theme === "light" ? "dark" : "light")}>Switch to {theme === "light" ? "dark" : "light"} theme</button>
      <label><input type="checkbox" checked={failNext} onChange={(event) => setFailNext(event.target.checked)} />Fail the next question send</label>
      <button type="button" disabled={!waiting || Boolean(finalChoice)} onClick={() => setHistory(rounds => rounds.map(round => round.reply === null ? {
        ...round, delivery_state: "uncertain", delivery_claim_id: crypto.randomUUID(), delivery_session_id: "fictional-session",
      } : round))}>Simulate unconfirmed delivery</button>
      <button type="button" disabled={!waiting || Boolean(finalChoice)} onClick={() => setHistory((rounds) => rounds.map((round) => round.reply === null ? {
        ...round, reply: "Petal checked the export path. Pausing the export avoids incomplete results; it does not pause the rest of your workers. I recommend waiting for the missing source, then retrying this export.",
        replied_at: Math.floor(Date.now() / 1000), replying_worker_id: "queen", replying_session_id: "fictional-queen-session", delivery_state: "delivered",
      } : round))}>Simulate Queen reply</button>
    </details>
    <DecisionInbox decisions={[decision]} tasks={[]} busy={false}
      workers={[{ id: "petal", name: "Petal", workspace: "/fictional/orchard" }, { id: "queen", name: "Queen", workspace: "/fictional/queen" }] as Worker[]}
      onResolve={async (_decision, action) => choose(action)}
      onAnswer={async (_decision, answers) => choose(answers.Answer.join("; "))}
      onFetchClarifications={async () => history}
      onReconcileClarification={async request => {
        const previous = history.find(round => round.id === request.clarification_id);
        if (!previous || previous.delivery_claim_id !== request.claim_id || previous.delivery_session_id !== request.session_id) throw new Error("The fictional delivery changed. Refresh first.");
        const saved: DecisionClarification = { ...previous, delivery_state: request.choice === "retry" ? "queued" : "delivered" };
        setHistory(rounds => rounds.map(round => round.id === saved.id ? saved : round));
        return saved;
      }}
      onAskClarification={async (_decision, id, question) => {
          if (failNext) { setFailNext(false); throw new Error("The fictional connection failed. Your question is still here; try again."); }
          const saved: DecisionClarification = { id, question, decision_id: "fictional-decision", operator_id: "fictional-operator", asked_at: Math.floor(Date.now() / 1000), reply: null, replied_at: null, replying_worker_id: null, replying_session_id: null, delivery_state: "queued" };
          setHistory((rounds) => [...rounds, saved]);
          return saved;
        }} />
    {finalChoice && <p role="status">Fictional final answer: {finalChoice}</p>}
  </main>;
}
