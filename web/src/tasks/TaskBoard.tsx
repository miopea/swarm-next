import { useEffect, useRef, useState, type DragEvent, type FormEvent } from "react";

import type { SessionSummary, Task, TaskActivity, TaskActivityPage, TaskDraftInput, TaskPriority, TaskState, TaskUpdateInput } from "../api";
import BeeMascot from "../brand/BeeMascot";

type Props = {
  tasks: Task[];
  sessions: SessionSummary[];
  workerNames: ReadonlyMap<string, string>;
  busy: boolean;
  onCreate: (input: TaskDraftInput) => Promise<void>;
  onUpdate: (task: Task, input: TaskUpdateInput) => Promise<void>;
  onTransition: (task: Task, state: TaskState) => Promise<void>;
  onAssign: (task: Task, sessionId: string) => Promise<void>;
  onStartWorker: (task: Task) => Promise<void>;
  onFetchActivity: (taskId: string) => Promise<TaskActivityPage>;
  onReorder: (taskIds: string[]) => Promise<void>;
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
  review: ["active", "completed"],
  completed: [],
};

export default function TaskBoard({
  tasks,
  sessions,
  workerNames,
  busy,
  onCreate,
  onUpdate,
  onTransition,
  onAssign,
  onStartWorker,
  onFetchActivity,
  onReorder,
}: Props) {
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [priority, setPriority] = useState<TaskPriority>("normal");
  const [workspace, setWorkspace] = useState("");
  const [draggedTaskId, setDraggedTaskId] = useState<string>();

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!title.trim() || !workspace.trim()) return;
    await onCreate({ title, description, priority, workspace });
    setTitle("");
    setDescription("");
    setPriority("normal");
    setWorkspace("");
  }

  const openTasks = tasks
    .filter((task) => task.state !== "completed")
    .sort((left, right) => left.position - right.position);
  const completedTasks = tasks.filter((task) => task.state === "completed");
  const draggedTask = tasks.find((task) => task.id === draggedTaskId);

  function reorderBefore(targetTaskId: string) {
    if (!draggedTaskId || draggedTaskId === targetTaskId) return;
    const taskIds = openTasks.map((task) => task.id).filter((taskId) => taskId !== draggedTaskId);
    taskIds.splice(taskIds.indexOf(targetTaskId), 0, draggedTaskId);
    setDraggedTaskId(undefined);
    void onReorder(taskIds);
  }

  function moveTaskAt(index: number, offset: -1 | 1) {
    const target = index + offset;
    if (target < 0 || target >= openTasks.length) return;
    const taskIds = openTasks.map((task) => task.id);
    [taskIds[index], taskIds[target]] = [taskIds[target], taskIds[index]];
    void onReorder(taskIds);
  }

  return (
    <div className="task-board">
      <section className="task-compose" aria-labelledby="new-task-heading">
        <div>
          <p className="eyebrow">New work</p>
          <h3 id="new-task-heading">Give the next worker a clear outcome</h3>
          <p>Tasks remember the workspace, assignment, and every lifecycle transition.</p>
        </div>
        <form onSubmit={(event) => void submit(event)}>
          <div className="field-stack task-title-field">
            <label htmlFor="task-title">Task title</label>
            <input
              id="task-title"
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
          <div className="field-stack">
            <label htmlFor="task-workspace">Workspace</label>
            <input
              id="task-workspace"
              value={workspace}
              onChange={(event) => setWorkspace(event.target.value)}
              placeholder="/absolute/path/to/workspace"
            />
          </div>
          <button disabled={busy || !title.trim() || !workspace.trim()}>Create draft</button>
        </form>
      </section>

      <section className="task-section" aria-labelledby="active-work-heading">
        <div className="section-heading">
          <div><p className="eyebrow">Queue</p><h3 id="active-work-heading">Active work</h3></div>
          <span className="count-badge">{openTasks.length}</span>
        </div>
        {openTasks.length === 0 ? (
          <div className="empty-card"><BeeMascot className="empty-bee" expression="available" /><div><strong>No work queued</strong><span>Create a focused task when you are ready.</span></div></div>
        ) : (
          <div className="task-grid">
            {openTasks.map((task, index) => (
              <TaskCard
                key={task.id}
                task={task}
                sessions={sessions}
                workerNames={workerNames}
                busy={busy}
                onUpdate={onUpdate}
                onTransition={onTransition}
                onAssign={onAssign}
                onStartWorker={onStartWorker}
                onFetchActivity={onFetchActivity}
                canMoveEarlier={index > 0}
                canMoveLater={index < openTasks.length - 1}
                onMoveEarlier={() => moveTaskAt(index, -1)}
                onMoveLater={() => moveTaskAt(index, 1)}
                onDropBefore={() => reorderBefore(task.id)}
                onDragStart={setDraggedTaskId}
                onDragEnd={() => setDraggedTaskId(undefined)}
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
                  setDraggedTaskId(undefined);
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
        <details className="completed-tasks">
          <summary><span>Completed work</span><small>{completedTasks.length}</small></summary>
          <div className="task-grid compact">
            {completedTasks.map((task) => (
              <TaskCard
                key={task.id}
                task={task}
                sessions={sessions}
                workerNames={workerNames}
                busy={busy}
                onUpdate={onUpdate}
                onTransition={onTransition}
                onAssign={onAssign}
                onStartWorker={onStartWorker}
                onFetchActivity={onFetchActivity}
                canMoveEarlier={false}
                canMoveLater={false}
                onMoveEarlier={() => undefined}
                onMoveLater={() => undefined}
                onDropBefore={() => undefined}
                onDragStart={setDraggedTaskId}
                onDragEnd={() => setDraggedTaskId(undefined)}
              />
            ))}
          </div>
        </details>
      )}
    </div>
  );
}

function TaskCard({ task, sessions, workerNames, busy, onUpdate, onTransition, onAssign, onStartWorker, onFetchActivity, canMoveEarlier, canMoveLater, onMoveEarlier, onMoveLater, onDropBefore, onDragStart, onDragEnd }: Omit<Props, "tasks" | "onCreate" | "onReorder"> & { task: Task; canMoveEarlier: boolean; canMoveLater: boolean; onMoveEarlier: () => void; onMoveLater: () => void; onDropBefore: () => void; onDragStart: (taskId: string) => void; onDragEnd: () => void }) {
  const assigned = sessions.find((session) => session.session_id === task.assigned_session_id);
  const runningSessions = sessions.filter((session) => session.running);
  const [editing, setEditing] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [activity, setActivity] = useState<TaskActivityPage>();
  const [historyError, setHistoryError] = useState(false);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
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
  return (
    <article
      ref={cardRef}
      className="task-card"
      aria-label={task.title}
      draggable={!busy && !editing && task.state !== "completed"}
      onDragStart={(event: DragEvent<HTMLElement>) => { event.dataTransfer.effectAllowed = "move"; event.dataTransfer.setData("text/plain", task.id); onDragStart(task.id); }}
      onDragEnd={onDragEnd}
      onDragOver={(event) => event.preventDefault()}
      onDrop={(event) => { event.preventDefault(); onDropBefore(); }}
      onContextMenu={(event) => { event.preventDefault(); setMenuOpen(true); }}
      onKeyDown={(event) => { if (event.key === "Escape") setMenuOpen(false); }}
    >
      <div className="task-card-topline">
        <div className="task-signals">
          <span className={`task-state state-${task.state}`}>{stateLabels[task.state]}</span>
          <span className={`task-priority priority-${task.priority}`}>{priorityLabels[task.priority]}</span>
        </div>
        <code>{shortWorkspace(task.workspace)}</code>
      </div>
      <h4>{task.title}</h4>
      {task.description && !editing && <p className="task-description">{task.description}</p>}
      {editing ? (
        <TaskEditForm task={task} busy={busy} onUpdate={onUpdate} onCancel={() => setEditing(false)} />
      ) : task.state !== "completed" && (
        <div className="assignment-row">
          <label htmlFor={`assignment-${task.id}`}>Worker</label>
          <select
            id={`assignment-${task.id}`}
            value={task.assigned_session_id ?? ""}
            onChange={(event) => event.target.value && void onAssign(task, event.target.value)}
            disabled={busy || runningSessions.length === 0}
          >
            <option value="">{runningSessions.length === 0 ? "No workers running" : "Unassigned"}</option>
            {runningSessions.map((session) => (
              <option key={session.session_id} value={session.session_id}>
                {workerNames.get(session.session_id) ?? workerName(session.session_id)} · {session.running ? "running" : "exited"}
              </option>
            ))}
          </select>
        </div>
      )}
      <div className="task-actions">
        {!editing && <PrimaryTaskAction task={task} assigned={Boolean(assigned?.running)} busy={busy} onTransition={onTransition} onStartWorker={onStartWorker} />}
        {!editing && <button className="text-button" disabled={busy} onClick={() => setEditing(true)}>Edit</button>}
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
              <span>{activityLabel(entry)}</span>
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
  const [workspace, setWorkspace] = useState(task.workspace);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!title.trim() || !workspace.trim()) return;
    try {
      await onUpdate(task, { title, description, priority, workspace });
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
        <div className="field-stack">
          <label htmlFor={`edit-workspace-${task.id}`}>Workspace</label>
          <input id={`edit-workspace-${task.id}`} value={workspace} onChange={(event) => setWorkspace(event.target.value)} />
        </div>
      </div>
      <div className="task-edit-actions">
        <button disabled={busy || !title.trim() || !workspace.trim()}>Save changes</button>
        <button className="text-button" type="button" disabled={busy} onClick={onCancel}>Cancel</button>
      </div>
    </form>
  );
}

function PrimaryTaskAction({ task, assigned, busy, onTransition, onStartWorker }: {
  task: Task;
  assigned: boolean;
  busy: boolean;
  onTransition: Props["onTransition"];
  onStartWorker: Props["onStartWorker"];
}) {
  if (task.state === "draft") return <button disabled={busy} onClick={() => void onTransition(task, "ready")}>Mark ready</button>;
  if (task.state === "ready" && !assigned) return <button disabled={busy} onClick={() => void onStartWorker(task)}>Start with Claude</button>;
  if (task.state === "ready") return <button disabled={busy} onClick={() => void onTransition(task, "active")}>Start work</button>;
  if (task.state === "active") return <button disabled={busy} onClick={() => void onTransition(task, "review")}>Send to review</button>;
  if (task.state === "blocked") return <button disabled={busy} onClick={() => void onTransition(task, "active")}>Resume work</button>;
  if (task.state === "review") return <button disabled={busy} onClick={() => void onTransition(task, "completed")}>Complete</button>;
  return null;
}

export function workerName(sessionId: string): string {
  return `Claude ${sessionId.slice(-4).toUpperCase()}`;
}

function shortWorkspace(workspace: string): string {
  const parts = workspace.split(/[\\/]/).filter(Boolean);
  return parts.at(-1) ?? workspace;
}
