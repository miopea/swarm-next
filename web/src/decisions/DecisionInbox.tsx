import { useEffect, useMemo, useState, type ReactNode } from "react";

import type { DecisionRequest, DecisionSurface, Task, TaskActivityPage, Worker } from "../api";
import BeeMascot from "../brand/BeeMascot";
import WorkActivity from "./WorkActivity";

const kindLabel = {
  input: "Input",
  approval: "Approval",
  credentials: "Credentials",
  conflict: "Conflict",
  help: "Help",
};

type Props = {
  decisions: DecisionRequest[];
  tasks: Task[];
  workers: Worker[];
  busy: boolean;
  focusDecisionId?: string;
  focusRequest?: number;
  additionalPendingCount?: number;
  attentionCards?: ReactNode;
  onOpenTask?: (taskId: string) => void;
  onFetchActivity?: () => Promise<TaskActivityPage>;
  onResolve: (decision: DecisionRequest, action: string, note: string, surface: DecisionSurface) => Promise<void>;
};

export default function DecisionInbox({ decisions, tasks, workers, busy, focusDecisionId, focusRequest, additionalPendingCount = 0, attentionCards, onOpenTask, onFetchActivity, onResolve }: Props) {
  const [view, setView] = useState<"attention" | "activity">("attention");
  const [showResolved, setShowResolved] = useState(false);
  const [notes, setNotes] = useState<Record<string, string>>({});
  const [dismissConfirmId, setDismissConfirmId] = useState<string>();
  const [activity, setActivity] = useState<TaskActivityPage>();
  const [activityLoading, setActivityLoading] = useState(false);
  const [activityFailed, setActivityFailed] = useState(false);
  const taskNames = useMemo(() => new Map(tasks.map((task) => [task.id, task.title])), [tasks]);
  const workerNames = useMemo(() => new Map(workers.map((worker) => [worker.id, worker.name])), [workers]);
  const visible = decisions.filter((decision) => showResolved || decision.state === "pending");
  const pending = decisions.filter((decision) => decision.state === "pending").length;
  const pendingTotal = pending + additionalPendingCount;

  useEffect(() => {
    if (!focusDecisionId) return;
    if (decisions.some((decision) => decision.id === focusDecisionId && decision.state === "resolved")) setShowResolved(true);
  }, [decisions, focusDecisionId]);

  // Deliberately not keyed on `decisions`. Navigation asking for a decision is
  // what should move the page, and `focusRequest` is how it asks; every live
  // refresh of the list is not. Keyed on the list, this scrolled and refocused
  // the card whenever anything in the Hive changed, which on a busy Hive moves
  // the card between the operator reading an action and clicking it.
  // `showResolved` stays because revealing history is what puts the card in the
  // document in the first place.
  useEffect(() => {
    if (!focusDecisionId) return;
    const frame = requestAnimationFrame(() => {
      const card = document.querySelector<HTMLElement>(`[data-decision-id="${CSS.escape(focusDecisionId)}"]`);
      card?.scrollIntoView({ behavior: "smooth", block: "center" });
      card?.focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(frame);
  }, [focusDecisionId, focusRequest, showResolved]);

  const loadActivity = async () => {
    if (!onFetchActivity) return;
    setActivityLoading(true);
    setActivityFailed(false);
    try { setActivity(await onFetchActivity()); }
    catch { setActivityFailed(true); }
    finally { setActivityLoading(false); }
  };

  useEffect(() => {
    if (view === "activity" && !activity && !activityLoading && !activityFailed) void loadActivity();
  }, [view, activity, activityLoading, activityFailed]);

  return (
    <section className="decision-inbox" aria-labelledby="decision-inbox-heading">
      <div className="attention-tabs" role="tablist" aria-label="Attention workspace">
        <button role="tab" aria-selected={view === "attention"} onClick={() => setView("attention")}>Needs you <small>{pendingTotal}</small></button>
        <button role="tab" aria-selected={view === "activity"} onClick={() => { setView("activity"); if (activity) void loadActivity(); }}>Activity</button>
      </div>
      {view === "activity" ? <WorkActivity activity={activity} tasks={tasks} workers={workers} loading={activityLoading} failed={activityFailed} onRetry={() => void loadActivity()} onOpenTask={onOpenTask} /> : <>
      <div className="decision-inbox-intro">
        <div>
          <p className="eyebrow">One calm queue</p>
          <h3 id="decision-inbox-heading">What needs you</h3>
          <p>Workers ask here instead of interrupting another terminal. Resolve the judgment; Swarm keeps the context.</p>
        </div>
        <label className="decision-history-toggle">
          <input type="checkbox" checked={showResolved} onChange={(event) => setShowResolved(event.target.checked)} />
          Show resolved
        </label>
      </div>

      {attentionCards}

      {visible.length === 0 && additionalPendingCount === 0 ? (
        <div className="decision-empty">
          <BeeMascot className="empty-bee" expression="available" />
          <p className="eyebrow">All clear</p>
          <h3>{pending === 0 ? "Nothing needs your attention" : "No matching requests"}</h3>
          <p>Routine worker activity stays quiet. Only judgment, credentials, conflicts, or requested help land here.</p>
        </div>
      ) : (
        <div className="decision-list">
          {visible.map((decision) => {
            const requester = workerNames.get(decision.requesting_worker_id) ?? "Worker";
            const note = notes[decision.id] ?? "";
            return (
              <article className={`decision-card urgency-${decision.urgency} state-${decision.state}`} data-decision-id={decision.id} key={decision.id} tabIndex={-1}>
                <header>
                  <div className="decision-requester">
                    <span className="decision-bee"><BeeMascot expression={decision.urgency === "time_sensitive" ? "blocked" : "focused"} /></span>
                    <div><p className="eyebrow">{requester} · {kindLabel[decision.kind]}</p><h4>{decision.title}</h4></div>
                  </div>
                  <span className={`decision-urgency ${decision.urgency}`}>{decision.urgency === "time_sensitive" ? "Time-sensitive" : "When ready"}</span>
                </header>
                <DecisionReason reason={decision.reason} />
                <dl className="decision-context">
                  {decision.task_id && <div><dt>Task</dt><dd>{onOpenTask ? <button type="button" className="decision-task-link" onClick={() => onOpenTask(decision.task_id!)}>{taskNames.get(decision.task_id) ?? "Linked task"}</button> : taskNames.get(decision.task_id) ?? "Linked task"}</dd></div>}
                  {decision.risk && <div><dt>Risk</dt><dd>{decision.risk}</dd></div>}
                  {decision.evidence && <div><dt>Evidence</dt><dd>{decision.evidence}</dd></div>}
                  <div><dt>Suggested</dt><dd>{decision.suggested_action}</dd></div>
                </dl>
                {decision.state === "pending" ? (
                  <div className="decision-resolution">
                    <label><span>Optional note</span><textarea value={note} maxLength={4000} onChange={(event) => setNotes((current) => ({ ...current, [decision.id]: event.target.value }))} placeholder="Add context for the worker" /></label>
                    <div className="decision-actions">
                      {decision.allowed_actions.map((action, index) => (
                        <button key={action} type="button" className={index === 0 ? "primary-action" : "secondary-button"} disabled={busy} onClick={() => { setDismissConfirmId(undefined); void onResolve(decision, action, note, "inbox_action"); }}>{humanize(action)}</button>
                      ))}
                      <button
                        type="button"
                        className="secondary-button decision-dismiss"
                        disabled={busy}
                        title="Resolve this request without taking any proposed action"
                        onClick={() => {
                          if (dismissConfirmId !== decision.id) {
                            setDismissConfirmId(decision.id);
                            return;
                          }
                          setDismissConfirmId(undefined);
                          void onResolve(decision, "dismissed", note, "inbox_dismiss");
                        }}
                      >
                        {dismissConfirmId === decision.id ? "Confirm dismiss" : "Dismiss request"}
                      </button>
                    </div>
                  </div>
                ) : (
                  <div className="decision-resolved"><p><strong>{humanize(decision.resolution_action ?? "resolved")}</strong>{decision.resolution_note ? ` · ${decision.resolution_note}` : ""}</p><span className={`delivery-state ${decision.delivery_state ?? "recorded"}`}>{deliveryLabel(decision.delivery_state)}</span></div>
                )}
              </article>
            );
          })}
        </div>
      )}
      </>}
    </section>
  );
}

function deliveryLabel(state: DecisionRequest["delivery_state"]): string {
  switch (state) {
    case "queued": return "Waiting for a quiet moment";
    case "dispatching": return "Sending now";
    case "delivered": return "Delivered to worker";
    case "uncertain": return "Delivery uncertain · worker can retrieve it";
    default: return "Recorded before delivery tracking";
  }
}
function humanize(value: string): string {
  return value.replaceAll("_", " ").replace(/^./, (letter) => letter.toUpperCase());
}

const queenSectionLabels: Record<string, string> = {
  "dispatchable now": "Can proceed now",
  "blocked on your ruling": "Needs your ruling",
  "not delegable / out of scope this run": "Cannot proceed in this review",
};

function DecisionReason({ reason }: { reason: string }) {
  const heading = /(DISPATCHABLE NOW|BLOCKED ON YOUR RULING|NOT DELEGABLE \/ OUT OF SCOPE THIS RUN)\s*\((\d+)\)/gi;
  const matches = [...reason.matchAll(heading)];
  if (!matches.length) return <p className="decision-reason">{reason}</p>;
  const preamble = reason.slice(0, matches[0].index).trim();
  return (
    <div className="decision-reason decision-reason-structured">
      {preamble ? <p>{preamble}</p> : null}
      <div className="decision-reason-sections">
        {matches.map((match, index) => {
          const start = (match.index ?? 0) + match[0].length;
          const end = matches[index + 1]?.index ?? reason.length;
          const items = reason.slice(start, end).trim().split(/\s+(?=\d+\.\s)/).map((item) => item.replace(/^\d+\.\s*/, "").trim()).filter(Boolean);
          return (
            <section key={`${match[1]}-${index}`} className={`decision-reason-section section-${index}`}>
              <header><strong>{queenSectionLabels[match[1].toLowerCase()] ?? humanize(match[1])}</strong><span>{match[2]}</span></header>
              <ol>{items.map((item, itemIndex) => <li key={`${itemIndex}-${item.slice(0, 28)}`}>{item}</li>)}</ol>
            </section>
          );
        })}
      </div>
    </div>
  );
}
