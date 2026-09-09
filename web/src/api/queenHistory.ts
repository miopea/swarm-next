import { authenticatedFetch } from "./request";

export type QueenRunOutcome = "completed" | "needs_operator" | "no_action" | "incomplete";
export type QueenRunEvidence = {
  run_id: string;
  finished_on_build: string | null;
  trigger: "manual" | "actionable_work";
  requested_at: number | null;
  delivered_at: number | null;
  finished_at: number;
  attempts: number;
  initial_actionable_count: number;
  requested_outcome: QueenRunOutcome;
  accepted_outcome: QueenRunOutcome;
};
export type QueenRunHistory = {
  retention_days: number;
  max_retained: number;
  retained_count: number;
  records: QueenRunEvidence[];
};

export async function fetchQueenRunHistory(token: string, signal: AbortSignal): Promise<QueenRunHistory> {
  const response = await authenticatedFetch(token, "/api/v1/runtime/queen-history?limit=100", { signal });
  return response.json() as Promise<QueenRunHistory>;
}
