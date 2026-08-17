import type { QueenAutomationStatus } from "../api";

export function queenAutomationStateLabel(status: QueenAutomationStatus | undefined) {
  if (!status) return "Checking Queen…";
  if (status.state === "queued") return "Review queued";
  if (status.state === "delivering") return "Sending work to Queen";
  if (status.state === "running") return "Queen is reviewing work";
  if (status.state === "uncertain") return "Review needs attention";
  if (status.state === "completed" && status.outcome === "needs_operator") return "Queen needs you";
  if (status.state === "completed" && status.outcome === "no_action") return "Nothing needed routing";
  if (status.state === "completed") return "Review complete";
  return status.enabled ? "Watching for new work" : "Manual review only";
}

export function queenAutomationCompactLabel(status: QueenAutomationStatus) {
  if (status.state === "uncertain") return "Automation needs review";
  if (status.state === "completed" && status.outcome === "needs_operator") return "Queen needs you";
  if (status.state === "queued") return "Review queued";
  if (status.state === "delivering" || status.state === "running") return "Reviewing work";
  return status.enabled ? "Automation on" : "Automation off";
}

export function queenAutomationStateDetail(status: QueenAutomationStatus | undefined) {
  if (!status) return "Loading durable automation state.";
  if (status.waiting_reason) return status.waiting_reason;
  if (status.state === "running") return `${status.actionable_count} actionable item${status.actionable_count === 1 ? "" : "s"} in this review.`;
  if (status.state === "uncertain") return "Delivery was interrupted before Swarm could confirm completion. Retry resumes this same review after you check Queen's terminal.";
  if (status.state === "completed" && status.outcome === "needs_operator") return "Open Queen when you are ready to resolve her decision.";
  if (status.state === "completed") return "The latest bounded review ended safely.";
  if (status.enabled) return `${status.actionable_count} actionable item${status.actionable_count === 1 ? "" : "s"}; new durable changes trigger a review.`;
  return `${status.actionable_count} actionable item${status.actionable_count === 1 ? "" : "s"}; nothing runs automatically.`;
}

export function queenAutomationStateTone(status: QueenAutomationStatus | undefined) {
  if (status?.state === "uncertain" || (status?.state === "completed" && status.outcome === "needs_operator")) return "offline";
  if (status?.state === "running" || status?.state === "delivering") return "online";
  return "waiting";
}

export function queenAutomationNeedsAttention(status: QueenAutomationStatus | undefined) {
  return status?.state === "uncertain"
    || (status?.state === "completed" && status.outcome === "needs_operator");
}
