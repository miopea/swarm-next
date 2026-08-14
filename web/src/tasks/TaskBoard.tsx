import { useEffect, useRef, useState, type DragEvent, type FormEvent } from "react";

import type { JiraComment, JiraTaskLink, SessionSummary, Task, TaskActivity, TaskActivityPage, TaskDraftInput, TaskPriority, TaskState, TaskUpdateInput, Worker } from "../api";
import BeeMascot from "../brand/BeeMascot";
import { useReorderDrag } from "../shared/useReorderDrag";
import JiraTaskIntake from "./JiraTaskIntake";
import TaskBoardControls, { type TaskBoardFilter, type TaskBoardSort, type TaskProjectChoice } from "./TaskBoardControls";
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
  onTransition: (task: Task, state: TaskState) => Promise<void>;
  onAssign: (task: Task, workerId: string) => Promise<void>;
  onStartWorker: (task: Task) => Promise<void>;
  onOpenWorker: (sessionId: string) => void;
  onFetchActivity: (taskId: string) => Promise<TaskActivityPage>;
  onFetchJiraComments: (taskId: string) => Promise<JiraComment[]>;
  onAddJiraComment: (taskId: string, body: string) => Promise<{ state: string }>;
  onRetryJira: (task: Task) => Promise<void>;
  onJiraImported: () => Promise<void>;
  onReorder: (taskIds: string[]) => Promise<void>;
  query?: string;
  filter?: TaskBoardFilter;
  sort?: TaskBoardSort;
  project?: string;
  worker?: string;
  onQueryChange?: (query: string) => void;
  onFilterChange?: (filter: TaskBoardFilter) => void;
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

function taskStateLabel(task: Task): string {
  if (task.state === "ready" && task.assigned_worker_id) return "Assigned";
  return stateLabels[task.state];
}

function workerAttentionLabel(worker: Worker): string {
  const labels = {
    sleeping: "sleeping",
    resting: "resting",
    buzzing: "buzzing",
    with_operator: "with you",
    awaiting_operator: "awaiting you",
    blocked: "blocked",
  } as const;
  return labels[worker.attention_state] ?? (worker.running ? "resting" : "sleeping");
}

const priorityLabels: Record<TaskPriority, string> = {
  low: "Low",
  normal: "Normal",
  high: "High",
  urgent: "Urgent",
};
const dispatchLabels = {
  queued: "Briefing waits for a quiet moment",
  dispatching: "Briefing worker",
  delivered: "Worker briefed",
  uncertain: "Briefing uncertain — task remains authoritative",
} as const;
const outcomeDeliveryLabels = {
  queued: "Queen handoff waits for a quiet moment",
  dispatching: "Notifying Queen",
  delivered: "Queen notified",
  uncertain: "Queen handoff uncertain — task remains authoritative",
} as const;
const validTargets: Record<TaskState, TaskState[]> = {
  draft: ["ready"],
  ready: ["active", "blocked"],
  active: ["blocked", "review"],
  blocked: ["ready", "active"],
  review: ["active", "completed"],
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
  onTransition,
  onAssign,
  onStartWorker,
  onOpenWorker,
  onFetchActivity,
  onFetchJiraComments,
  onAddJiraComment,
  onRetryJira,
  onJiraImported,
  onReorder,
  query = "",
  filter = "all",
  sort = "queue",
  project = "all",
  worker = "all",
  onQueryChange,
  onFilterChange,
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

  const taskView = buildTaskBoardView(tasks, jiraTaskLinks, workers, { text: query, filter, sort, project, worker });
  const { open: openTasks, completed: completedTasks, jiraByTask } = taskView;
  const canReorder = sort === "queue" && !query.trim() && filter === "all";
  const taskReorder = useReorderDrag(openTasks.map((task) => task.id), (taskIds) => void onReorder(taskIds));
  const draggedTask = tasks.find((task) => task.id === taskReorder.draggedId);

  useEffect(() => {
    if (!focusTaskId) return;
    if (completedTasks.some((task) => task.id === focusTaskId)) completedTasksPanel.current?.setAttribute("open", "");
    const frame = requestAnimationFrame(() => {
      const card = document.querySelector<HTMLElement>(`[data-task-id="${CSS.escape(focusTaskId)}"]`);
      card?.scrollIntoView({ behavior: "smooth", block: "center" });
      card?.focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(frame);
  }, [focusTaskId, focusRequest, completedTasks]);

  useEffect(() => {
    if (!composeRequest) return;
    setComposeOpen(true);
    setJiraOpen(false);
  }, [composeRequest]);

  useEffect(() => {
    if (!composeRequest || !composeOpen) return;
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
      <section className={`task-compose${composeOpen || jiraOpen ? " compose-open" : " compose-collapsed"}`} aria-labelledby="new-task-heading">
        <div className="task-compose-header">
          <div>
            <p className="eyebrow">Add work</p>
            <h3 id="new-task-heading">What should the Hive take on next?</h3>
            <p>Create focused work or claim an unassigned Jira issue.</p>
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
                requestAnimationFrame(() => titleInput.current?.focus());
              }
            }}
          >
            {composeOpen ? "Close task form" : "Create task"}
          </button>
          <button
            type="button"
            className={jiraOpen ? "primary-action" : "secondary-button"}
            aria-expanded={jiraOpen}
            aria-controls="jira-work-source"
            onClick={() => { setJiraOpen((current) => !current); setComposeOpen(false); }}
          >
            {jiraOpen ? "Close Jira work" : "Choose Jira work"}
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
      </section>

      <details className="task-mobile-controls">
        <summary>Find, filter, and sort <span>{openTasks.length}/{taskView.allOpenCount}</span></summary>
        <TaskBoardControls query={query} filter={filter} sort={sort} project={project} worker={worker} workers={workers} projects={projects} openCount={taskView.allOpenCount} busy={busy} onQueryChange={(value) => onQueryChange?.(value)} onFilterChange={(value) => onFilterChange?.(value)} onSortChange={(value) => onSortChange?.(value)} onProjectChange={(value) => onProjectChange?.(value)} onWorkerChange={(value) => onWorkerChange?.(value)} onSync={onJiraSync} />
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
                sessions={sessions}
                workers={workers}
                busy={busy}
                onUpdate={onUpdate}
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
                sessions={sessions}
                workers={workers}
                busy={busy}
                onUpdate={onUpdate}
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

function TaskCard({ task, jiraLink, sessions, workers, busy, onUpdate, onTransition, onAssign, onStartWorker, onOpenWorker, onFetchActivity, onFetchJiraComments, onAddJiraComment, onRetryJira, canMoveEarlier, canMoveLater, onMoveEarlier, onMoveLater, onDropBefore, dropTarget, onDragTarget, onDragLeave, onDragStart, onDragEnd }: Omit<Props, "tasks" | "jiraTaskLinks" | "operatorToken" | "focusTaskId" | "focusRequest" | "composeRequest" | "onCreate" | "onJiraImported" | "onReorder"> & { task: Task; jiraLink?: JiraTaskLink; canMoveEarlier: boolean; canMoveLater: boolean; onMoveEarlier: () => void; onMoveLater: () => void; onDropBefore: () => void; dropTarget: boolean; onDragTarget: () => void; onDragLeave: () => void; onDragStart: (taskId: string) => void; onDragEnd: () => void }) {
  const assigned = sessions.find((session) => session.session_id === task.assigned_session_id);
  const assignableWorkers = workers.filter((worker) => worker.role !== "queen");
  const targetWorker = assignableWorkers.find((worker) => worker.id === task.assigned_worker_id);
  const [editing, setEditing] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [activity, setActivity] = useState<TaskActivityPage>();
  const [historyError, setHistoryError] = useState(false);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [discussionOpen, setDiscussionOpen] = useState(false);
  const [comments, setComments] = useState<JiraComment[]>([]);
  const [commentBody, setCommentBody] = useState("");
  const [discussionLoading, setDiscussionLoading] = useState(false);
  const [discussionError, setDiscussionError] = useState("");
  const [discussionMessage, setDiscussionMessage] = useState("");
  const cardRef = useRef<HTMLElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    function dismissMenu(event: PointerEvent) {
      if (event.target instanceof Node && !cardRef.current?.contains(event.target)) setMenuOpen(false);
    }
    document.addEventListener("pointerdown", dismissMenu);
    return () => document.removeEventListener("pointerdown", dismissMenu);
  }, [menuOpen]);

  function runMenuAction(action: () => void) {
    setMenuOpen(false);
    action();
  }

  async function loadActivity() {
    setHistoryLoading(true);
    setHistoryError(false);
    try {
      setActivity(await onFetchActivity(task.id));
    } catch {
      setHistoryError(true);
    } finally {
      setHistoryLoading(false);
    }
  }

  function toggleHistory() {
    if (historyOpen) {
      setHistoryOpen(false);
      return;
    }
    setHistoryOpen(true);
    void loadActivity();
  }

  async function toggleDiscussion() {
    if (discussionOpen) {
      setDiscussionOpen(false);
      return;
    }
    setDiscussionOpen(true);
    setDiscussionLoading(true);
    setDiscussionError("");
    setDiscussionMessage("");
    try {
      setComments(await onFetchJiraComments(task.id));
    } catch (error) {
      setDiscussionError(error instanceof Error ? error.message : "Jira discussion is unavailable.");
    } finally {
      setDiscussionLoading(false);
    }
  }

  async function submitComment(event: FormEvent) {
    event.preventDefault();
    const body = commentBody.trim();
    if (!body) return;
    setDiscussionLoading(true);
    setDiscussionError("");
    try {
      const result = await onAddJiraComment(task.id, body);
      setCommentBody("");
      setComments(await onFetchJiraComments(task.id));
      setDiscussionMessage(result.state === "delivered" ? "Shared to Jira." : "Saved safely; Jira delivery is pending.");
    } catch (error) {
      setDiscussionError(error instanceof Error ? error.message : "The Jira update could not be sent.");
    } finally {
      setDiscussionLoading(false);
    }
  }
  return (
    <article
      ref={cardRef}
      data-task-id={task.id}
      tabIndex={-1}
      className={`task-card state-${task.state}${dropTarget ? " drop-target-before" : ""}`}
      aria-label={task.title}
      draggable={!busy && !editing && task.state !== "completed" && (canMoveEarlier || canMoveLater)}
      onDragStart={(event: DragEvent<HTMLElement>) => { event.dataTransfer.effectAllowed = "move"; event.dataTransfer.setData("text/plain", task.id); onDragStart(task.id); }}
      onDragEnd={onDragEnd}
      onDragEnter={onDragTarget}
      onDragOver={(event) => { event.preventDefault(); onDragTarget(); }}
      onDragLeave={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) onDragLeave(); }}
      onDrop={(event) => { event.preventDefault(); onDropBefore(); }}
      onContextMenu={(event) => { event.preventDefault(); setMenuOpen(true); }}
      onKeyDown={(event) => { if (event.key === "Escape") setMenuOpen(false); }}
    >
      <div className="task-metadata-panel">
        <section className="task-metadata-section" aria-label="Swarm details">
          <strong className="task-section-label">Swarm</strong>
          <dl>
            <div><dt>Status</dt><dd><span className={`task-state state-${task.state}`}>{taskStateLabel(task)}</span></dd></div>
            <div><dt>Priority</dt><dd><span className={`task-priority priority-${task.priority}`}>{priorityLabels[task.priority]}</span></dd></div>
          </dl>
        </section>
        {jiraLink && (
          <section className="task-metadata-section task-jira-origin" aria-label={`Jira issue ${jiraLink.issue_key}`}>
            <strong className="task-section-label">Jira</strong>
            <dl>
              <div><dt>Issue</dt><dd>
                {jiraLink.issue_url ? (
                  <a href={jiraLink.issue_url} target="_blank" rel="noreferrer" title={`Open ${jiraLink.issue_key} in Jira`}>
                    <strong>{jiraLink.issue_key}</strong><span aria-hidden="true">↗</span>
                  </a>
                ) : <strong>{jiraLink.issue_key}</strong>}
              </dd></div>
              <div><dt>Project</dt><dd className="task-text-value" title={jiraLink.project_name}>{jiraLink.project_name}</dd></div>
              <div><dt>Status</dt><dd className="task-text-value">{jiraLink.jira_status_name}</dd></div>
              <div><dt>Assignee</dt><dd className="task-text-value">{jiraLink.jira_assignee_name ?? "Unassigned"}</dd></div>
            </dl>
          {jiraLink.outbound_state && (
            <span className={`jira-sync-state ${jiraLink.outbound_state}`}>
              {jiraLink.outbound_state === "queued" || jiraLink.outbound_state === "dispatching"
                ? "Updating Jira…"
                : "Jira update needs attention"}
            </span>
          )}
          {(jiraLink.outbound_state === "conflict" || jiraLink.outbound_state === "uncertain") && (
            <button className="text-button jira-sync-retry" type="button" disabled={busy} onClick={() => void onRetryJira(task)}>
              Retry Jira
            </button>
          )}
          </section>
        )}
      </div>
      <h4>{task.title}</h4>
      {task.description && !editing && <p className="task-description">{task.description}</p>}
      {editing ? (
        <TaskEditForm task={task} busy={busy} onUpdate={onUpdate} onCancel={() => setEditing(false)} />
      ) : task.state !== "completed" && (
        <div className="task-assignment-cell">
          <div className="task-current-worker">
            <span className="task-meta-label">Swarm worker</span>
            {targetWorker?.active_session_id ? (
              <button type="button" className="task-owner task-owner-link" onClick={() => onOpenWorker(targetWorker.active_session_id!)}>{targetWorker.name}</button>
            ) : <strong className="task-owner">{targetWorker?.name ?? "Unassigned"}</strong>}
          </div>
          <div className="assignment-row">
            <label className="visually-hidden" htmlFor={`assignment-${task.id}`}>Assign Swarm worker</label>
            <select
              aria-label={`Assign Swarm worker for ${task.title}`}
              id={`assignment-${task.id}`}
              value={targetWorker?.id ?? ""}
              onChange={(event) => void onAssign(task, event.target.value)}
              disabled={busy || assignableWorkers.length === 0}
            >
              <option value="">Unassigned</option>
              {assignableWorkers.map((worker) => (
                <option key={worker.id} value={worker.id}>
                  {worker.name} · {workerAttentionLabel(worker)}
                </option>
              ))}
            </select>
          </div>
          {task.dispatch_state && (
            <p className={`task-dispatch task-dispatch-${task.dispatch_state}`} role="status">
              {dispatchLabels[task.dispatch_state]}
            </p>
          )}
          {task.outcome_delivery_state && (
            <p className={`task-dispatch task-dispatch-${task.outcome_delivery_state}`} role="status">
              {outcomeDeliveryLabels[task.outcome_delivery_state]}
            </p>
          )}
          {!editing && (targetWorker || task.state !== "ready") && <PrimaryTaskAction task={task} assigned={Boolean(assigned?.running)} targetWorker={targetWorker} busy={busy} onTransition={onTransition} onStartWorker={onStartWorker} />}
        </div>
      )}
      <div className="task-actions">
        {!editing && <button className="text-button" disabled={busy} onClick={() => setEditing(true)}>Edit</button>}
        {!editing && jiraLink && <button className="text-button" disabled={busy} onClick={() => void toggleDiscussion()}>{discussionOpen ? "Hide discussion" : "Discussion"}</button>}
        {!editing && (
          <button className="task-menu-trigger" aria-label={`Actions for ${task.title}`} aria-haspopup="menu" aria-expanded={menuOpen} onClick={() => setMenuOpen((current) => !current)}>
            <span aria-hidden="true">•••</span>
          </button>
        )}
      </div>
      {menuOpen && (
        <div className="task-menu" role="menu" aria-label={`${task.title} actions`}>
          <button role="menuitem" onClick={() => runMenuAction(() => setEditing(true))}>Edit task</button>
          <button role="menuitem" onClick={() => runMenuAction(toggleHistory)}>{historyOpen ? "Hide history" : "Show history"}</button>
          {task.state !== "completed" && <button role="menuitem" disabled={busy || !canMoveEarlier} onClick={() => runMenuAction(onMoveEarlier)}>Move earlier</button>}
          {task.state !== "completed" && <button role="menuitem" disabled={busy || !canMoveLater} onClick={() => runMenuAction(onMoveLater)}>Move later</button>}
          {task.state === "active" && <button className="danger-text" role="menuitem" disabled={busy} onClick={() => runMenuAction(() => void onTransition(task, "blocked"))}>Block task</button>}
          {task.state === "review" && <button role="menuitem" disabled={busy} onClick={() => runMenuAction(() => void onTransition(task, "active"))}>Changes needed</button>}
        </div>
      )}
      {historyOpen && (
        <TaskActivityPanel
          activity={activity}
          loading={historyLoading}
          failed={historyError}
          onRetry={() => void loadActivity()}
        />
      )}
      {discussionOpen && jiraLink && (
        <section className="jira-discussion" aria-label={`Jira discussion for ${jiraLink.issue_key}`}>
          <div className="jira-discussion-heading"><strong>Jira discussion</strong><small>Two-way · shared with everyone on the issue</small></div>
          {discussionLoading && comments.length === 0 ? <p>Loading discussion…</p> : null}
          {discussionError ? <p className="settings-error" role="alert">{discussionError}</p> : null}
          {discussionMessage ? <p className="settings-message" role="status">{discussionMessage}</p> : null}
          {comments.length > 0 ? (
            <ol>
              {comments.map((comment) => (
                <li key={comment.id}><span><strong>{comment.author_name}</strong><small>{comment.body}</small></span><time>{new Date(comment.created_at).toLocaleString()}</time></li>
              ))}
            </ol>
          ) : !discussionLoading ? <p>No Jira comments yet.</p> : null}
          <form onSubmit={(event) => void submitComment(event)}>
            <label htmlFor={`jira-comment-${task.id}`}>Add an update</label>
            <textarea id={`jira-comment-${task.id}`} value={commentBody} maxLength={4000} placeholder="Progress, a question, evidence, or a handoff" onChange={(event) => setCommentBody(event.target.value)} />
            <button className="secondary-button" type="submit" disabled={discussionLoading || !commentBody.trim()}>Share to Jira</button>
          </form>
        </section>
      )}
    </article>
  );
}

function TaskActivityPanel({ activity, loading, failed, onRetry }: {
  activity: TaskActivityPage | undefined;
  loading: boolean;
  failed: boolean;
  onRetry: () => void;
}) {
  return (
    <section className="task-history" aria-label="Task history" aria-live="polite">
      {loading ? <p>Loading history…</p> : failed ? (
        <p>History is unavailable. <button className="text-button" type="button" onClick={onRetry}>Retry</button></p>
      ) : activity?.events.length ? (
        <>
        {activity.truncated && <p className="task-history-note">Showing the latest activity.</p>}
        <ol>
          {activity.events.map((entry) => (
            <li key={entry.sequence}>
              <span>
                <span>{activityLabel(entry)}</span>
                {entry.note && <small className="task-history-handoff">{entry.note}</small>}
              </span>
              <time dateTime={new Date(entry.occurred_at * 1000).toISOString()}>{formatActivityTime(entry.occurred_at)}</time>
            </li>
          ))}
        </ol>
        </>
      ) : <p>No history recorded.</p>}
    </section>
  );
}

function activityLabel(activity: TaskActivity): string {
  if (activity.kind === "created") return "Task created";
  if (activity.kind === "details_updated") return "Details updated";
  if (activity.kind === "assigned") return "Worker assigned";
  if (activity.kind === "unassigned") return "Worker released";
  if (activity.from_state && activity.to_state) {
    return `${stateLabels[activity.from_state]} → ${stateLabels[activity.to_state]}`;
  }
  return "State updated";
}

function formatActivityTime(occurredAt: number): string {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(occurredAt * 1000));
}

function TaskEditForm({ task, busy, onUpdate, onCancel }: {
  task: Task;
  busy: boolean;
  onUpdate: Props["onUpdate"];
  onCancel: () => void;
}) {
  const [title, setTitle] = useState(task.title);
  const [description, setDescription] = useState(task.description);
  const [priority, setPriority] = useState(task.priority);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!title.trim()) return;
    try {
      await onUpdate(task, { title, description, priority });
      onCancel();
    } catch {
      // The app-level alert explains the failure; retain this form for correction and retry.
    }
  }

  return (
    <form className="task-edit-form" aria-label={`Edit ${task.title}`} onSubmit={(event) => void submit(event)}>
      <label htmlFor={`edit-title-${task.id}`}>Title</label>
      <input id={`edit-title-${task.id}`} value={title} onChange={(event) => setTitle(event.target.value)} maxLength={240} />
      <label htmlFor={`edit-description-${task.id}`}>Description</label>
      <textarea id={`edit-description-${task.id}`} value={description} onChange={(event) => setDescription(event.target.value)} maxLength={10000} rows={4} />
      <div className="task-edit-row">
        <div className="field-stack">
          <label htmlFor={`edit-priority-${task.id}`}>Priority</label>
          <select id={`edit-priority-${task.id}`} value={priority} onChange={(event) => setPriority(event.target.value as TaskPriority)}>
            {Object.entries(priorityLabels).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
          </select>
        </div>
      </div>
      <div className="task-edit-actions">
        <button disabled={busy || !title.trim()}>Save changes</button>
        <button className="text-button" type="button" disabled={busy} onClick={onCancel}>Cancel</button>
      </div>
    </form>
  );
}

function PrimaryTaskAction({ task, assigned, targetWorker, busy, onTransition, onStartWorker }: {
  task: Task;
  assigned: boolean;
  targetWorker: Worker | undefined;
  busy: boolean;
  onTransition: Props["onTransition"];
  onStartWorker: Props["onStartWorker"];
}) {
  if (task.state === "draft") return <button disabled={busy} onClick={() => void onTransition(task, "ready")}>Mark ready</button>;
  if (task.state === "ready" && !assigned) return <button disabled={busy || !targetWorker} onClick={() => void onStartWorker(task)}>{targetWorker ? `Wake ${targetWorker.name}` : "Choose worker"}</button>;
  if (task.state === "ready") return <button disabled={busy} onClick={() => void onTransition(task, "active")}>Start work</button>;
  if (task.state === "active") return <button disabled={busy} onClick={() => void onTransition(task, "review")}>Send to review</button>;
  if (task.state === "blocked") return <button disabled={busy} onClick={() => void onTransition(task, "active")}>Resume work</button>;
  if (task.state === "review") return <button disabled={busy} onClick={() => void onTransition(task, "completed")}>Complete</button>;
  return null;
}

export function workerName(sessionId: string): string {
  return `Claude ${sessionId.slice(-4).toUpperCase()}`;
}

function repositoryName(workspace: string): string {
  const parts = workspace.split(/[\\/]/).filter(Boolean);
  return parts.at(-1) ?? workspace;
}
