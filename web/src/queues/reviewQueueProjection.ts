import type { QueenReviewQueueSnapshot } from "../api";
import type { Task } from "../api/tasks";

function canonical(value: unknown): string {
  return JSON.stringify(value, (_key, item: unknown) => {
    if (item && typeof item === "object" && !Array.isArray(item)) {
      return Object.fromEntries(Object.entries(item).sort(([a], [b]) => a.localeCompare(b)));
    }
    return item;
  });
}

/** Fence the complete task projection, not only its second-resolution timestamp.
 * A checked wait changes presentation only, never execution owner or authority. */
function currentQueueAssessments(tasks: Task[], snapshot?: QueenReviewQueueSnapshot) {
  const current = new Map(tasks.map(task => [task.id, task]));
  const waits = new Map<string, NonNullable<QueenReviewQueueSnapshot["items"][number]["previous_assessment"]>>();
  for (const item of snapshot?.items ?? []) {
    const task = current.get(item.task.id);
    const previous = item.previous_assessment;
    if (!task || task.next_move_owner !== "queen" || !previous
      || canonical(task) !== canonical(item.task)) continue;
    waits.set(task.id, previous);
  }
  return waits;
}

/** Match full evidence once per refresh, then partition presentation categories.
 * No cross-refresh cache can retain a stale authorization or task projection. */
export function projectReviewQueue(tasks: Task[], snapshot?: QueenReviewQueueSnapshot) {
  const current = currentQueueAssessments(tasks, snapshot);
  const checkedWaits: typeof current = new Map();
  const investigations: typeof current = new Map();
  const rechecks: typeof current = new Map();
  for (const [id, previous] of current) {
    const kind = previous.assessment.kind;
    if (kind === "insufficient_evidence" && previous.status === "insufficient_evidence") {
      investigations.set(id, previous);
    }
    // Historical context only: external conditions are not covered between runs.
    if (kind === "external_condition"
      && (previous.status === "fresh_external_check_required" || previous.status === "no_active_review")) {
      rechecks.set(id, previous);
    }
    // External judgments need a fresh check each run. Authenticated operator
    // deferrals remain applicable between runs while local evidence matches.
    if (kind !== "insufficient_evidence" && (previous.status === "covered_for_current_run"
      || (previous.status === "no_active_review" && kind === "operator_deferral"))) {
      checkedWaits.set(id, previous);
    }
  }
  return { checkedWaits, investigations, rechecks };
}
