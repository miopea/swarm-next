import type { DecisionRequest } from "../api";

export function waitingForClarification(decision: DecisionRequest): boolean {
  return decision.state === "pending" && decision.clarification?.next_move === "requester";
}

/** Pending permission is not necessarily an actionable operator question. */
export function needsOperatorDecision(decision: DecisionRequest): boolean {
  return decision.state === "pending" && !waitingForClarification(decision);
}
