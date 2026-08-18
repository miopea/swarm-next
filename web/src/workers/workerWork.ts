import type { Task } from "../api";

const WORK_ORDER: Task["state"][] = ["active", "review", "blocked", "ready", "draft"];

export type WorkerWork = {
  current?: Task;
  summary?: string;
};

export function workerWork(tasks: Task[]): WorkerWork {
  const open = tasks
    .filter((task) => task.state !== "completed")
    .sort((left, right) => {
      const stateOrder = WORK_ORDER.indexOf(left.state) - WORK_ORDER.indexOf(right.state);
      return stateOrder || left.position - right.position || left.created_at - right.created_at;
    });

  if (open.length === 0) return {};

  const counts = new Map<Task["state"], number>();
  open.forEach((task) => counts.set(task.state, (counts.get(task.state) ?? 0) + 1));
  const summary = WORK_ORDER
    .map((state) => {
      const count = counts.get(state) ?? 0;
      return count ? `${count} ${state}` : undefined;
    })
    .filter(Boolean)
    .join(" · ");

  return { current: open[0], summary };
}
