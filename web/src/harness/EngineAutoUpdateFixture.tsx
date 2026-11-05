import type { WorkerEngineAutoUpdateView, WorkerEngineUpdateAttempt } from "../api";
import { refusalSentence, type EngineUpdateAttention } from "../runtime/engineAutoUpdate";
import { engineUpdateHistory } from "../runtime/engineUpdateHistory";
import WorkerEngineAttentionCard from "../runtime/WorkerEngineAttentionCard";
import WorkerEngineAutoUpdate from "../settings/WorkerEngineAutoUpdate";

/**
 * The worker engine card's automatic update (ADR 0156) and its Needs you item,
 * in the state ?state= names: "waiting" (on, held up by a worker mid-turn),
 * "off" (switched off), "updated" (the last update applied itself),
 * "partway" (a try stopped one worker and not the other), or "day" (an update
 * has waited more than a day). Nothing is stopped.
 */
export default function EngineAutoUpdateFixture() {
  const state = new URLSearchParams(window.location.search).get("state") ?? "waiting";
  const now = Math.floor(Date.now() / 1000);
  const ledgerMidTurn = { reason: "turn_in_progress", worker: "Ledger", since: now - 12 * 60 };
  const view: WorkerEngineAutoUpdateView = {
    enabled: state !== "off",
    waiting_since: state === "updated" ? null : now - (state === "day" ? 26 : 1) * 3600,
    refusal: state === "waiting" || state === "day"
      ? ledgerMidTurn
      : state === "partway" ? { reason: "stopped_partway", worker: "Pantry", since: now - 5 * 60 } : null,
    last_update_workers: state === "updated"
      ? [{ name: "Ledger", status: "back" }, { name: "Pantry", status: "back" }]
      : [],
  };
  const updated: WorkerEngineUpdateAttempt = {
    id: "fixture-attempt",
    started_at: now - 40 * 60,
    from_version: "1.9.0",
    to_version: "1.9.1",
    stopped_sessions: 2,
    initiated: "automatic",
    outcome: "succeeded",
    detail: "",
    finished_at: now - 39 * 60,
  };
  const attention: EngineUpdateAttention | null = state === "partway"
    ? {
      kind: "stopped_partway",
      detail: "Stopped Ledger while resting, then could not stop Pantry. The engine was not replaced, so the stopped workers come back once it is.",
      stopped: ["Ledger"],
    }
    : state === "day"
      ? { kind: "waiting", hours: 26, reason: refusalSentence(ledgerMidTurn), enabled: true }
      : null;
  return (
    <div style={{ display: "grid", gap: 16, maxWidth: 760 }}>
      {attention ? (
        <div className="attention-workspace">
          <WorkerEngineAttentionCard attention={attention} onReview={() => undefined} />
        </div>
      ) : null}
      <article className={`runtime-subsystem-card runtime-subsystem-${state === "updated" ? "current" : "restart"}`} aria-label="Worker engine status">
        <header>
          <div><span className="runtime-component-name">Worker engine</span><strong>{state === "updated" ? "Up to date" : "Update waiting"}</strong></div>
          <span className={`runtime-status-badge ${state === "updated" ? "current" : "restart"}`}>{state === "updated" ? "Current" : "Restart required"}</span>
        </header>
        <WorkerEngineAutoUpdate
          operatorToken="fixture-only"
          view={view}
          behind={state !== "updated"}
          history={state === "updated"
            ? engineUpdateHistory(updated, now, view.last_update_workers)
            : null}
        />
      </article>
    </div>
  );
}
