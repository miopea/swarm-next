import type { Worker } from "../api";
import type { BeeExpression } from "../brand/BeeMascot";

export type WorkerAttentionPresentation = {
  state: Worker["attention_state"];
  label: string;
  compactLabel: string;
  expression: BeeExpression;
  presence: "offline" | "online" | "engaged" | "waiting" | "blocked";
};

const presentation: Record<Worker["attention_state"], Omit<WorkerAttentionPresentation, "state">> = {
  sleeping: { label: "Sleeping", compactLabel: "sleeping", expression: "sleeping", presence: "offline" },
  resting: { label: "Resting", compactLabel: "resting", expression: "available", presence: "online" },
  buzzing: { label: "Buzzing", compactLabel: "buzzing", expression: "thinking", presence: "online" },
  with_operator: { label: "With you", compactLabel: "with you", expression: "focused", presence: "engaged" },
  awaiting_operator: { label: "Awaiting you", compactLabel: "awaiting you", expression: "available", presence: "waiting" },
  blocked: { label: "Blocked", compactLabel: "blocked", expression: "blocked", presence: "blocked" },
};

export function workerAttention(worker: Worker, now = Date.now()): WorkerAttentionPresentation {
  const state = worker.attention_state === "with_operator"
    && worker.engagement_expires_at !== undefined
    && worker.engagement_expires_at * 1000 <= now
    ? "resting"
    : worker.attention_state;
  return { state, ...presentation[state] };
}

export function workerSwitcherDetail(worker: Worker, assignedTaskTitle?: string): string {
  const state = worker.running ? workerAttention(worker).label : "Sleeping";
  if (assignedTaskTitle) return `${state} · ${assignedTaskTitle}`;
  return worker.running ? state : "Sleeping · tap to wake";
}
