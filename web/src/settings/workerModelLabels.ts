import type { WorkerModel } from "../api";

/** What each worker model is called on screen (ADR 0116). */
export const WORKER_MODEL_LABELS: Record<WorkerModel, string> = {
  default: "Claude's default",
  opus: "Opus",
  sonnet: "Sonnet",
  haiku: "Haiku",
};
