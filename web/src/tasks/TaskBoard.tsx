import { useEffect, useLayoutEffect, useRef, useState, type FormEvent } from "react";

import {
  claimApiaryTask,
  recordTaskUnverifiable,
  createApiaryTask,
  fetchApiaryMembers,
  fetchApiaryTasks,
  fetchEmailTaskSources,
  fetchRemovedTasks,
  fetchFederationTaskOutbox,
  fetchLocalApiaryTaskExecutions,
  fetchMyFederationStewardship,
  materializeLocalApiaryTaskExecution,
  queueFederationStewardTask,
  type ApiaryMember,
  type ApiaryTask,
  type EmailTaskSource,
  type FederationStewardshipSnapshot,
  type FederationTaskOutboxEntry,
  type HiveIdentity,
  type JiraComment,
  type JiraTaskLink,
  type LocalApiaryTaskExecution,
  type SessionSummary,
  type Task,
  type TaskActivityPage,
  type TaskDraftInput,
  type TaskPriority,
  type TaskState,
  type TaskUpdateInput,
  type Worker,
} from "../api";
import BeeMascot from "../brand/BeeMascot";
import { useReorderDrag } from "../shared/useReorderDrag";
import JiraTaskIntake from "./JiraTaskIntake";
import EmailTaskIntake from "./EmailTaskIntake";
import TaskBoardControls, { type TaskBoardFilter, type TaskBoardSort, type TaskBoardSource, type TaskProjectChoice } from "./TaskBoardControls";
import TaskCard from "./TaskCard";
import { buildTaskBoardView } from "./taskBoardModel";

import { TITLE_BYTE_LIMIT, clampTitleToBytes, titleByteLength, titleFits } from "./titleLimit";

type Props = {
  tasks: Task[];
  jiraTaskLinks: JiraTaskLink[];
  operatorToken: string;
  hiveIdentity?: HiveIdentity;
  focusTaskId?: string;
  focusRequest?: number;
  composeRequest?: number;
  sessions: SessionSummary[];
  workers: Worker[];
  busy: boolean;
  onCreate: (input: TaskDraftInput) => Promise<void>;
  onUpdate: (task: Task, input: TaskUpdateInput) => Promise<void>;
  onRemove: (task: Task) => Promise<void>;
  onRestore: (task: Task) => Promise<void>;
  onTransition: (task: Task, state: TaskState, note?: string) => Promise<void>;
  onAssign: (task: Task, workerId: string) => Promise<void>;
  onStartWorker: (task: Task) => Promise<void>;
  onOpenWorker: (sessionId: string) => void;
  onOpenTask?: (taskId: string) => void;
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
  awaiting_release: "Awaiting release",
  completed: "Completed",
  abandoned: "Abandoned",
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
  // Parking work for a release is a drag; CLOSING it is not. It settles itself
  // when the deployment is recorded, so the only manual move offered here is
  // back to Active for work a release proved unfinished.
  awaiting_release: ["active"],
  completed: [],
  // Terminal, like completed. Reopening closed work is a correction, not a drag.
  abandoned: [],
};

export default function TaskBoard({
  tasks,
  jiraTaskLinks,
  operatorToken,
  hiveIdentity,
  focusTaskId,
  focusRequest,
  composeRequest,
  sessions,
  workers,
  busy,
  onCreate,
  onUpdate,
  onRemove,
  onRestore,
  onTransition,
  onAssign,
  onStartWorker,
  onOpenWorker,
  onOpenTask,
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
  // Which unverified row is being recorded, and the operator's reason. Kept
  // here rather than in TaskCard because the control belongs to THIS panel:
  // the same card appears elsewhere on the board where the action makes no
  // sense.
  const [unverifiableFor, setUnverifiableFor] = useState<string | null>(null);
  const [unverifiableNote, setUnverifiableNote] = useState("");
  const [title, setTitle] = useState("");
  // The server counts UTF-8 bytes; a maxLength counts UTF-16 units. They
  // agree only for ASCII, so a pasted subject can look short and be refused.
  const titleTooLong = title.trim().length > 0 && !titleFits(title);
  const [description, setDescription] = useState("");
  const [priority, setPriority] = useState<TaskPriority>("normal");
  const [workScope, setWorkScope] = useState<"hive" | "apiary">("hive");
  const [targetHiveId, setTargetHiveId] = useState("");
  const [apiaryTasks, setApiaryTasks] = useState<ApiaryTask[]>([]);
  const [apiaryMembers, setApiaryMembers] = useState<ApiaryMember[]>([]);
  const [apiaryExecutions, setApiaryExecutions] = useState<LocalApiaryTaskExecution[]>([]);
  const [apiaryOutbox, setApiaryOutbox] = useState<FederationTaskOutboxEntry[]>([]);
  const [stewardship, setStewardship] = useState<FederationStewardshipSnapshot | null>(null);
  const [apiaryRefreshState, setApiaryRefreshState] = useState<"idle" | "loading" | "ready" | "partial">("idle");
  const [apiaryWorkerChoices, setApiaryWorkerChoices] = useState<Record<string, string>>({});
  const [actingApiaryTask, setActingApiaryTask] = useState<string>();
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string>();
  const assignableWorkers = workers.filter((worker) => worker.role !== "queen");
  const [workerId, setWorkerId] = useState("");
  const [composeOpen, setComposeOpen] = useState(false);
  const [jiraOpen, setJiraOpen] = useState(false);
  const [emailOpen, setEmailOpen] = useState(false);
  const [jiraMounted, setJiraMounted] = useState(false);
  const [emailMounted, setEmailMounted] = useState(false);
  const [emailTaskSources, setEmailTaskSources] = useState<EmailTaskSource[]>([]);
  const [emailSourcesLoadError, setEmailSourcesLoadError] = useState(false);
  const [emailSourcesAttempt, setEmailSourcesAttempt] = useState(0);
  const [removedTasks, setRemovedTasks] = useState<Task[]>([]);
  const [removedTasksLoadError, setRemovedTasksLoadError] = useState(false);
  const [removedTasksAttempt, setRemovedTasksAttempt] = useState(0);
  const [restoringTaskId, setRestoringTaskId] = useState<string>();
  const [restoreError, setRestoreError] = useState<string>();
  const titleInput = useRef<HTMLInputElement>(null);
  const completedTasksPanel = useRef<HTMLDetailsElement>(null);
  /**
   * Whether the completed panel is open, held in state rather than read off the
   * DOM, because its CONTENTS are now gated on it.
   *
   * Completed work is the large majority of a long-lived Hive. Measured on the
   * operator's board 2026-09-02: 560 tasks, of which 462 sit inside this
   * collapsed panel and only 98 render above it. Collapsed costs no layout and
   * no paint, so this was invisible -- but React still built and reconciled
   * every one of those cards on every board render, and each card runs a find()
   * over the Jira links and a filter() over the email sources while it does.
   *
   * Same fixture, same machine, rendering that exact board shape:
   *
   *   before   13,073 DOM nodes   645-951ms
   *   after     2,891 DOM nodes   174-332ms
   */
  const [completedOpen, setCompletedOpen] = useState(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!title.trim()) return;
    setCreating(true);
    setCreateError(undefined);
    try {
      if (workScope === "apiary") {
        if (isStewardCreator) {
          if (!targetHiveId) return;
          await queueFederationStewardTask(operatorToken, { target_hive_id: targetHiveId, title: title.trim(), description: description.trim(), priority });
        } else {
          await createApiaryTask(operatorToken, { title: title.trim(), description: description.trim(), priority, home_hive_id: targetHiveId || undefined });
        }
        await refreshApiaryWork();
      } else {
        if (!workerId) return;
        await onCreate({ title, description, priority, worker_id: workerId });
      }
      setTitle("");
      setDescription("");
      setPriority("normal");
      setTargetHiveId("");
    } catch {
      setCreateError(workScope === "apiary" ? "The Apiary task was not created. Existing work is unchanged." : "The task was not created. Existing work is unchanged.");
    } finally {
      setCreating(false);
    }
  }

  const apiaryContext = hiveIdentity?.apiary_context;
  const inApiary = apiaryContext?.mode === "federated";
  const isKeeper = inApiary && apiaryContext.local_role === "keeper";
  const isStewardCreator = inApiary && apiaryContext.local_role === "member" && Boolean(stewardship?.stewardship?.capabilities.includes("assign"));
  const canCreateApiaryWork = isKeeper || isStewardCreator;
  const managedHiveIds = stewardship?.stewardship?.managed_hive_ids ?? [];
  const apiaryTargetMembers = isStewardCreator
    ? apiaryMembers.filter((member) => managedHiveIds.includes(member.hive_id))
    : apiaryMembers.filter((member) => member.role === "member");

  async function refreshApiaryWork() {
    if (!inApiary) {
      setApiaryTasks([]);
      setApiaryMembers([]);
      setApiaryExecutions([]);
      setApiaryOutbox([]);
      setStewardship(null);
      setApiaryRefreshState("idle");
      return;
    }
    setApiaryRefreshState("loading");
    const [shared, members, executions, outbox, stewardshipResult] = await Promise.allSettled([
      fetchApiaryTasks(operatorToken),
      fetchApiaryMembers(operatorToken),
      fetchLocalApiaryTaskExecutions(operatorToken),
      fetchFederationTaskOutbox(operatorToken),
      fetchMyFederationStewardship(operatorToken),
    ]);
    if (shared.status === "fulfilled") setApiaryTasks(Array.isArray(shared.value) ? shared.value : []);
    if (members.status === "fulfilled") setApiaryMembers(Array.isArray(members.value) ? members.value : []);
    if (executions.status === "fulfilled") setApiaryExecutions(Array.isArray(executions.value) ? executions.value : []);
    if (outbox.status === "fulfilled") setApiaryOutbox(Array.isArray(outbox.value) ? outbox.value : []);
    if (stewardshipResult.status === "fulfilled") setStewardship(stewardshipResult.value);
    setApiaryRefreshState([shared, members, executions, outbox, stewardshipResult].some((result) => result.status === "rejected") ? "partial" : "ready");
  }

  async function claimSharedTask(task: ApiaryTask) {
    setActingApiaryTask(task.id);
    setCreateError(undefined);
    try {
      await claimApiaryTask(operatorToken, task.id);
      await refreshApiaryWork();
    } catch {
      setCreateError("This Apiary task could not be claimed. Its current owner is unchanged.");
    } finally {
      setActingApiaryTask(undefined);
    }
  }

  async function sendSharedTaskToWorker(task: ApiaryTask) {
    const selectedWorker = apiaryWorkerChoices[task.id];
    if (!selectedWorker) return;
    setActingApiaryTask(task.id);
    setCreateError(undefined);
    try {
      const execution = await materializeLocalApiaryTaskExecution(operatorToken, task.id, selectedWorker);
      await refreshApiaryWork();
      onOpenTask?.(execution.local_task_id);
    } catch {
      setCreateError("This Apiary task could not be sent to the worker. Existing ownership is unchanged.");
    } finally {
      setActingApiaryTask(undefined);
    }
  }

  useEffect(() => {
    if (workerId && !assignableWorkers.some((worker) => worker.id === workerId)) setWorkerId("");
  }, [assignableWorkers, workerId]);

  const taskView = buildTaskBoardView(tasks, jiraTaskLinks, workers, { text: query, filter, source, sort, project, worker }, new Set(emailTaskSources.map((item) => item.task_id)));
  useEffect(() => {
    let cancelled = false;
    void fetchEmailTaskSources(operatorToken)
      .then((sources) => {
        if (cancelled) return;
        setEmailTaskSources(Array.isArray(sources) ? sources : []);
        setEmailSourcesLoadError(false);
      })
      .catch(() => { if (!cancelled) setEmailSourcesLoadError(true); });
    return () => { cancelled = true; };
  }, [emailSourcesAttempt, operatorToken, tasks]);
  useEffect(() => {
    let cancelled = false;
    void fetchRemovedTasks(operatorToken)
      .then((removed) => {
        if (cancelled) return;
        setRemovedTasks(Array.isArray(removed) ? removed : []);
        setRemovedTasksLoadError(false);
      })
      .catch(() => { if (!cancelled) setRemovedTasksLoadError(true); });
    return () => { cancelled = true; };
  }, [operatorToken, removedTasksAttempt, tasks]);
  useEffect(() => {
    void refreshApiaryWork();
  }, [operatorToken, inApiary]);
  useEffect(() => {
    if (!canCreateApiaryWork && workScope === "apiary") setWorkScope("hive");
  }, [canCreateApiaryWork, workScope]);
  const { open: openTasks, unverified: unverifiedTasks, completed: completedTasks, jiraByTask } = taskView;
  const focusedTaskCompleted = Boolean(focusTaskId && completedTasks.some((task) => task.id === focusTaskId));
  const canReorder = sort === "queue" && !query.trim() && filter === "all";
  // No refresh prop: the write emits a TasksChanged control-room event and the
  // board already re-reads on that stream, which is how every other mutation
  // here lands.
  const markUnverifiable = async (task: Task) => {
    const note = unverifiableNote.trim();
    if (!note) return;
    await recordTaskUnverifiable(operatorToken, task.id, note);
    setUnverifiableFor(null);
    setUnverifiableNote("");
  };

  const taskReorder = useReorderDrag(openTasks.map((task) => task.id), (taskIds) => void onReorder(taskIds));
  const draggedTask = tasks.find((task) => task.id === taskReorder.draggedId);

  useLayoutEffect(() => {
    if (!focusTaskId) return;
    if (focusedTaskCompleted) setCompletedOpen(true);
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

  function clearTaskView() {
    onQueryChange?.("");
    onFilterChange?.("all");
    onSourceChange?.("all");
    onProjectChange?.("all");
    onWorkerChange?.("all");
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
            onClick={() => {
              setJiraMounted(true);
              setJiraOpen((current) => !current);
              setComposeOpen(false);
              setEmailOpen(false);
            }}
          >
            {jiraOpen ? "Close Jira work" : "Claim Jira work"}
          </button>
          <button
            type="button"
            className={emailOpen ? "primary-action" : "secondary-button"}
            aria-expanded={emailOpen}
            aria-controls="email-work-source"
            onClick={() => {
              setEmailMounted(true);
              setEmailOpen((current) => !current);
              setComposeOpen(false);
              setJiraOpen(false);
            }}
          >
            {emailOpen ? "Close email" : "Use email"}
          </button>
          </div>
        </div>
        {composeOpen && <form id="new-task-form" onSubmit={(event) => void submit(event)}>
          {canCreateApiaryWork ? <fieldset className="task-scope-field">
            <legend>Who owns this work?</legend>
            <label className={workScope === "hive" ? "selected" : ""}><input type="radio" name="task-scope" checked={workScope === "hive"} onChange={() => setWorkScope("hive")} /><span><strong>This Hive</strong><small>Private work for one local worker</small></span></label>
            <label className={workScope === "apiary" ? "selected" : ""}><input type="radio" name="task-scope" checked={workScope === "apiary"} onChange={() => setWorkScope("apiary")} /><span><strong>Apiary</strong><small>{isStewardCreator ? "Shared work for a Hive in your Steward scope" : "Shared work owned by a Hive"}</small></span></label>
            <small>{workScope === "hive" ? "Private to this Hive and assigned to one repository worker." : isStewardCreator ? "Sent through Keeper to the selected Hive. Its workers and repositories remain private." : "Shared across the Apiary. A Hive may claim it, or Keeper may route it."}</small>
          </fieldset> : null}
          <div className="field-stack task-title-field">
            <label htmlFor="task-title">Task title</label>
            <input
              id="task-title"
              ref={titleInput}
              autoFocus={Boolean(composeRequest)}
              value={title}
              onChange={(event) => setTitle(event.target.value)}
              placeholder="What should be true when this is done?"
              aria-invalid={titleTooLong || undefined}
            />
            {titleTooLong ? (
              <small className="task-title-limit" role="status">
                {titleByteLength(title.trim()) - TITLE_BYTE_LIMIT} too long. Punctuation copied from an email can count for more than one character.
                <button type="button" className="text-button" onClick={() => setTitle(clampTitleToBytes(title))}>Shorten it for me</button>
              </small>
            ) : null}
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
          {workScope === "hive" ? <div className="field-stack task-worker-field">
            <label htmlFor="task-worker">Who should handle this?</label>
            <select id="task-worker" value={workerId} onChange={(event) => setWorkerId(event.target.value)}>
              <option value="">{assignableWorkers.length === 0 ? "Configure a worker first" : "Choose a worker"}</option>
              {assignableWorkers.map((worker) => (
                <option key={worker.id} value={worker.id}>{worker.name} · {repositoryName(worker.workspace)}</option>
              ))}
            </select>
          </div> : <div className="field-stack task-worker-field">
            <label htmlFor="task-home-hive">Route to a Hive <span>optional</span></label>
            <select id="task-home-hive" value={targetHiveId} onChange={(event) => setTargetHiveId(event.target.value)}>
              <option value="">{isStewardCreator ? "Choose a managed Hive" : "Unassigned · any Member may claim"}</option>
              {apiaryTargetMembers.map((member) => <option key={member.hive_id} value={member.hive_id}>{member.hive_name} · {member.operator_display_name}</option>)}
            </select>
          </div>}
          <button disabled={busy || creating || !title.trim() || titleTooLong || (workScope === "hive" && !workerId) || (workScope === "apiary" && isStewardCreator && !targetHiveId)}>{creating ? "Creating…" : workScope === "apiary" ? isStewardCreator ? "Route through Keeper" : "Create for Apiary" : "Create draft"}</button>
          {createError ? <p className="form-error task-create-error" role="alert">{createError}</p> : null}
        </form>}
        {jiraMounted ? <div id="jira-work-source" hidden={!jiraOpen}><JiraTaskIntake operatorToken={operatorToken} onImported={onJiraImported} /></div> : null}
        {emailMounted ? <div id="email-work-source" hidden={!emailOpen}><EmailTaskIntake operatorToken={operatorToken} workers={workers} onImported={onEmailImported} /></div> : null}
      </section>

      <details className="task-mobile-controls">
        <summary>Find, filter, and sort <span>{openTasks.length}/{taskView.allOpenCount}</span></summary>
        <TaskBoardControls query={query} filter={filter} source={source} sort={sort} project={project} worker={worker} workers={workers} projects={projects} openCount={taskView.allOpenCount} busy={busy} onQueryChange={(value) => onQueryChange?.(value)} onFilterChange={(value) => onFilterChange?.(value)} onSourceChange={(value) => onSourceChange?.(value)} onSortChange={(value) => onSortChange?.(value)} onProjectChange={(value) => onProjectChange?.(value)} onWorkerChange={(value) => onWorkerChange?.(value)} onSync={onJiraSync} />
      </details>

      {emailSourcesLoadError ? (
        <div className="form-error task-board-retry" role="alert">
          <span>Linked email details could not be refreshed. Task content is unchanged, but email source filters may be incomplete.</span>
          <button className="secondary-button" type="button" onClick={() => setEmailSourcesAttempt((attempt) => attempt + 1)}>Retry email details</button>
        </div>
      ) : null}

      {inApiary && apiaryRefreshState === "partial" ? (
        <div className="form-error task-board-retry" role="alert">
          <span>Shared Apiary work could not be fully refreshed. Last-known tasks and ownership remain visible, but claiming or routing may be out of date.</span>
          <button className="secondary-button" type="button" onClick={() => void refreshApiaryWork()}>Retry Apiary work</button>
        </div>
      ) : null}

      <section className="task-section" aria-labelledby="active-work-heading">
        <div className="section-heading">
          <div><p className="eyebrow">Queue</p><h3 id="active-work-heading">Active work</h3></div>
          <span className="count-badge">{openTasks.length === taskView.allOpenCount ? openTasks.length : `${openTasks.length}/${taskView.allOpenCount}`}</span>
        </div>
        {openTasks.length === 0 ? (
          <div className="empty-card"><BeeMascot className="empty-bee" expression="available" /><div><strong>{taskView.allOpenCount ? "No work matches this view" : "No work queued"}</strong><span>{taskView.allOpenCount ? "One or more board filters are hiding open work." : "Create a focused task when you are ready."}</span>{taskView.allOpenCount ? <button type="button" className="secondary-button" onClick={clearTaskView}>Show all open work</button> : null}</div></div>
        ) : (
          <div className="task-grid">
            {openTasks.map((task, index) => (
              <TaskCard
                key={task.id}
                task={task}
                jiraLink={jiraTaskLinks.find((link) => link.task_id === task.id)}
                emailSources={emailTaskSources.filter((source) => source.task_id === task.id)}
                operatorToken={operatorToken}
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

      {inApiary && apiaryTasks.length > 0 ? <section className="task-section apiary-task-section" aria-labelledby="apiary-work-heading">
        <div className="section-heading"><div><p className="eyebrow">Shared across Hives</p><h3 id="apiary-work-heading">Apiary work</h3><small>Managed here; summarized in Apiary.</small></div><span className="count-badge">{apiaryTasks.length}</span></div>
        {createError && !composeOpen ? <p className="form-error apiary-task-error" role="alert">{createError}</p> : null}
        <div className="apiary-task-board-list">
          {apiaryTasks.map((task) => {
            const home = task.home_hive_id ? apiaryMembers.find((member) => member.hive_id === task.home_hive_id)?.hive_name ?? "Assigned Hive" : "Available to claim";
            const localHiveId = hiveIdentity?.hive.id;
            const mine = Boolean(localHiveId && task.home_hive_id === localHiveId);
            const queued = apiaryOutbox.some((entry) => entry.state === "queued" && entry.command.task_id === task.id);
            const execution = apiaryExecutions.find((candidate) => candidate.apiary_task_id === task.id);
            return <article key={task.id} className="apiary-task-board-card">
              <span><small>{priorityLabels[task.priority]} · {stateLabels[task.state]}</small><strong>{task.title}</strong>{task.description ? <p>{task.description}</p> : null}</span>
              <span className="apiary-task-board-owner"><strong>{home}</strong><small>{mine ? "Owned by this Hive" : "Apiary task"}</small>
                {queued ? <small>Change queued for Keeper</small> : inApiary && apiaryContext.local_role === "member" && !task.home_hive_id ? <button className="secondary-button" type="button" disabled={actingApiaryTask === task.id} onClick={() => void claimSharedTask(task)}>{actingApiaryTask === task.id ? "Claiming…" : "Claim for this Hive"}</button> : null}
                {mine && execution ? <button className="secondary-button" type="button" onClick={() => onOpenTask?.(execution.local_task_id)}>Open local task</button> : mine ? <span className="apiary-task-worker-route"><select aria-label={`Worker for ${task.title}`} value={apiaryWorkerChoices[task.id] ?? ""} onChange={(event) => setApiaryWorkerChoices((current) => ({ ...current, [task.id]: event.target.value }))}><option value="">Choose a worker</option>{assignableWorkers.map((worker) => <option key={worker.id} value={worker.id}>{worker.name} · {worker.attention_state}</option>)}</select><button className="primary-action" type="button" disabled={actingApiaryTask === task.id || !apiaryWorkerChoices[task.id]} onClick={() => void sendSharedTaskToWorker(task)}>{actingApiaryTask === task.id ? "Sending…" : "Send to worker"}</button></span> : null}
              </span>
            </article>;
          })}
        </div>
      </section> : null}

      {/* Held above Completed work and outside its fold. Finished-but-unverified
          is the one closed state that still needs somebody, and filing it in the
          place people go to stop looking is what the operator reported. */}
      {unverifiedTasks.length > 0 && (
        <section className="unverified-tasks" aria-labelledby="unverified-work-heading">
          <div className="section-heading">
            <div>
              <p className="eyebrow">Finished, not shown to be live</p>
              <h3 id="unverified-work-heading">Waiting on evidence</h3>
              <small>No deployment recorded and no approved nothing-to-deploy claim.</small>
            </div>
            <span className="count-badge">{unverifiedTasks.length}</span>
          </div>
          <div className="task-grid compact">
            {unverifiedTasks.map((task) => (
              <div className="unverified-row" key={task.id}>
              <TaskCard
                task={task}
                jiraLink={jiraTaskLinks.find((link) => link.task_id === task.id)}
                emailSources={emailTaskSources.filter((source) => source.task_id === task.id)}
                operatorToken={operatorToken}
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
              {/* The panel reported a problem and offered nothing that could
                  resolve it, which is what the operator hit. It deliberately
                  does NOT offer "mark verified": this work is ten days old and
                  was done by workers against other repositories, so the only
                  thing the operator can honestly assert is that nobody can
                  establish where it went now. Operator ruling 2026-08-29,
                  decision 01a04d9f-da18-7d02-b47b-978f0a6b9a01. */}
              {unverifiableFor === task.id ? (
                <form
                  className="unverifiable-form"
                  onSubmit={(event) => {
                    event.preventDefault();
                    void markUnverifiable(task);
                  }}
                >
                  <label htmlFor={`unverifiable-${task.id}`}>Why can this no longer be checked?</label>
                  <textarea
                    id={`unverifiable-${task.id}`}
                    value={unverifiableNote}
                    onChange={(event) => setUnverifiableNote(event.target.value)}
                    placeholder="What you looked at, and what stopped you establishing where this went."
                    rows={2}
                  />
                  <div className="unverifiable-actions">
                    <button type="submit" className="secondary-button" disabled={busy || !unverifiableNote.trim()}>
                      Record as unverifiable
                    </button>
                    <button type="button" className="ghost-button" onClick={() => setUnverifiableFor(null)}>
                      Cancel
                    </button>
                  </div>
                  <small>This records that nobody could verify it. It never records that it shipped.</small>
                </form>
              ) : (
                <button
                  type="button"
                  className="ghost-button unverifiable-trigger"
                  onClick={() => { setUnverifiableFor(task.id); setUnverifiableNote(""); }}
                >
                  Cannot be verified now
                </button>
              )}
              </div>
            ))}
          </div>
        </section>
      )}

      {completedTasks.length > 0 && (
        <details
          ref={completedTasksPanel}
          className="completed-tasks"
          open={completedOpen}
          onToggle={(event) => setCompletedOpen(event.currentTarget.open)}
        >
          <summary><span>Completed work</span><small>{completedTasks.length}</small></summary>
          <div className="task-grid compact">
            {completedOpen && completedTasks.map((task) => (
              <TaskCard
                key={task.id}
                task={task}
                jiraLink={jiraTaskLinks.find((link) => link.task_id === task.id)}
                emailSources={emailTaskSources.filter((source) => source.task_id === task.id)}
                operatorToken={operatorToken}
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

      {removedTasks.length > 0 && <details className="removed-tasks">
        <summary><span>Removed local work</span><small>{removedTasks.length}</small></summary>
        <p>Recover a task removed from this Hive. Jira work stays under Jira and never appears here.</p>
        <div className="removed-task-list">
          {removedTasks.map((task) => <article key={task.id}>
            <span><strong>{task.title}</strong><small>Retired from {stateLabels[task.state]} · {new Date(task.updated_at * 1000).toLocaleString()}</small></span>
            <button
              type="button"
              className="secondary-button"
              disabled={busy || restoringTaskId === task.id}
              onClick={() => {
                setRestoreError(undefined);
                setRestoringTaskId(task.id);
                void onRestore(task)
                  .then(() => setRemovedTasks((current) => current.filter((candidate) => candidate.id !== task.id)))
                  .catch(() => setRestoreError(`“${task.title}” could not be restored. It remains available here.`))
                  .finally(() => setRestoringTaskId(undefined));
              }}
            >{restoringTaskId === task.id ? "Restoring…" : "Restore to board"}</button>
          </article>)}
        </div>
      </details>}
      {restoreError ? <p className="form-error" role="alert">{restoreError}</p> : null}
      {removedTasksLoadError ? (
        <div className="form-error task-board-retry" role="alert">
          <span>Removed local work could not be refreshed. No task was changed.</span>
          <button className="secondary-button" type="button" onClick={() => setRemovedTasksAttempt((attempt) => attempt + 1)}>Retry removed work</button>
        </div>
      ) : null}
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
