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
export function checkedQueueWaits(tasks: Task[], snapshot?: QueenReviewQueueSnapshot) {
  const current = new Map(tasks.map(task => [task.id, task]));
  const waits = new Map<string, NonNullable<QueenReviewQueueSnapshot["items"][number]["previous_assessment"]>>();
  for (const item of snapshot?.items ?? []) {
    const task = current.get(item.task.id);
    const previous = item.previous_assessment;
    if (!task || task.next_move_owner !== "queen" || !previous
      || canonical(task) !== canonical(item.task)) continue;
    // External judgments need a fresh check each run. Authenticated operator
    // deferrals remain applicable between runs while local evidence matches.
    if (previous.status === "covered_for_current_run"
      || (previous.status === "no_active_review" && previous.assessment.kind === "operator_deferral")) {
      waits.set(task.id, previous);
    }
  }
  return waits;
}
