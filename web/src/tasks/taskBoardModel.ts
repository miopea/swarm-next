import type { JiraTaskLink, Task, TaskPriority, TaskState, Worker } from "../api";
import { isClosedTaskState, isOpenTaskState } from "../api/tasks";

export type TaskBoardFilter = "all" | "unassigned" | "assigned" | "active" | "attention";
export type TaskBoardSource = "all" | "jira" | "email" | "local";
export type TaskBoardSort = "queue" | "priority" | "status" | "created" | "updated" | "worker" | "project";

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
  /**
   * Finished, with nothing showing it to be live.
   *
   * Held apart from `completed` rather than sorted within it. Completed is
   * where work goes to stop being looked at — 115 of 138 tasks on the reporting
   * Hive — so filing unverified work there put the one finished state that
   * still needs somebody's attention in the place least likely to get it.
   */
  unverified: Task[];
  completed: Task[];
  allOpenCount: number;
  jiraByTask: Map<string, JiraTaskLink>;
};

const priorityOrder: Record<TaskPriority, number> = { urgent: 0, high: 1, normal: 2, low: 3 };
const stateOrder: Record<TaskState, number> = { blocked: 0, review: 1, awaiting_release: 2, active: 3, ready: 4, draft: 5, completed: 6, abandoned: 7 };

function taskComparator(
  sort: TaskBoardSort,
  workerNames: Map<string, string>,
  jiraProjects: Map<string, string>,
): (left: Task, right: Task) => number {
  if (sort === "priority") return (left, right) => priorityOrder[left.priority] - priorityOrder[right.priority] || left.position - right.position;
  if (sort === "status") return (left, right) => stateOrder[left.state] - stateOrder[right.state] || left.position - right.position;
  // Oldest first, and that is the point of having it. updated_at moves whenever
  // anything touches a task, including automation, so it answers "what changed
  // recently"; created_at answers "what has been waiting longest", which is the
  // question that surfaces neglect. Newest-first would bury exactly the tasks
  // this ordering exists to find.
  if (sort === "created") return (left, right) => left.created_at - right.created_at || left.position - right.position;
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
  const allOpen = tasks.filter((task) => isOpenTaskState(task.state));
  const completed = tasks.filter((task) => isClosedTaskState(task.state));
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
  // Evidence, not deployment. A task closed on a no-deployment claim Queen
  // approved is properly finished — somebody looked and agreed there was
  // nothing to ship — and keying this off deployment_recorded alone called 30
  // of the 68 rows it touched unverified when they were not.
  //
  // And a Jira issue mirrored in, that no Swarm worker ever acted on, is not
  // Swarm work awaiting evidence at all: the completion record exists, in the
  // system that owns the work. Twelve of these sat in the section asking
  // somebody to chase evidence nobody owed.
  //
  // The condition is worker involvement AND Jira ownership, never the Jira link
  // alone. Work a Swarm worker really did against a Jira issue and never
  // deployed is a genuine gap and stays visible — that case has never occurred
  // on this Hive, which is precisely why it is written down rather than left to
  // the shape of today's data.
  const ownedElsewhere = (task: Task) => Boolean(jiraByTask.get(task.id)) && !task.worked_here;
  // Recorded-unverifiable leaves this queue because nothing is coming for it:
  // the operator has said nobody can now establish where it went. It does NOT
  // move to the verified side either -- completed still renders it as
  // unverifiable, so the board never claims somebody checked.
  // ABANDONED WORK OWES NOTHING, so it never joins this queue. It carries no
  // evidence for the same reason it was abandoned -- nothing shipped and
  // nothing is coming -- and without this line every abandoned task would
  // appear here asking somebody to chase evidence that cannot exist. That is
  // the clicking this state was added to delete, reintroduced one layer up.
  const unverified = closed.filter(
    (task) =>
      task.state !== "abandoned" &&
      !task.closed_on_evidence &&
      !task.closed_unverifiable &&
      !ownedElsewhere(task),
  );
  return {
    open,
    unverified,
    // ABANDONED WORK BELONGS HERE, and the alternative was invisibility. It
    // is excluded from `unverified` because it owes no evidence, so unless
    // it is admitted here it lands in neither list and disappears from the
    // board -- closed in the database and nowhere on the screen.
    completed: closed.filter(
      (task) =>
        task.state === "abandoned" ||
        task.closed_on_evidence ||
        task.closed_unverifiable ||
        ownedElsewhere(task),
    ),
    allOpenCount: allOpen.length,
    jiraByTask,
  };
}
