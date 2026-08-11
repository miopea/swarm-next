import { useState, type FormEvent } from "react";

import type { SessionSummary, Task, TaskState } from "../api";
import BeeMascot from "../brand/BeeMascot";

type Props = {
  tasks: Task[];
  sessions: SessionSummary[];
  workerNames: ReadonlyMap<string, string>;
  busy: boolean;
  onCreate: (title: string, workspace: string) => Promise<void>;
  onTransition: (task: Task, state: TaskState) => Promise<void>;
  onAssign: (task: Task, sessionId: string) => Promise<void>;
  onStartWorker: (task: Task) => Promise<void>;
};

const stateLabels: Record<TaskState, string> = {
  draft: "Draft",
  ready: "Ready",
  active: "In progress",
  blocked: "Blocked",
  review: "Review",
  completed: "Completed",
};

export default function TaskBoard({
  tasks,
  sessions,
  workerNames,
  busy,
  onCreate,
  onTransition,
  onAssign,
  onStartWorker,
}: Props) {
  const [title, setTitle] = useState("");
  const [workspace, setWorkspace] = useState("");

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!title.trim() || !workspace.trim()) return;
    await onCreate(title, workspace);
    setTitle("");
    setWorkspace("");
  }

  const openTasks = tasks.filter((task) => task.state !== "completed");
  const completedTasks = tasks.filter((task) => task.state === "completed");

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
            {openTasks.map((task) => (
              <TaskCard
                key={task.id}
                task={task}
                sessions={sessions}
                workerNames={workerNames}
                busy={busy}
                onTransition={onTransition}
                onAssign={onAssign}
                onStartWorker={onStartWorker}
              />
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
                onTransition={onTransition}
                onAssign={onAssign}
                onStartWorker={onStartWorker}
              />
            ))}
          </div>
        </details>
      )}
    </div>
  );
}

function TaskCard({ task, sessions, workerNames, busy, onTransition, onAssign, onStartWorker }: Omit<Props, "tasks" | "onCreate"> & { task: Task }) {
  const assigned = sessions.find((session) => session.session_id === task.assigned_session_id);
  const runningSessions = sessions.filter((session) => session.running);
  return (
    <article className="task-card">
      <div className="task-card-topline">
        <span className={`task-state state-${task.state}`}>{stateLabels[task.state]}</span>
        <code>{shortWorkspace(task.workspace)}</code>
      </div>
      <h4>{task.title}</h4>
      {task.state !== "completed" && (
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
        <PrimaryTaskAction task={task} assigned={Boolean(assigned?.running)} busy={busy} onTransition={onTransition} onStartWorker={onStartWorker} />
        {task.state === "active" && <button className="text-button danger-text" disabled={busy} onClick={() => void onTransition(task, "blocked")}>Block</button>}
        {task.state === "review" && <button className="text-button" disabled={busy} onClick={() => void onTransition(task, "active")}>Changes needed</button>}
      </div>
    </article>
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
