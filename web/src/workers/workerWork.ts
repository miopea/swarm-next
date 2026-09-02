import type { Task } from "../api";

/**
 * How a worker's open work is summarised: the natural progression, so a chip
 * reads "1 active · 2 blocked · 6 ready".
 *
 * ⚠️ EVERY OPEN STATE MUST BE HERE. A state missing from this list is silently
 * dropped from the summary, so the chip and the "+N" badge beside it disagree
 * by exactly that state's count. `awaiting_release` was missing until
 * 2026-09-02: a worker with 25 finished-and-unshipped tasks read "1 review · 6
 * blocked" next to a badge saying +31, and the operator asked which was the
 * error. Neither was — they were counting different things, and only one said so.
 */
const WORK_ORDER: Task["state"][] = [
  "active",
  "review",
  "awaiting_release",
  "blocked",
  "ready",
  "draft",
];

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
const CURRENT_ORDER: Task["state"][] = [
  "active",
  "ready",
  "review",
  "blocked",
  "draft",
  // Finished and waiting on a release. The worker is not "on" it in any sense,
  // so it ranks below work that is merely stuck.
  "awaiting_release",
];

/**
 * Where a state sorts, with UNKNOWN STATES LAST.
 *
 * ⚠️ THIS EXISTS BECAUSE `indexOf` RETURNS -1, AND -1 SORTS BEFORE 0. A state
 * missing from an order array did not fall to the end — it jumped to the FRONT
 * and outranked `active`. That is how `awaiting_release` came to be displayed
 * as what a worker was currently on, while its actual review and blocked work
 * sat behind a "+31".
 *
 * Adding the missing state fixes today. This fixes the next one: a state added
 * to the lifecycle and forgotten here now sorts harmlessly last instead of
 * taking over the display.
 */
function rank(order: Task["state"][], state: Task["state"]): number {
  const index = order.indexOf(state);
  return index === -1 ? Number.MAX_SAFE_INTEGER : index;
}

export type WorkerWork = {
  current?: Task;
  summary?: string;
  /** Every open task this worker owns, including `current`. */
  openCount: number;
};

export function workerWork(tasks: Task[]): WorkerWork {
  const open = tasks
    // Abandoned work is closed, exactly as completed work is. Counting it as
    // open inflates the badge with work nobody will ever do again.
    .filter((task) => task.state !== "completed" && task.state !== "abandoned")
    .sort((left, right) => {
      const stateOrder = rank(WORK_ORDER, left.state) - rank(WORK_ORDER, right.state);
      return stateOrder || left.position - right.position || left.created_at - right.created_at;
    });

  if (open.length === 0) return { openCount: 0 };

  const current = [...open].sort((left, right) => {
    const stateOrder = rank(CURRENT_ORDER, left.state) - rank(CURRENT_ORDER, right.state);
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
