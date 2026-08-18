import { useEffect, useLayoutEffect, useRef, useState, type FormEvent } from "react";

import { fetchEmailTaskSources, type EmailTaskSource, type JiraComment, type JiraTaskLink, type SessionSummary, type Task, type TaskActivityPage, type TaskDraftInput, type TaskPriority, type TaskState, type TaskUpdateInput, type Worker } from "../api";
import BeeMascot from "../brand/BeeMascot";
import { useReorderDrag } from "../shared/useReorderDrag";
import JiraTaskIntake from "./JiraTaskIntake";
import EmailTaskIntake from "./EmailTaskIntake";
import TaskBoardControls, { type TaskBoardFilter, type TaskBoardSort, type TaskBoardSource, type TaskProjectChoice } from "./TaskBoardControls";
import TaskCard from "./TaskCard";
import { buildTaskBoardView } from "./taskBoardModel";

type Props = {
  tasks: Task[];
  jiraTaskLinks: JiraTaskLink[];
  operatorToken: string;
  focusTaskId?: string;
  focusRequest?: number;
  composeRequest?: number;
  sessions: SessionSummary[];
  workers: Worker[];
  busy: boolean;
  onCreate: (input: TaskDraftInput) => Promise<void>;
  onUpdate: (task: Task, input: TaskUpdateInput) => Promise<void>;
  onRemove: (task: Task) => Promise<void>;
  onTransition: (task: Task, state: TaskState, note?: string) => Promise<void>;
  onAssign: (task: Task, workerId: string) => Promise<void>;
  onStartWorker: (task: Task) => Promise<void>;
  onOpenWorker: (sessionId: string) => void;
  onFetchActivity: (taskId: string) => Promise<TaskActivityPage>;
  onFetchJiraComments: (taskId: string) => Promise<JiraComment[]>;
  onAddJiraComment: (taskId: string, body: string) => Promise<{ state: string }>;
  onRetryJira: (task: Task) => Promise<void>;
  onJiraImported: () => Promise<void>;
  onEmailImported?: () => Promise<void>;
  onReorder: (taskIds: string[]) => Promise<void>;
  query?: string;
  filter?: TaskBoardFilter;
  source?: TaskBoardSource;
  sort?: TaskBoardSort;
  project?: string;
  worker?: string;
  onQueryChange?: (query: string) => void;
  onFilterChange?: (filter: TaskBoardFilter) => void;
  onSourceChange?: (source: TaskBoardSource) => void;
  onSortChange?: (sort: TaskBoardSort) => void;
  onProjectChange?: (project: string) => void;
  onWorkerChange?: (worker: string) => void;
  projects?: TaskProjectChoice[];
  onJiraSync?: () => void;
};

const stateLabels: Record<TaskState, string> = {
  draft: "Draft",
  ready: "Ready",
  active: "In progress",
  blocked: "Blocked",
  review: "Review",
  completed: "Completed",
};

const priorityLabels: Record<TaskPriority, string> = {
  low: "Low",
  normal: "Normal",
  high: "High",
  urgent: "Urgent",
};
const validTargets: Record<TaskState, TaskState[]> = {
  draft: ["ready"],
  ready: ["active", "blocked"],
  active: ["blocked", "review"],
  blocked: ["ready", "active"],
  // Completion carries durable verification evidence and therefore uses the
  // explicit review form rather than an evidence-free drag shortcut.
  review: ["active"],
  completed: [],
};

export default function TaskBoard({
  tasks,
  jiraTaskLinks,
  operatorToken,
  focusTaskId,
  focusRequest,
  composeRequest,
  sessions,
  workers,
  busy,
  onCreate,
  onUpdate,
  onRemove,
  onTransition,
  onAssign,
  onStartWorker,
  onOpenWorker,
  onFetchActivity,
  onFetchJiraComments,
  onAddJiraComment,
  onRetryJira,
  onJiraImported,
  onEmailImported = onJiraImported,
  onReorder,
  query = "",
  filter = "all",
  source = "all",
  sort = "queue",
  project = "all",
  worker = "all",
  onQueryChange,
  onFilterChange,
  onSourceChange,
  onSortChange,
  onProjectChange,
  onWorkerChange,
  projects = [],
  onJiraSync,
}: Props) {
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [priority, setPriority] = useState<TaskPriority>("normal");
  const assignableWorkers = workers.filter((worker) => worker.role !== "queen");
  const [workerId, setWorkerId] = useState("");
  const [composeOpen, setComposeOpen] = useState(false);
  const [jiraOpen, setJiraOpen] = useState(false);
  const [emailOpen, setEmailOpen] = useState(false);
  const [emailTaskSources, setEmailTaskSources] = useState<EmailTaskSource[]>([]);
  const titleInput = useRef<HTMLInputElement>(null);
  const completedTasksPanel = useRef<HTMLDetailsElement>(null);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!title.trim() || !workerId) return;
    await onCreate({ title, description, priority, worker_id: workerId });
    setTitle("");
    setDescription("");
    setPriority("normal");
  }

  useEffect(() => {
    if (!workerId && assignableWorkers[0]) setWorkerId(assignableWorkers[0].id);
    else if (workerId && !assignableWorkers.some((worker) => worker.id === workerId)) setWorkerId(assignableWorkers[0]?.id ?? "");
  }, [assignableWorkers, workerId]);

  const taskView = buildTaskBoardView(tasks, jiraTaskLinks, workers, { text: query, filter, source, sort, project, worker }, new Set(emailTaskSources.map((item) => item.task_id)));
  useEffect(() => {
    let cancelled = false;
    void fetchEmailTaskSources(operatorToken)
      .then((sources) => { if (!cancelled) setEmailTaskSources(Array.isArray(sources) ? sources : []); })
      .catch(() => { if (!cancelled) setEmailTaskSources([]); });
    return () => { cancelled = true; };
  }, [operatorToken, tasks]);
  const { open: openTasks, completed: completedTasks, jiraByTask } = taskView;
  const focusedTaskCompleted = Boolean(focusTaskId && completedTasks.some((task) => task.id === focusTaskId));
  const canReorder = sort === "queue" && !query.trim() && filter === "all";
  const taskReorder = useReorderDrag(openTasks.map((task) => task.id), (taskIds) => void onReorder(taskIds));
  const draggedTask = tasks.find((task) => task.id === taskReorder.draggedId);

  useLayoutEffect(() => {
    if (!focusTaskId) return;
    if (focusedTaskCompleted) completedTasksPanel.current?.setAttribute("open", "");
    const frame = requestAnimationFrame(() => {
      const card = document.querySelector<HTMLElement>(`[data-task-id="${CSS.escape(focusTaskId)}"]`);
      card?.scrollIntoView({ behavior: "smooth", block: "center" });
      card?.focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(frame);
  }, [focusTaskId, focusRequest, focusedTaskCompleted]);

  useEffect(() => {
    if (!composeRequest) return;
    setComposeOpen(true);
    setJiraOpen(false);
    setEmailOpen(false);
  }, [composeRequest]);

  useLayoutEffect(() => {
    if (!composeRequest || !composeOpen) return;
    titleInput.current?.focus();
    // Closing the command dialog can restore focus to its former control after
    // this panel mounts, especially in mobile Chromium. Reassert focus after
    // that teardown so the newly requested task field remains the destination.
    const frame = requestAnimationFrame(() => titleInput.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, [composeOpen, composeRequest]);

  function moveTaskAt(index: number, offset: -1 | 1) {
    const target = index + offset;
    if (target < 0 || target >= openTasks.length) return;
    const taskIds = openTasks.map((task) => task.id);
    [taskIds[index], taskIds[target]] = [taskIds[target], taskIds[index]];
    void onReorder(taskIds);
  }

  return (
    <div className="task-board">
      <section className={`task-compose${composeOpen || jiraOpen || emailOpen ? " compose-open" : " compose-collapsed"}`} aria-labelledby="new-task-heading">
        <div className="task-compose-header">
          <div>
            <p className="eyebrow">Add work</p>
            <h3 id="new-task-heading">What should the Hive take on next?</h3>
            <p>Create focused work, claim Jira work, or turn an Inbox report into a task.</p>
          </div>
          <div className="task-entry-actions">
          <button
            type="button"
            className={composeOpen ? "primary-action" : "secondary-button"}
            aria-expanded={composeOpen}
            aria-controls="new-task-form"
            onClick={() => {
              if (composeOpen) setComposeOpen(false);
              else {
                setComposeOpen(true);
                setJiraOpen(false);
                setEmailOpen(false);
                requestAnimationFrame(() => titleInput.current?.focus());
              }
            }}
          >
            {composeOpen ? "Close task form" : "Write task"}
          </button>
          <button
            type="button"
            className={jiraOpen ? "primary-action" : "secondary-button"}
            aria-expanded={jiraOpen}
            aria-controls="jira-work-source"
            onClick={() => { setJiraOpen((current) => !current); setComposeOpen(false); setEmailOpen(false); }}
          >
            {jiraOpen ? "Close Jira work" : "Claim Jira work"}
          </button>
          <button
            type="button"
            className={emailOpen ? "primary-action" : "secondary-button"}
            aria-expanded={emailOpen}
            aria-controls="email-work-source"
            onClick={() => { setEmailOpen((current) => !current); setComposeOpen(false); setJiraOpen(false); }}
          >
            {emailOpen ? "Close email" : "Use email"}
          </button>
          </div>
        </div>
        {composeOpen && <form id="new-task-form" onSubmit={(event) => void submit(event)}>
          <div className="field-stack task-title-field">
            <label htmlFor="task-title">Task title</label>
            <input
              id="task-title"
              ref={titleInput}
              autoFocus={Boolean(composeRequest)}
              value={title}
              onChange={(event) => setTitle(event.target.value)}
              placeholder="What should be true when this is done?"
              maxLength={240}
            />
          </div>
          <div className="field-stack task-description-field">
            <label htmlFor="task-description">Description <span>optional</span></label>
            <textarea
              id="task-description"
              value={description}
              onChange={(event) => setDescription(event.target.value)}
              placeholder="Context, constraints, or what done looks like"
              maxLength={10000}
              rows={2}
            />
          </div>
          <div className="field-stack task-priority-field">
            <label htmlFor="task-priority">Priority</label>
            <select id="task-priority" value={priority} onChange={(event) => setPriority(event.target.value as TaskPriority)}>
              {Object.entries(priorityLabels).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
            </select>
          </div>
          <div className="field-stack task-worker-field">
            <label htmlFor="task-worker">Who should handle this?</label>
            <select id="task-worker" value={workerId} onChange={(event) => setWorkerId(event.target.value)}>
              {assignableWorkers.length === 0 && <option value="">Configure a worker first</option>}
              {assignableWorkers.map((worker) => (
                <option key={worker.id} value={worker.id}>{worker.name} · {repositoryName(worker.workspace)}</option>
              ))}
            </select>
          </div>
          <button disabled={busy || !title.trim() || !workerId}>Create draft</button>
        </form>}
        {jiraOpen ? <div id="jira-work-source"><JiraTaskIntake operatorToken={operatorToken} onImported={onJiraImported} /></div> : null}
        {emailOpen ? <div id="email-work-source"><EmailTaskIntake operatorToken={operatorToken} workers={workers} onImported={onEmailImported} /></div> : null}
      </section>

      <details className="task-mobile-controls">
        <summary>Find, filter, and sort <span>{openTasks.length}/{taskView.allOpenCount}</span></summary>
        <TaskBoardControls query={query} filter={filter} source={source} sort={sort} project={project} worker={worker} workers={workers} projects={projects} openCount={taskView.allOpenCount} busy={busy} onQueryChange={(value) => onQueryChange?.(value)} onFilterChange={(value) => onFilterChange?.(value)} onSourceChange={(value) => onSourceChange?.(value)} onSortChange={(value) => onSortChange?.(value)} onProjectChange={(value) => onProjectChange?.(value)} onWorkerChange={(value) => onWorkerChange?.(value)} onSync={onJiraSync} />
      </details>

      <section className="task-section" aria-labelledby="active-work-heading">
        <div className="section-heading">
          <div><p className="eyebrow">Queue</p><h3 id="active-work-heading">Active work</h3></div>
          <span className="count-badge">{openTasks.length === taskView.allOpenCount ? openTasks.length : `${openTasks.length}/${taskView.allOpenCount}`}</span>
        </div>
        {openTasks.length === 0 ? (
          <div className="empty-card"><BeeMascot className="empty-bee" expression="available" /><div><strong>{taskView.allOpenCount ? "No work matches this view" : "No work queued"}</strong><span>{taskView.allOpenCount ? "Adjust the task-board filters in the sidebar." : "Create a focused task when you are ready."}</span></div></div>
        ) : (
          <div className="task-grid">
            {openTasks.map((task, index) => (
              <TaskCard
                key={task.id}
                task={task}
                jiraLink={jiraTaskLinks.find((link) => link.task_id === task.id)}
                emailSources={emailTaskSources.filter((source) => source.task_id === task.id)}
                operatorToken={operatorToken}
                sessions={sessions}
                workers={workers}
                busy={busy}
                onUpdate={onUpdate}
                onRemove={onRemove}
                onTransition={onTransition}
                onAssign={onAssign}
                onStartWorker={onStartWorker}
                onOpenWorker={onOpenWorker}
                onFetchActivity={onFetchActivity}
                onFetchJiraComments={onFetchJiraComments}
                onAddJiraComment={onAddJiraComment}
                onRetryJira={onRetryJira}
                canMoveEarlier={canReorder && index > 0}
                canMoveLater={canReorder && index < openTasks.length - 1}
                onMoveEarlier={() => moveTaskAt(index, -1)}
                onMoveLater={() => moveTaskAt(index, 1)}
                onDropBefore={() => taskReorder.dropBefore(task.id)}
                dropTarget={taskReorder.dropTargetId === task.id && taskReorder.draggedId !== task.id}
                onDragTarget={() => taskReorder.target(task.id)}
                onDragLeave={() => taskReorder.leave(task.id)}
                onDragStart={canReorder ? taskReorder.start : () => undefined}
                onDragEnd={taskReorder.end}
              />
            ))}
          </div>
        )}
        {draggedTask && (
          <div className="task-drop-strip" aria-live="polite">
            <span>Move <strong>{draggedTask.title}</strong> to</span>
            {validTargets[draggedTask.state].map((state) => (
              <button
                className={`task-drop-target state-${state}`}
                key={state}
                onDragOver={(event) => event.preventDefault()}
                onDrop={(event) => {
                  event.preventDefault();
                  taskReorder.end();
                  void onTransition(draggedTask, state);
                }}
                disabled={busy}
              >
                {stateLabels[state]}
              </button>
            ))}
          </div>
        )}

      </section>

      {completedTasks.length > 0 && (
        <details ref={completedTasksPanel} className="completed-tasks">
          <summary><span>Completed work</span><small>{completedTasks.length}</small></summary>
          <div className="task-grid compact">
            {completedTasks.map((task) => (
              <TaskCard
                key={task.id}
                task={task}
                jiraLink={jiraTaskLinks.find((link) => link.task_id === task.id)}
                emailSources={emailTaskSources.filter((source) => source.task_id === task.id)}
                operatorToken={operatorToken}
                sessions={sessions}
                workers={workers}
                busy={busy}
                onUpdate={onUpdate}
                onRemove={onRemove}
                onTransition={onTransition}
                onAssign={onAssign}
                onStartWorker={onStartWorker}
                onOpenWorker={onOpenWorker}
                onFetchActivity={onFetchActivity}
                onFetchJiraComments={onFetchJiraComments}
                onAddJiraComment={onAddJiraComment}
                onRetryJira={onRetryJira}
                canMoveEarlier={false}
                canMoveLater={false}
                onMoveEarlier={() => undefined}
                onMoveLater={() => undefined}
                onDropBefore={() => undefined}
                dropTarget={false}
                onDragTarget={() => undefined}
                onDragLeave={() => undefined}
                onDragStart={taskReorder.start}
                onDragEnd={taskReorder.end}
              />
            ))}
          </div>
        </details>
      )}
    </div>
  );
}

export function workerName(sessionId: string): string {
  return `Claude ${sessionId.slice(-4).toUpperCase()}`;
}

function repositoryName(workspace: string): string {
  const parts = workspace.split(/[\\/]/).filter(Boolean);
  return parts.at(-1) ?? workspace;
}
