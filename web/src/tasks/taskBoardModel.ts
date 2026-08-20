import type { JiraTaskLink, Task, TaskPriority, TaskState, Worker } from "../api";

export type TaskBoardFilter = "all" | "unassigned" | "assigned" | "active" | "attention";
export type TaskBoardSource = "all" | "jira" | "email" | "local";
export type TaskBoardSort = "queue" | "priority" | "status" | "updated" | "worker" | "project";

export type TaskBoardQuery = {
  text: string;
  filter: TaskBoardFilter;
  source: TaskBoardSource;
  sort: TaskBoardSort;
  project: string;
  worker: string;
};

export type TaskBoardView = {
  open: Task[];
  completed: Task[];
  allOpenCount: number;
  jiraByTask: Map<string, JiraTaskLink>;
};

const priorityOrder: Record<TaskPriority, number> = { urgent: 0, high: 1, normal: 2, low: 3 };
const stateOrder: Record<TaskState, number> = { blocked: 0, review: 1, active: 2, ready: 3, draft: 4, completed: 5 };

function taskComparator(
  sort: TaskBoardSort,
  workerNames: Map<string, string>,
  jiraProjects: Map<string, string>,
): (left: Task, right: Task) => number {
  if (sort === "priority") return (left, right) => priorityOrder[left.priority] - priorityOrder[right.priority] || left.position - right.position;
  if (sort === "status") return (left, right) => stateOrder[left.state] - stateOrder[right.state] || left.position - right.position;
  if (sort === "updated") return (left, right) => right.updated_at - left.updated_at || left.position - right.position;
  if (sort === "worker") return (left, right) => (workerNames.get(left.assigned_worker_id ?? "") ?? "Unassigned").localeCompare(workerNames.get(right.assigned_worker_id ?? "") ?? "Unassigned") || left.position - right.position;
  if (sort === "project") return (left, right) => (jiraProjects.get(left.id) ?? "Local work").localeCompare(jiraProjects.get(right.id) ?? "Local work") || left.position - right.position;
  return (left, right) => left.position - right.position;
}

export function buildTaskBoardView(
  tasks: Task[],
  jiraTaskLinks: JiraTaskLink[],
  workers: Worker[],
  query: TaskBoardQuery,
  emailTaskIds: ReadonlySet<string> = new Set(),
): TaskBoardView {
  const allOpen = tasks.filter((task) => task.state !== "completed");
  const completed = tasks.filter((task) => task.state === "completed");
  const jiraByTask = new Map(jiraTaskLinks.map((link) => [link.task_id, link]));
  const jiraProjects = new Map(jiraTaskLinks.map((link) => [link.task_id, link.project_name]));
  const workerNames = new Map(workers.map((worker) => [worker.id, worker.name]));
  const normalizedText = query.text.trim().toLocaleLowerCase();

  // One predicate for both lists. Filtering and sorting used to apply to open
  // work only, so choosing a source narrowed the top of the board and left
  // finished work below it untouched — the operator sorted by email and watched
  // the closed tasks ignore them.
  const matches = (task: Task) => {
    const jiraLink = jiraByTask.get(task.id);
    const source = jiraLink ? "jira" : emailTaskIds.has(task.id) ? "email" : "local";
    const matchesText = !normalizedText || [task.title, task.description, jiraLink?.issue_key, jiraLink?.project_name]
      .some((value) => value?.toLocaleLowerCase().includes(normalizedText));
    if (!matchesText) return false;
    if (query.source !== "all" && source !== query.source) return false;
    if (query.project !== "all" && jiraLink?.project_key !== query.project) return false;
    if (query.worker === "unassigned" && task.assigned_worker_id) return false;
    if (query.worker !== "all" && query.worker !== "unassigned" && task.assigned_worker_id !== query.worker) return false;
    return true;
  };

  const open = allOpen.filter((task) => {
    if (!matches(task)) return false;
    // State filters describe work in progress, so they say nothing about work
    // that is finished and are not asked of it.
    if (query.filter === "unassigned") return !task.assigned_worker_id;
    if (query.filter === "assigned") return Boolean(task.assigned_worker_id);
    if (query.filter === "active") return task.state === "active";
    if (query.filter === "attention") return task.state === "blocked" || task.state === "review";
    return true;
  });

  const comparator = taskComparator(query.sort, workerNames, jiraProjects);
  open.sort(comparator);
  const closed = completed.filter(matches);
  closed.sort(comparator);
  return { open, completed: closed, allOpenCount: allOpen.length, jiraByTask };
}
