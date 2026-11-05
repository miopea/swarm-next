import type { DecisionRequest } from "../api";

export function waitingForClarification(decision: DecisionRequest): boolean {
  return decision.state === "pending" && decision.clarification?.next_move === "requester";
}

/**
 * Pending permission is not necessarily an actionable operator question. A
 * command card Queen may grant is hers until she grants it or hands it on
 * (ADR 0158), so it is not counted as theirs.
 */
export function needsOperatorDecision(decision: DecisionRequest): boolean {
  return decision.state === "pending" && !waitingForClarification(decision) && !decision.awaiting_queen;
}
