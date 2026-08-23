import type { Task } from "../api";

/**
 * How a worker's open work is summarised: the natural progression, so a chip
 * reads "1 active · 2 blocked · 6 ready".
 */
const WORK_ORDER: Task["state"][] = ["active", "review", "blocked", "ready", "draft"];

/**
 * Which single task answers "what is this worker on".
 *
 * Not the same question as the summary above, and ordering it the same way was
 * wrong: `blocked` and `review` are both states where the worker is waiting on
 * somebody else, so neither should outrank work it can actually pick up.
 *
 * The operator hit exactly that — one blocked task and fifteen ready ones, with
 * the blocked one shown as the worker's current task: "why is that one showing
 * as the active task?" It outranked every task the worker could have started.
 */
const CURRENT_ORDER: Task["state"][] = ["active", "ready", "review", "blocked", "draft"];

export type WorkerWork = {
  current?: Task;
  summary?: string;
  /** Every open task this worker owns, including `current`. */
  openCount: number;
};

export function workerWork(tasks: Task[]): WorkerWork {
  const open = tasks
    .filter((task) => task.state !== "completed")
    .sort((left, right) => {
      const stateOrder = WORK_ORDER.indexOf(left.state) - WORK_ORDER.indexOf(right.state);
      return stateOrder || left.position - right.position || left.created_at - right.created_at;
    });

  if (open.length === 0) return { openCount: 0 };

  const current = [...open].sort((left, right) => {
    const stateOrder = CURRENT_ORDER.indexOf(left.state) - CURRENT_ORDER.indexOf(right.state);
    return stateOrder || left.position - right.position || left.created_at - right.created_at;
  })[0];

  const counts = new Map<Task["state"], number>();
  open.forEach((task) => counts.set(task.state, (counts.get(task.state) ?? 0) + 1));
  const summary = WORK_ORDER
    .map((state) => {
      const count = counts.get(state) ?? 0;
      return count ? `${count} ${state}` : undefined;
    })
    .filter(Boolean)
    .join(" · ");

  return { current, summary, openCount: open.length };
}
