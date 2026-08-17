import { useEffect, useRef, useState, type DragEvent, type FormEvent } from "react";

import {
  type EmailTaskSource,
  type JiraComment,
  type JiraTaskLink,
  type SessionSummary,
  type Task,
  type TaskActivityPage,
  type TaskPriority,
  type TaskState,
  type TaskUpdateInput,
  type Worker,
} from "../api";
import EmailResolutionPanel from "./EmailResolutionPanel";
import JiraDiscussion from "./JiraDiscussion";
import TaskActivityPanel from "./TaskActivityPanel";
import TaskAssignment from "./TaskAssignment";
import TaskDetailDialog from "./TaskDetailDialog";
import TaskMetadata from "./TaskMetadata";

export type TaskCardProps = {
  task: Task;
  jiraLink?: JiraTaskLink;
  emailSources: EmailTaskSource[];
  operatorToken: string;
  sessions: SessionSummary[];
  workers: Worker[];
  busy: boolean;
  onUpdate: (task: Task, input: TaskUpdateInput) => Promise<void>;
  onTransition: (task: Task, state: TaskState, note?: string) => Promise<void>;
  onAssign: (task: Task, workerId: string) => Promise<void>;
  onStartWorker: (task: Task) => Promise<void>;
  onOpenWorker: (sessionId: string) => void;
  onFetchActivity: (taskId: string) => Promise<TaskActivityPage>;
  onFetchJiraComments: (taskId: string) => Promise<JiraComment[]>;
  onAddJiraComment: (taskId: string, body: string) => Promise<{ state: string }>;
  onRetryJira: (task: Task) => Promise<void>;
  canMoveEarlier: boolean;
  canMoveLater: boolean;
  onMoveEarlier: () => void;
  onMoveLater: () => void;
  onDropBefore: () => void;
  dropTarget: boolean;
  onDragTarget: () => void;
  onDragLeave: () => void;
  onDragStart: (taskId: string) => void;
  onDragEnd: () => void;
};

const priorityLabels: Record<TaskPriority, string> = {
  low: "Low",
  normal: "Normal",
  high: "High",
  urgent: "Urgent",
};

export default function TaskCard({ task, jiraLink, emailSources, operatorToken, sessions, workers, busy, onUpdate, onTransition, onAssign, onStartWorker, onOpenWorker, onFetchActivity, onFetchJiraComments, onAddJiraComment, onRetryJira, canMoveEarlier, canMoveLater, onMoveEarlier, onMoveLater, onDropBefore, dropTarget, onDragTarget, onDragLeave, onDragStart, onDragEnd }: TaskCardProps) {
  const assigned = sessions.find((session) => session.session_id === task.assigned_session_id);
  const [editing, setEditing] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [activity, setActivity] = useState<TaskActivityPage>();
  const [historyError, setHistoryError] = useState(false);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [discussionOpen, setDiscussionOpen] = useState(false);
  const [emailDetailsOpen, setEmailDetailsOpen] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);
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

  function toggleDiscussion() {
    setDiscussionOpen((current) => !current);
  }

  function openDetailsFromEvent(target: EventTarget | null) {
    if (target instanceof Element && target.closest("button, a, input, select, textarea, form")) return;
    setDetailsOpen(true);
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
      onDoubleClick={(event) => openDetailsFromEvent(event.target)}
      onKeyDown={(event) => { if (event.key === "Escape") setMenuOpen(false); if (event.key === "Enter") openDetailsFromEvent(event.target); }}
    >
      <TaskMetadata task={task} jiraLink={jiraLink} busy={busy} onRetryJira={onRetryJira} />
      <h4>{task.title}</h4>
      {task.description && !editing && <p className="task-description">{task.description}</p>}
      {editing ? (
        <TaskEditForm task={task} busy={busy} onUpdate={onUpdate} onCancel={() => setEditing(false)} />
      ) : task.state !== "completed" && (
        <TaskAssignment task={task} workers={workers} workerRunning={Boolean(assigned?.running)} busy={busy} onAssign={onAssign} onOpenWorker={onOpenWorker} onTransition={onTransition} onStartWorker={onStartWorker} />
      )}
      <div className="task-actions">
        {!editing && <button className="text-button" disabled={busy} onClick={() => setEditing(true)}>Edit</button>}
        {!editing && jiraLink && <button className="text-button" disabled={busy} onClick={toggleDiscussion}>{discussionOpen ? "Hide discussion" : "Discussion"}</button>}
        {!editing && emailSources.length > 0 && <button className="text-button" disabled={busy} onClick={() => setEmailDetailsOpen((current) => !current)}>{emailDetailsOpen ? "Hide email" : task.state === "completed" ? "Close email loop" : emailSources.length === 1 ? "Email source" : `${emailSources.length} email sources`}</button>}
        {!editing && (
          <button className="task-menu-trigger" aria-label={`Actions for ${task.title}`} aria-haspopup="menu" aria-expanded={menuOpen} onClick={() => setMenuOpen((current) => !current)}>
            <span aria-hidden="true">•••</span>
          </button>
        )}
      </div>
      {menuOpen && (
        <div className="task-menu" role="menu" aria-label={`${task.title} actions`}>
          <button role="menuitem" onClick={() => runMenuAction(() => setDetailsOpen(true))}>View details</button>
          <button role="menuitem" onClick={() => runMenuAction(() => setEditing(true))}>Edit task</button>
          <button role="menuitem" onClick={() => runMenuAction(toggleHistory)}>{historyOpen ? "Hide history" : "Show history"}</button>
          {task.state !== "completed" && <button role="menuitem" disabled={busy || !canMoveEarlier} onClick={() => runMenuAction(onMoveEarlier)}>Move earlier</button>}
          {task.state !== "completed" && <button role="menuitem" disabled={busy || !canMoveLater} onClick={() => runMenuAction(onMoveLater)}>Move later</button>}
          {task.state === "active" && <button className="danger-text" role="menuitem" disabled={busy} onClick={() => runMenuAction(() => void onTransition(task, "blocked"))}>Block task</button>}
          {task.state === "review" && <button role="menuitem" disabled={busy} onClick={() => runMenuAction(() => void onTransition(task, "active"))}>Changes needed</button>}
        </div>
      )}
      {historyOpen && <TaskActivityPanel activity={activity} loading={historyLoading} failed={historyError} onRetry={() => void loadActivity()} />}
      {discussionOpen && jiraLink && <JiraDiscussion taskId={task.id} issueKey={jiraLink.issue_key} onFetch={onFetchJiraComments} onAdd={onAddJiraComment} />}
      {emailDetailsOpen && emailSources.length > 0 && <EmailResolutionPanel operatorToken={operatorToken} task={task} sources={emailSources} />}
      {detailsOpen && <TaskDetailDialog task={task} jiraLink={jiraLink} operatorToken={operatorToken} onClose={() => setDetailsOpen(false)} onEdit={() => { setDetailsOpen(false); setEditing(true); }} />}
    </article>
  );
}

function TaskEditForm({ task, busy, onUpdate, onCancel }: {
  task: Task;
  busy: boolean;
  onUpdate: TaskCardProps["onUpdate"];
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
