import { useState, type DragEvent } from "react";

import {
  type EmailTaskSource,
  type JiraComment,
  type JiraTaskLink,
  type SessionSummary,
  type Task,
  type TaskActivityPage,
  type TaskState,
  type TaskUpdateInput,
  type Worker,
} from "../api";
import EmailResolutionPanel from "./EmailResolutionPanel";
import CursorMenu, { pointFromElement, type MenuPoint } from "../shared/CursorMenu";
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

export default function TaskCard({ task, jiraLink, emailSources, operatorToken, sessions, workers, busy, onUpdate, onTransition, onAssign, onStartWorker, onOpenWorker, onFetchActivity, onFetchJiraComments, onAddJiraComment, onRetryJira, canMoveEarlier, canMoveLater, onMoveEarlier, onMoveLater, onDropBefore, dropTarget, onDragTarget, onDragLeave, onDragStart, onDragEnd }: TaskCardProps) {
  const assigned = sessions.find((session) => session.session_id === task.assigned_session_id);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [activity, setActivity] = useState<TaskActivityPage>();
  const [historyError, setHistoryError] = useState(false);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [menuPoint, setMenuPoint] = useState<MenuPoint>();
  const [discussionOpen, setDiscussionOpen] = useState(false);
  const [emailDetailsOpen, setEmailDetailsOpen] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);

  function runMenuAction(action: () => void) {
    setMenuPoint(undefined);
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
      data-task-id={task.id}
      tabIndex={-1}
      className={`task-card state-${task.state}${dropTarget ? " drop-target-before" : ""}`}
      aria-label={task.title}
      draggable={!busy && task.state !== "completed" && (canMoveEarlier || canMoveLater)}
      onDragStart={(event: DragEvent<HTMLElement>) => { event.dataTransfer.effectAllowed = "move"; event.dataTransfer.setData("text/plain", task.id); onDragStart(task.id); }}
      onDragEnd={onDragEnd}
      onDragEnter={onDragTarget}
      onDragOver={(event) => { event.preventDefault(); onDragTarget(); }}
      onDragLeave={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) onDragLeave(); }}
      onDrop={(event) => { event.preventDefault(); onDropBefore(); }}
      onContextMenu={(event) => { event.preventDefault(); setMenuPoint({ x: event.clientX, y: event.clientY }); }}
      onDoubleClick={(event) => openDetailsFromEvent(event.target)}
      onKeyDown={(event) => { if (event.key === "Escape") setMenuPoint(undefined); if (event.key === "Enter") openDetailsFromEvent(event.target); }}
    >
      <TaskMetadata task={task} jiraLink={jiraLink} busy={busy} onRetryJira={onRetryJira} />
      <h4>{task.title}</h4>
      {task.description && <p className="task-description">{task.description}</p>}
      {task.state !== "completed" && (
        <TaskAssignment task={task} workers={workers} workerRunning={Boolean(assigned?.running)} busy={busy} onAssign={onAssign} onOpenWorker={onOpenWorker} onTransition={onTransition} onStartWorker={onStartWorker} />
      )}
      <div className="task-actions">
        <button className="text-button" disabled={busy} onClick={() => setDetailsOpen(true)}>Edit</button>
        {jiraLink && <button className="text-button" disabled={busy} onClick={toggleDiscussion}>{discussionOpen ? "Hide discussion" : "Discussion"}</button>}
        {emailSources.length > 0 && <button className="text-button" disabled={busy} onClick={() => setEmailDetailsOpen((current) => !current)}>{emailDetailsOpen ? "Hide email" : task.state === "completed" ? "Close email loop" : emailSources.length === 1 ? "Email source" : `${emailSources.length} email sources`}</button>}
        <button className="task-menu-trigger" aria-label={`Actions for ${task.title}`} aria-haspopup="menu" aria-expanded={Boolean(menuPoint)} onClick={(event) => {
          const point = pointFromElement(event.currentTarget);
          setMenuPoint((current) => current ? undefined : point);
        }}>
          <span aria-hidden="true">•••</span>
        </button>
      </div>
      {menuPoint && (
        <CursorMenu className="task-menu" point={menuPoint} onClose={() => setMenuPoint(undefined)} label={`${task.title} actions`}>
          <button role="menuitem" onClick={() => runMenuAction(() => setDetailsOpen(true))}>Review and edit</button>
          <button role="menuitem" onClick={() => runMenuAction(toggleHistory)}>{historyOpen ? "Hide history" : "Show history"}</button>
          {task.state !== "completed" && <button role="menuitem" disabled={busy || !canMoveEarlier} onClick={() => runMenuAction(onMoveEarlier)}>Move earlier</button>}
          {task.state !== "completed" && <button role="menuitem" disabled={busy || !canMoveLater} onClick={() => runMenuAction(onMoveLater)}>Move later</button>}
          {task.state === "active" && <button className="danger-text" role="menuitem" disabled={busy} onClick={() => runMenuAction(() => void onTransition(task, "blocked"))}>Block task</button>}
          {task.state === "review" && <button role="menuitem" disabled={busy} onClick={() => runMenuAction(() => void onTransition(task, "active"))}>Changes needed</button>}
        </CursorMenu>
      )}
      {historyOpen && <TaskActivityPanel activity={activity} loading={historyLoading} failed={historyError} onRetry={() => void loadActivity()} />}
      {discussionOpen && jiraLink && <JiraDiscussion taskId={task.id} issueKey={jiraLink.issue_key} onFetch={onFetchJiraComments} onAdd={onAddJiraComment} />}
      {emailDetailsOpen && emailSources.length > 0 && <EmailResolutionPanel operatorToken={operatorToken} task={task} sources={emailSources} />}
      {detailsOpen && <TaskDetailDialog task={task} jiraLink={jiraLink} emailSources={emailSources} operatorToken={operatorToken} busy={busy} onClose={() => setDetailsOpen(false)} onSave={(input) => onUpdate(task, input)} />}
    </article>
  );
}
