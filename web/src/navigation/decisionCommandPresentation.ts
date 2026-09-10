import type { DecisionRequest } from "../api";
import { waitingForClarification } from "../decisions/decisionAttention";

/** Search preserves history without presenting it as current operator work. */
export function decisionCommandPresentation(decision: Pick<DecisionRequest, "state" | "clarification" | "reason">) {
  const waiting = waitingForClarification(decision as DecisionRequest);
  const group = decision.state !== "pending" ? "Decision history" as const
    : waiting ? "Waiting for reply" as const : "Attention" as const;
  const status = decision.state === "resolved" ? "Answered"
    : decision.state === "withdrawn" ? "Withdrawn"
    : waiting ? "Waiting for the requester · not answered" : "Needs your answer";
  return { group, detail: decision.reason ? `${status} · ${decision.reason}` : status };
}
