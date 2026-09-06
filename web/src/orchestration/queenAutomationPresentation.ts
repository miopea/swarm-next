import type { QueenAutomationStatus } from "../api";

export function queenAutomationStateLabel(status: QueenAutomationStatus | undefined) {
  if (!status) return "Checking Queen…";
  if (status.state === "queued" || status.state === "delivering") return "Review queued";
  if (status.state === "running") return "Queen is reviewing work";
  if (status.state === "uncertain") return "Review needs attention";
  if (status.state === "completed" && status.outcome === "needs_operator") return "Queen needs you";
  if (status.state === "completed" && status.outcome === "incomplete") return "Review unfinished";
  if (status.state === "completed" && (status.queen_owned_count ?? 0) > 0) return "Queen has work remaining";
  if (status.state === "completed") return "Review run ended";
  return status.enabled ? "Watching for new work" : "Manual review only";
}

export function queenAutomationCompactLabel(status: QueenAutomationStatus) {
  if (status.state === "completed" && status.outcome === "incomplete") return "Review unfinished";
  if (status.state === "uncertain") return "Automation needs review";
  if (status.state === "completed" && status.outcome === "needs_operator") return "Queen needs you";
  if (status.state === "queued" || status.state === "delivering") return "Review queued";
  if (status.state === "running") return "Reviewing work";
  if (status.state === "completed" && (status.queen_owned_count ?? 0) > 0) return "Queen work remaining";
  return status.enabled ? "Automation on" : "Automation off";
}

/**
 * Where this wording is being read, so it can answer that surface's question.
 *
 * The same sentence appeared in Queen's terminal, the settings panel, and the
 * Needs-you card at once. Each is asking something different — "what is true of
 * the terminal I am looking at", "how does this work and what can I change",
 * "what wants me and what do I do" — and one sentence cannot answer three
 * questions, so it answered none of them well.
 */
export type QueenAutomationSurface = "terminal" | "settings" | "attention";

export function queenAutomationStateDetail(
  status: QueenAutomationStatus | undefined,
  surface: QueenAutomationSurface = "settings",
) {
  if (!status) return "Loading durable automation state.";
  if (status.waiting_reason) return status.waiting_reason;
  if (status.state === "running") return `${status.actionable_count} actionable item${status.actionable_count === 1 ? "" : "s"} in this review.`;
  if (status.state === "uncertain") {
    if (surface === "terminal") return "Swarm could not confirm its last review reached this terminal. It is above if it arrived.";
    if (surface === "attention") return "Swarm could not confirm the last review reached Queen. Check her terminal, then resume it.";
    return "Delivery was interrupted before Swarm could confirm completion. Retry resumes this same review after you check Queen's terminal.";
  }
  if (status.state === "completed" && status.outcome === "needs_operator") {
    if (surface === "terminal") return "Queen is waiting on your answer to a request she filed.";
    if (surface === "attention") return "Queen filed a request and stopped. Open her to resolve it.";
    return "Open Queen when you are ready to resolve her decision.";
  }
  if (status.state === "completed") {
    if (status.outcome === "incomplete") return "Queen's turn ended without current evidence covering her outstanding work. Recovery remains with Queen; see Queues for what still needs attention.";
    const count = status.queen_owned_count;
    if (count != null && count > 0) return `The run ended; ${count} task${count === 1 ? " still needs" : "s still need"} Queen to route, reassess or review. See Queues for ownership and blockers.`;
    if (count === 0) return "The run ended. No current task names Queen as next owner; workers, releases or external holds may still have work.";
    return "The run ended. This server has not supplied current Queen ownership; check Queues before treating work as handled.";
  }
  if (status.enabled) return `${status.actionable_count} actionable item${status.actionable_count === 1 ? "" : "s"}; new durable changes trigger a review.`;
  return `${status.actionable_count} actionable item${status.actionable_count === 1 ? "" : "s"}; nothing runs automatically.`;
}

export function queenAutomationStateTone(status: QueenAutomationStatus | undefined) {
  if (status?.state === "uncertain" || (status?.state === "completed" && status.outcome === "needs_operator")) return "offline";
  if (status?.state === "running") return "online";
  return "waiting";
}

/**
 * Whether Queen's automation needs the operator.
 *
 * `needs_operator` is a claim about something they can act on, so it only holds
 * while one of her requests is actually pending. Without that check the control
 * room told the operator she had "filed a request and stopped" when she had
 * filed nothing — a card that reappeared on every run, said to open her, and
 * had nothing behind it when they did.
 *
 * `uncertain` is different and still stands on its own: a delivery nobody could
 * confirm is a real stall whether or not anything was filed.
 */
export function queenAutomationNeedsAttention(
  status: QueenAutomationStatus | undefined,
  queenRequestPending = false,
  coveredBySpecificDecision = false,
) {
  // A pending decision is not evidence that an interrupted delivery recovered.
  if (status?.state === "uncertain") return true;
  return status?.state === "completed"
    && status.outcome === "needs_operator"
    && queenRequestPending
    && !coveredBySpecificDecision;
}
