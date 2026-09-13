import { useCallback, useEffect, useId, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { refusedNativeAnswerNotice } from "./refusedNativeAnswer";

import type { DecisionClarification, DecisionRequest, DecisionSurface, Task, TaskActivityPage, Worker } from "../api";
import DecisionClarificationPanel from "./DecisionClarificationPanel";
import type { ReconcileClarification } from "./ClarificationRecovery";
import { useDecisionClarifications, type ReadClarifications } from "./useDecisionClarifications";
import { needsOperatorDecision, waitingForClarification } from "./decisionAttention";
import BeeMascot from "../brand/BeeMascot";
import DecisionInterview from "./DecisionInterview";
import LongText from "./LongText";
import WorkActivity from "./WorkActivity";
import { useVisiblePolling } from "../runtime/useVisiblePolling";

/// The repository a person recognises, out of an absolute workspace path.
function repoName(workspace: string) {
  const trimmed = workspace.replace(/\/+$/, "");
  return trimmed.slice(trimmed.lastIndexOf("/") + 1) || trimmed;
}

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
  coordinatorUnavailable?: boolean;
  /**
   * Rendered BELOW the requests, for panels that are not asking anything.
   *
   * Everything in `attentionCards` sits above the list and therefore claims to
   * outrank it. Queued briefings do not: their own copy says "Nothing is wrong
   * with these", and they were rendering above questions the operator has to
   * answer before work can continue.
   */
  trailingCards?: ReactNode;
  onOpenTask?: (taskId: string) => void;
  onFetchActivity?: (signal: AbortSignal) => Promise<TaskActivityPage>;
  onResolve: (decision: DecisionRequest, action: string, note: string, surface: DecisionSurface) => Promise<void>;
  onAnswer?: (decision: DecisionRequest, answers: Record<string, string[]>, note: string) => Promise<void>;
  onFetchClarifications?: ReadClarifications;
  onReconcileClarification?: ReconcileClarification;
  onAskClarification?: (decision: DecisionRequest, id: string, question: string) => Promise<DecisionClarification>;
};

export default function DecisionInbox({ decisions, tasks, workers, busy, focusDecisionId, focusRequest, additionalPendingCount = 0, attentionCards, coordinatorUnavailable = false, trailingCards, onOpenTask, onFetchActivity, onResolve, onAnswer, onFetchClarifications, onAskClarification, onReconcileClarification }: Props) {
  const [view, setView] = useState<"attention" | "activity">("attention");
  const clarifications = useDecisionClarifications(decisions, view === "attention" ? onFetchClarifications : undefined);
  const tabId = useId();
  const attentionTab = useRef<HTMLButtonElement>(null);
  const activityTab = useRef<HTMLButtonElement>(null);
  const pendingNavigation = useRef(false);
  const [showResolved, setShowResolved] = useState(false);
  const [notes, setNotes] = useState<Record<string, string>>({});
  const [dismissConfirmId, setDismissConfirmId] = useState<string>();
  const [speakingId, setSpeakingId] = useState<string>();
  const [spoken, setSpoken] = useState<Record<string, string>>({});
  const [submissionErrors, setSubmissionErrors] = useState<Record<string, string>>({});
  const [submittingIds, setSubmittingIds] = useState<Set<string>>(new Set());
  const inFlight = useRef(new Set<string>());
  const currentDecisions = useRef(decisions);
  currentDecisions.current = decisions;
  const [activity, setActivity] = useState<TaskActivityPage>();
  const [activityLoading, setActivityLoading] = useState(false);
  const [activityFailed, setActivityFailed] = useState(false);
  const taskNames = useMemo(() => new Map(tasks.map((task) => [task.id, task.title])), [tasks]);
  const workerNames = useMemo(() => new Map(workers.map((worker) => [worker.id, worker.name])), [workers]);
  // Which repository this is about. The operator could not tell from the card:
  // the only mention of it was inside the evidence prose, and on a Queen-raised
  // approval the requester's own workspace is the Queen directory, not the repo
  // the decision concerns. So the linked task wins, and the requester is the
  // fallback for a decision with no task.
  const taskRepos = useMemo(() => new Map(tasks.map((task) => [task.id, task.workspace])), [tasks]);
  const workerRepos = useMemo(() => new Map(workers.map((worker) => [worker.id, worker.workspace])), [workers]);
  // A pending card the operator can already see keeps its place. The server
  // orders decisions newest first, so without this an arrival during an
  // ordinary refresh is inserted ABOVE the card being read and shoves it down
  // a whole card height — between reading an action and pressing it. Scroll
  // and focus were held still earlier; the list itself was still reflowing.
  //
  // New arrivals land at the bottom and announce themselves through the count.
  // Resolved history keeps the server's order, which is the useful one there.
  const pendingOrder = useRef<string[]>([]);
  const visible = useMemo(() => {
    const shown = decisions.filter((decision) => showResolved || decision.state === "pending");
    const stillPending = new Set(
      shown.filter((decision) => decision.state === "pending").map((decision) => decision.id),
    );
    pendingOrder.current = pendingOrder.current.filter((id) => stillPending.has(id));
    const ordered = new Set(pendingOrder.current);
    for (const decision of shown) {
      if (decision.state === "pending" && !ordered.has(decision.id)) {
        pendingOrder.current.push(decision.id);
        ordered.add(decision.id);
      }
    }
    const positions = new Map(pendingOrder.current.map((id, index) => [id, index]));
    const place = (decision: DecisionRequest) =>
      decision.state === "pending" ? positions.get(decision.id)! : Number.MAX_SAFE_INTEGER;
    return [...shown].sort((first, second) => place(first) - place(second));
  }, [decisions, showResolved]);
  const pending = decisions.filter(needsOperatorDecision).length;
  const waitingReplies = decisions.filter(waitingForClarification).length;
  const pendingTotal = pending + additionalPendingCount;

  useEffect(() => {
    // Draft UI state belongs to pending requests, not every request ever seen
    // during a multi-day browser session. Never carry a resolved answer forward.
    const pendingIds = new Set(decisions.filter((decision) => decision.state === "pending").map((decision) => decision.id));
    const prune = (previous: Record<string, string>) => {
      const entries = Object.entries(previous).filter(([id]) => pendingIds.has(id));
      return entries.length === Object.keys(previous).length ? previous : Object.fromEntries(entries);
    };
    setNotes(prune);
    setSpoken(prune);
    setSubmissionErrors(prune);
    setSpeakingId((id) => id && pendingIds.has(id) ? id : undefined);
    setDismissConfirmId((id) => id && pendingIds.has(id) ? id : undefined);
  }, [decisions]);

  async function submitDecision(decision: DecisionRequest, action: () => Promise<void>) {
    if (busy || inFlight.current.has(decision.id)) return;
    inFlight.current.add(decision.id);
    setSubmittingIds(new Set(inFlight.current));
    setSubmissionErrors(current => ({ ...current, [decision.id]: "" }));
    try {
      await action();
      setSpeakingId(current => current === decision.id ? undefined : current);
    } catch (error) {
      if (currentDecisions.current.some(current => current.id === decision.id && current.state === "pending")) {
        setSubmissionErrors(current => ({ ...current, [decision.id]: error instanceof Error ? error.message : "Your answer could not be sent. It is still here to retry." }));
      }
    } finally {
      inFlight.current.delete(decision.id);
      setSubmittingIds(new Set(inFlight.current));
    }
  }

  function submitAnswer(decision: DecisionRequest, answers: Record<string, string[]>, note: string) {
    return submitDecision(decision, async () => {
      if (!onAnswer) throw new Error("Answers cannot be sent from this view. Your text has been kept.");
      await onAnswer(decision, answers, note);
    });
  }

  // Navigation owns one focus request, including when its data arrives later.
  // Refreshes cannot create another request or pull the operator out of Activity.
  useEffect(() => {
    pendingNavigation.current = Boolean(focusDecisionId);
    if (focusDecisionId) setView("attention");
  }, [focusDecisionId, focusRequest]);

  useEffect(() => {
    if (!pendingNavigation.current || view !== "attention") return;
    if (!focusDecisionId) return;
    if (!showResolved && decisions.some((decision) => decision.id === focusDecisionId && decision.state !== "pending")) {
      setShowResolved(true);
      return;
    }
    const frame = requestAnimationFrame(() => {
      const card = document.querySelector<HTMLElement>(`[data-decision-id="${CSS.escape(focusDecisionId)}"]`);
      if (!card) return;
      pendingNavigation.current = false;
      card?.scrollIntoView({ behavior: window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ? "instant" : "smooth", block: "center" });
      card?.focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(frame);
  }, [focusDecisionId, focusRequest, showResolved, view, decisions]);

  // Manual activation: arrows move focus; Enter/Space selects a tab. Merely
  // exploring tabs must not launch the asynchronous Activity read.
  const moveTabFocus = (event: KeyboardEvent<HTMLButtonElement>) => {
    let target: HTMLButtonElement | null;
    if (event.key === "Home") target = attentionTab.current;
    else if (event.key === "End") target = activityTab.current;
    else if (event.key === "ArrowLeft" || event.key === "ArrowRight") target = event.currentTarget === attentionTab.current ? activityTab.current : attentionTab.current;
    else return;
    event.preventDefault();
    target?.focus();
  };

  const readActivity = useCallback(async (signal: AbortSignal) => {
    if (!onFetchActivity) return;
    setActivityLoading(true);
    setActivityFailed(false);
    const timedOut = () => {
      if (signal.reason?.name === "TimeoutError") {
        setActivityFailed(true);
        setActivityLoading(false);
      }
    };
    signal.addEventListener("abort", timedOut, { once: true });
    try {
      const result = await onFetchActivity(signal);
      if (!signal.aborted) setActivity(result);
    } catch {
      if (!signal.aborted) setActivityFailed(true);
    } finally {
      signal.removeEventListener("abort", timedOut);
      if (!signal.aborted) setActivityLoading(false);
    }
  }, [onFetchActivity]);
  // Fetch on entry/return or explicit refresh, never on an interval. The shared
  // owner coalesces requests and cancels on hide, navigation, or disposal.
  const loadActivity = useVisiblePolling(readActivity, view === "activity" && Boolean(onFetchActivity), null);

  return (
    <section className="decision-inbox" aria-labelledby="decision-inbox-heading">
      <h3 id="decision-inbox-heading" className="sr-only">What needs you</h3>
      <div className="attention-tabs" role="tablist" aria-label="Attention workspace">
        <button ref={attentionTab} id={`${tabId}-attention`} role="tab" aria-controls={`${tabId}-panel`} tabIndex={view === "attention" ? 0 : -1} aria-selected={view === "attention"} onKeyDown={moveTabFocus} onClick={() => setView("attention")}>Needs you <small>{pendingTotal}</small></button>
        <button ref={activityTab} id={`${tabId}-activity`} role="tab" aria-controls={`${tabId}-panel`} tabIndex={view === "activity" ? 0 : -1} aria-selected={view === "activity"} onKeyDown={moveTabFocus} onClick={() => { pendingNavigation.current = false; if (view === "activity") void loadActivity(); else setView("activity"); }}>Activity</button>
      </div>
      <div id={`${tabId}-panel`} role="tabpanel" aria-labelledby={`${tabId}-${view}`}>
      {view === "activity" && <WorkActivity activity={activity} tasks={tasks} workers={workers} loading={activityLoading} failed={activityFailed} onRetry={() => void loadActivity()} onOpenTask={onOpenTask} />}
      {/* Keep pending forms owned by the inbox across tab switches. Hidden
          controls leave the accessibility tree; clarification reads remain
          disabled on Activity. Changed questions still reset their keyed form. */}
      <div hidden={view !== "attention"}>
      <div className="decision-inbox-intro">
        {waitingReplies > 0 && <p className="muted">{waitingReplies} {waitingReplies === 1 ? "conversation is" : "conversations are"} waiting for a reply, not an answer from you. Your decision options remain available.</p>}
        <label className="decision-history-toggle">
          <input type="checkbox" checked={showResolved} onChange={(event) => setShowResolved(event.target.checked)} />
          Show history
        </label>
      </div>

      {/*
        * Reserving the cards' space, per the operator's ruling on item 48's
        * second door. Each of these appears and disappears on live state, and
        * Queen's status changes on her own cycle, so any of them mounting
        * shoved the list below down by a whole card — the same jump that was
        * reported for an inserted decision, arriving through a different door.
        *
        * The reservation applies only while there is a list to disturb. With
        * nothing waiting there is nothing below to shove, and holding a card's
        * worth of blank above "Nothing needs your attention" would be the
        * oddity the alternative layout was rejected for.
        */}
      <div className={visible.length > 0 ? "decision-attention-cards reserved" : "decision-attention-cards"}>
        {attentionCards}
        {coordinatorUnavailable && <p role="status">Coordination status could not refresh. Showing last known work; it may have changed.</p>}
      </div>

      {visible.length === 0 && additionalPendingCount === 0 && !coordinatorUnavailable ? (
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
            const repo = (decision.task_id ? taskRepos.get(decision.task_id) : undefined)
              ?? workerRepos.get(decision.requesting_worker_id);
            const note = notes[decision.id] ?? "";
            const decisionBusy = busy || submittingIds.has(decision.id);
            // Combine only an exact displayed match. Authored commands and
            // identifiers are case-sensitive; a similar answer is not advice.
            const recommendedAction = decision.suggested_action !== "" && decision.state === "pending" && !decision.questions?.length
              ? decision.allowed_actions.find(action => humanize(action).trim() === humanize(decision.suggested_action).trim())
              : undefined;
            const recommendationId = `${tabId}-recommendation-${decision.id}`;
            return (
              <article className={`decision-card urgency-${decision.urgency} state-${decision.state}`} data-decision-id={decision.id} key={decision.id} tabIndex={-1}>
                <header>
                  <div className="decision-requester">
                    <span className="decision-bee"><BeeMascot expression={decision.urgency === "time_sensitive" ? "blocked" : "focused"} /></span>
                    <div>
                      <p className="eyebrow">{requester} · {kindLabel[decision.kind]}{repo ? <> · <span className="decision-repo" title={repo}>{repoName(repo)}</span></> : null}</p>
                      <h4>{decision.title}</h4>
                    </div>
                  </div>
                  <span className={`decision-urgency ${decision.state === "pending" ? decision.urgency : "settled"}`}>{decision.state === "withdrawn" ? "Withdrawn" : decision.state === "resolved" ? "Answered" : waitingForClarification(decision) ? "Waiting for reply" : decision.urgency === "time_sensitive" ? "Time-sensitive" : "When ready"}</span>
                </header>
                {/* What is being decided comes first and stays short. The
                    reason, risk and evidence are the argument behind it — on
                    the live inbox they ran to about five thousand characters
                    together — so they fold behind it rather than in front. */}
                {decision.summary ? <div className="decision-summary"><LongText text={decision.summary} label="the summary" foldAbove={300} /></div> : null}
                {/* ⚠️ ABOVE THE RECOMMENDATION AND BELOW THE SUMMARY. The
                    operator already believes they answered this; the first thing
                    to establish is that Swarm saw it and why it could not be
                    used. Buried under the risk fold it would be indistinguishable
                    from the silence this exists to end. */}
                {decision.state === "pending" && decision.refused_native_answer
                  ? <p className="decision-refused-answer" role="status">
                      {refusedNativeAnswerNotice(decision.refused_native_answer.reason)}
                    </p>
                  : null}
                {/* ⚠️ SAID BEFORE THEY ANSWER, NOT AFTER. The operator answered
                    decision 01a0939d in a worker's terminal and it never
                    cleared — it had no questions, so no capture could ever have
                    matched it, and nothing told them either way. "It never went
                    away." A refusal afterwards only explains; this prevents.
                    Only shown while pending and only when it is TRUE: an item
                    that cannot be answered in a terminal says nothing, because
                    the control room already works and a second instruction on
                    every item would be noise. */}
                {decision.state === "pending" && decision.terminal_answerable
                  ? <p className="decision-terminal-answerable" role="status">
                      You can answer this in {requester}'s terminal, or here.
                    </p>
                  : null}
                {decision.suggested_action && recommendedAction === undefined && <div className="decision-ask"><span>{requester} {decision.state === "pending" ? "recommends" : "recommended"}</span><LongText text={humanize(decision.suggested_action)} label="the recommendation" foldAbove={300} /></div>}
                {decision.suggested_action === "" && <div className="decision-ask"><span>{requester}'s view</span><p>No preference</p></div>}
                {decision.risk && <dl className="decision-context">
                  <div className="decision-risk"><dt>Risk</dt><dd><LongText text={decision.risk} label="the risk" foldAbove={300} /></dd></div>
                </dl>}
                {onFetchClarifications && onAskClarification && <DecisionClarificationPanel
                  requester={requester}
                  pending={decision.state === "pending"}
                  waiting={waitingForClarification(decision)}
                  roundCount={decision.clarification?.round_count ?? 0}
                  replyAvailable={decision.clarification?.next_move === "operator" && decision.clarification.latest_reply_at !== null}
                  workerNames={workerNames}
                  {...clarifications.forDecision(decision)}
                  onReload={() => clarifications.reload(decision.id)}
                  onReconcile={onReconcileClarification ? async request => {
                    const saved = await onReconcileClarification(request);
                    clarifications.reload(decision.id);
                    return saved;
                  } : undefined}
                  onAsk={async (id, question) => {
                    const saved = await onAskClarification(decision, id, question);
                    clarifications.reload(decision.id);
                    return saved;
                  }}
                />}
                {decision.state === "pending" && decision.questions?.length ? (
                  <div className="decision-resolution">
                    {/* An interview offers no buttons: the asker did not know
                        what to offer, which is why it asked. Dismissal stays
                        available, and needs a reason, so "hold for now" and
                        "stop asking me" cannot be recorded identically. */}
                    <DecisionInterview
                      questions={decision.questions}
                      busy={decisionBusy}
                      onAnswer={(answers, answerNote) => { setDismissConfirmId(undefined); void submitAnswer(decision, answers, answerNote); }}
                    />
                    <div className="decision-actions">
                      <button
                        type="button"
                        className="secondary-button decision-dismiss"
                        disabled={decisionBusy || !note.trim()}
                        title={note.trim() ? "Decline to answer, telling the worker why" : "Dismissing an interview needs a reason the worker can act on"}
                        onClick={() => { setDismissConfirmId(undefined); void submitDecision(decision, () => onResolve(decision, "dismissed", note, "inbox_dismiss")); }}
                      >Decline with a reason</button>
                      <label className="decision-dismiss-reason">
                        <span>Reason</span>
                        <input value={note} maxLength={4000} disabled={decisionBusy} placeholder="Why you are not answering now" onChange={(event) => setNotes((current) => ({ ...current, [decision.id]: event.target.value }))} />
                      </label>
                    </div>
                  </div>
                ) : decision.state === "pending" ? (
                  <div className="decision-resolution">
                    {decision.requested_command ? (
                      // THE COMMAND IS SHOWN BECAUSE ONE BUTTON BELOW GRANTS IT.
                      // Approving "the one contact formula-column test" is not
                      // approving a command you never saw, and a grant that
                      // compiled from prose the operator read loosely would be a
                      // worse trade than the block it removes. What is rendered
                      // here is the exact text that becomes the rule.
                      //
                      // Not truncated and not styled down: this is the part of
                      // the card that carries the consequence.
                      <div className="decision-command">
                        <p className="eyebrow">Command this would allow</p>
                        <pre><code>{decision.requested_command}</code></pre>
                        <small>
                          Grants permission for this exact command, once, for this worker only.
                          The worker still needs to run it. Permission expires when the task leaves
                          the board. Any other command is still refused.
                        </small>
                      </div>
                    ) : null}
                    <details className="decision-argument decision-note">
                      <summary>{note.trim() ? "Edit your note" : "Add an optional note"}</summary>
                      <label><span>Optional note</span><textarea value={note} maxLength={4000} disabled={decisionBusy} onChange={(event) => setNotes((current) => ({ ...current, [decision.id]: event.target.value }))} placeholder="Add context for the worker" /></label>
                    </details>
                    <div className="decision-actions">
                      {decision.allowed_actions.map((action) => (
                        <button key={action} type="button"
                          className={action === recommendedAction ? "primary-action decision-recommended-action" : "secondary-button"}
                          aria-describedby={action === recommendedAction ? recommendationId : undefined}
                          disabled={decisionBusy}
                          onClick={() => { setDismissConfirmId(undefined); void submitDecision(decision, () => onResolve(decision, action, note, "inbox_action")); }}>
                          {action === recommendedAction && <span id={recommendationId} className="decision-action-recommendation" aria-hidden="true">{requester} recommends</span>}
                          <span>{humanize(action)}</span>
                        </button>
                      ))}
                      {/* The buttons above are the asker's guesses. When none
                          of them is the answer, the answer is still the
                          operator's to give — pressing the closest one or
                          dismissing both lose it. */}
                      <button
                        type="button"
                        className="secondary-button"
                        disabled={decisionBusy}
                        aria-expanded={speakingId === decision.id}
                        aria-controls={`${tabId}-answer-${decision.id}`}
                        onClick={() => setSpeakingId(speakingId === decision.id ? undefined : decision.id)}
                      >{speakingId === decision.id ? "Never mind" : "Say something else"}</button>
                      <button
                        type="button"
                        className="secondary-button decision-dismiss"
                        disabled={decisionBusy}
                        title="Resolve this request without taking any proposed action"
                        onClick={() => {
                          if (dismissConfirmId !== decision.id) {
                            setDismissConfirmId(decision.id);
                            return;
                          }
                          setDismissConfirmId(undefined);
                          void submitDecision(decision, () => onResolve(decision, "dismissed", note, "inbox_dismiss"));
                        }}
                      >
                        {dismissConfirmId === decision.id ? "Confirm dismiss" : "Dismiss request"}
                      </button>
                    </div>
                    {speakingId === decision.id ? (
                      <div id={`${tabId}-answer-${decision.id}`} className="decision-own-words" role="group" aria-label="Answer in your own words">
                        <label>
                          <span>Tell the worker what to do instead</span>
                          <textarea
                            rows={3}
                            disabled={decisionBusy}
                            value={spoken[decision.id] ?? ""}
                            maxLength={4000}
                            placeholder="Describe the outcome you want instead"
                            onChange={(event) => setSpoken((current) => ({ ...current, [decision.id]: event.target.value }))}
                          />
                        </label>
                        <button
                          type="button"
                          className="primary-action"
                          disabled={decisionBusy || !(spoken[decision.id] ?? "").trim()}
                          onClick={() => {
                            void submitAnswer(decision, { Answer: [(spoken[decision.id] ?? "").trim()] }, note);
                          }}
                        >Send this instead</button>
                      </div>
                    ) : null}
                  </div>
                ) : decision.state === "withdrawn" ? (
                  <div className="decision-resolved"><p><strong>Withdrawn</strong>{decision.withdrawal_reason ? ` · ${decision.withdrawal_reason}` : ""}</p><span>No operator decision or approval was recorded.</span></div>
                ) : (
                  <div className="decision-resolved"><p><strong>{humanize(decision.resolution_action ?? "resolved")}</strong>{decision.resolution_note ? ` · ${decision.resolution_note}` : ""}</p><span className={`delivery-state ${decision.delivery_state ?? "recorded"}`}>{deliveryLabel(decision.delivery_state)}</span></div>
                )}
                {decision.state === "pending" && submissionErrors[decision.id] && <p className="field-error" role="alert">{submissionErrors[decision.id]}</p>}
                <details className="decision-argument">
                  <summary>Why, and what it rests on</summary>
                  {decision.task_id && <dl className="decision-context"><div><dt>Task</dt><dd>{onOpenTask ? <button type="button" className="decision-task-link" onClick={() => onOpenTask(decision.task_id!)}>{taskNames.get(decision.task_id) ?? "Linked task"}</button> : taskNames.get(decision.task_id) ?? "Linked task"}</dd></div></dl>}
                  {!!decision.linked_tasks?.length && <div>
                    <p className="eyebrow">Also waiting on this answer</p>
                    <ul>{decision.linked_tasks.map((link) => <li key={link.task_id}>
                      {onOpenTask ? <button type="button" className="decision-task-link" onClick={() => onOpenTask(link.task_id)}>{taskNames.get(link.task_id) ?? "Linked task"}</button> : taskNames.get(link.task_id) ?? "Linked task"}
                      {" · "}{link.reason}
                    </li>)}</ul>
                    <p>This does not extend command approval to these tasks.</p>
                  </div>}
                  <DecisionReason reason={decision.reason} />
                  {decision.evidence && <div><p className="eyebrow">Evidence</p><LongText text={decision.evidence} label="the evidence" /></div>}
                </details>
              </article>
            );
          })}
        </div>
      )}
      </div>
      </div>
      {trailingCards}
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
  // Older actions are bare lower-case machine keys. Authored action sentences
  // can contain exact commands, identifiers and case-sensitive settings; their
  // wording is evidence for a decision and must not be rewritten for display.
  return /^[a-z]+(?:_[a-z]+)*$/.test(value)
    ? value.replaceAll("_", " ").replace(/^./, (letter) => letter.toUpperCase())
    : value;
}

const queenSectionLabels: Record<string, string> = {
  "dispatchable now": "Can proceed now",
  "blocked on your ruling": "Needs your ruling",
  "not delegable / out of scope this run": "Cannot proceed in this review",
};

function DecisionReason({ reason }: { reason: string }) {
  const heading = /(DISPATCHABLE NOW|BLOCKED ON YOUR RULING|NOT DELEGABLE \/ OUT OF SCOPE THIS RUN)\s*\((\d+)\)/gi;
  const matches = [...reason.matchAll(heading)];
  // No recognised structure to lift out, so it is shown as written: the author
  // put paragraphs in it, and collapsing them is what produced a block nobody
  // could read.
  if (!matches.length) return <div className="decision-reason"><LongText text={reason} label="the reason" /></div>;
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
