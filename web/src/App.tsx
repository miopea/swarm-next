import { lazy, Suspense, useEffect, useMemo, useState, type FormEvent, type KeyboardEvent as ReactKeyboardEvent } from "react";

import {
  assignTask,
  createTask,
  createWorker,
  fetchHive,
  fetchSessions,
  fetchTasks,
  fetchWorkers,
  startWorker,
  stopClaudeSession,
  stopWorker,
  transitionTask,
  updateTask,
  type ControlRoomEvent,
  type Health,
  type HiveIdentity,
  type SessionSummary,
  type Task,
  type TaskDraftInput,
  type TaskState,
  type TaskUpdateInput,
  type Worker,
} from "./api";
import BeeMascot from "./brand/BeeMascot";
import { applyColorTheme, initialColorTheme, type ColorTheme } from "./brand/theme";
import { ControlRoomLiveFeed, type LiveFeedState } from "./controlRoom/ControlRoomLiveFeed";
import SettingsWorkspace from "./settings/SettingsWorkspace";
import TaskBoard, { workerName } from "./tasks/TaskBoard";
import TerminalLoadBoundary from "./terminal/TerminalLoadBoundary";
import { terminalWorkspace } from "./terminal/TerminalWorkspace";
import WorkerRosterItem from "./workers/WorkerRosterItem";

const loadTerminalView = () => import("./terminal/TerminalView");
const TerminalView = lazy(loadTerminalView);
const OPERATOR_TOKEN_STORAGE_KEY = "swarm-next.operator-token.v1";
const SURFACE_STORAGE_KEY = "swarm-next.surface.v1";

type LoadState = { kind: "loading" } | { kind: "ready"; health: Health } | { kind: "unavailable" };
type Surface = "tasks" | "workers" | "settings";

export function App() {
  const [loadState, setLoadState] = useState<LoadState>({ kind: "loading" });
  const [tokenDraft, setTokenDraft] = useState("");
  const [operatorToken, setOperatorToken] = useState<string>();
  const [hiveIdentity, setHiveIdentity] = useState<HiveIdentity>();
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [workers, setWorkers] = useState<Worker[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string>();
  const [workerNameDraft, setWorkerNameDraft] = useState("");
  const [workspace, setWorkspace] = useState("");
  const [showWorkerForm, setShowWorkerForm] = useState(false);
  const [surface, setSurface] = useState<Surface>(readSavedSurface);
  const [operationError, setOperationError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [colorTheme, setColorTheme] = useState<ColorTheme>(initialColorTheme);
  const [liveFeedState, setLiveFeedState] = useState<LiveFeedState>("connecting");
  const [recentEvents, setRecentEvents] = useState<ControlRoomEvent[]>([]);

  useEffect(() => applyColorTheme(colorTheme), [colorTheme]);
  useEffect(() => saveSurface(surface), [surface]);

  useEffect(() => { void loadTerminalView().catch(() => undefined); }, []);

  useEffect(() => {
    const controller = new AbortController();
    void fetch("/health", { signal: controller.signal })
      .then((response) => {
        if (!response.ok) throw new Error(`Health returned ${response.status}`);
        return response.json() as Promise<Health>;
      })
      .then((health) => setLoadState({ kind: "ready", health }))
      .catch((error: unknown) => {
        if (!(error instanceof DOMException && error.name === "AbortError")) setLoadState({ kind: "unavailable" });
      });
    return () => controller.abort();
  }, []);

  useEffect(() => {
    const savedToken = readSavedOperatorToken();
    if (!savedToken) return;
    let cancelled = false;
    setBusy(true);
    void loadControlRoom(savedToken)
      .then(({ hive, sessions: nextSessions, workers: nextWorkers, tasks: nextTasks }) => {
        if (cancelled) return;
        terminalWorkspace.authenticate(savedToken);
        setOperatorToken(savedToken);
        setHiveIdentity(hive);
        setSessions(nextSessions);
        setWorkers(nextWorkers);
        setTasks(nextTasks);
        setActiveSessionId(preferredSessionId(nextWorkers, nextSessions));
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        clearSavedOperatorToken();
        terminalWorkspace.logout();
        setOperationError(error instanceof Error ? error.message : "Saved authentication is no longer valid");
      })
      .finally(() => {
        if (!cancelled) setBusy(false);
      });
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    if (!operatorToken) {
      setLiveFeedState("connecting");
      return;
    }
    let cancelled = false;
    const feed = new ControlRoomLiveFeed();
    feed.start(
      operatorToken,
      async (page) => {
        const controlRoom = await loadControlRoom(operatorToken);
        if (cancelled) return;
        setHiveIdentity(controlRoom.hive);
        setSessions(controlRoom.sessions);
        setWorkers(controlRoom.workers);
        setTasks(controlRoom.tasks);
        setRecentEvents((current) => page.reset_required
          ? page.events.slice(-16)
          : [...current, ...page.events].filter((event, index, events) =>
              events.findIndex((candidate) => candidate.sequence === event.sequence) === index,
            ).slice(-16));
        setActiveSessionId((current) =>
          current && controlRoom.sessions.some((session) => session.session_id === current)
            ? current
            : preferredSessionId(controlRoom.workers, controlRoom.sessions),
        );
      },
      setLiveFeedState,
    );
    return () => {
      cancelled = true;
      feed.stop();
    };
  }, [operatorToken]);

  async function authenticate(event: FormEvent) {
    event.preventDefault();
    if (!tokenDraft) return;
    await perform(async () => {
      const controlRoom = await loadControlRoom(tokenDraft);
      terminalWorkspace.authenticate(tokenDraft);
      setOperatorToken(tokenDraft);
      setHiveIdentity(controlRoom.hive);
      setSessions(controlRoom.sessions);
      setWorkers(controlRoom.workers);
      setTasks(controlRoom.tasks);
      setActiveSessionId((current) => current ?? preferredSessionId(controlRoom.workers, controlRoom.sessions));
      const tokenWasSaved = saveOperatorToken(tokenDraft);
      setTokenDraft("");
      if (!tokenWasSaved) throw new Error("Unlocked, but this browser blocked tab storage; refreshing will lock Swarm again.");
    });
  }

  async function refreshControlRoom() {
    if (!operatorToken) return;
    await perform(async () => {
      const controlRoom = await loadControlRoom(operatorToken);
      setHiveIdentity(controlRoom.hive);
      setSessions(controlRoom.sessions);
      setWorkers(controlRoom.workers);
      setTasks(controlRoom.tasks);
      setActiveSessionId((current) =>
        current && controlRoom.sessions.some((session) => session.session_id === current)
          ? current
          : preferredSessionId(controlRoom.workers, controlRoom.sessions),
      );
    });
  }

  async function startSession(event: FormEvent) {
    event.preventDefault();
    if (!operatorToken || !workerNameDraft.trim() || !workspace.trim()) return;
    await perform(async () => {
      const profile = await createWorker(operatorToken, {
        name: workerNameDraft,
        workspace,
      });
      const runningWorker = await startWorker(operatorToken, profile.id);
      const sessionId = requireActiveSession(runningWorker);
      const controlRoom = await loadControlRoom(operatorToken);
      setSessions(controlRoom.sessions);
      setWorkers(controlRoom.workers);
      setActiveSessionId(sessionId);
      setWorkerNameDraft("");
      setWorkspace("");
      setShowWorkerForm(false);
      setSurface("workers");
    });
  }

  async function startWorkerForTask(task: Task) {
    if (!operatorToken) return;
    await perform(async () => {
      const profile = await createWorker(operatorToken, {
        name: availableTaskWorkerName(task.title, workers),
        workspace: task.workspace,
      });
      const runningWorker = await startWorker(operatorToken, profile.id);
      const sessionId = requireActiveSession(runningWorker);
      await assignTask(operatorToken, task.id, sessionId);
      await transitionTask(operatorToken, task.id, "active");
      const controlRoom = await loadControlRoom(operatorToken);
      setHiveIdentity(controlRoom.hive);
      setSessions(controlRoom.sessions);
      setWorkers(controlRoom.workers);
      setTasks(controlRoom.tasks);
      setActiveSessionId(sessionId);
      setSurface("workers");
    });
  }

  async function stopSession(sessionId: string) {
    if (!operatorToken) return;
    await perform(async () => {
      const profile = workers.find((worker) => worker.active_session_id === sessionId);
      if (profile) await stopWorker(operatorToken, profile.id);
      else await stopClaudeSession(operatorToken, sessionId);
      terminalWorkspace.closeSession(sessionId);
      const controlRoom = await loadControlRoom(operatorToken);
      setHiveIdentity(controlRoom.hive);
      setSessions(controlRoom.sessions);
      setWorkers(controlRoom.workers);
      setTasks(controlRoom.tasks);
      setActiveSessionId((current) => current === sessionId ? preferredSessionId(controlRoom.workers, controlRoom.sessions) : current);
    });
  }

  async function startExistingWorker(profile: Worker) {
    if (!operatorToken) return;
    await perform(async () => {
      const runningWorker = await startWorker(operatorToken, profile.id);
      const sessionId = requireActiveSession(runningWorker);
      const controlRoom = await loadControlRoom(operatorToken);
      setHiveIdentity(controlRoom.hive);
      setSessions(controlRoom.sessions);
      setWorkers(controlRoom.workers);
      setTasks(controlRoom.tasks);
      setActiveSessionId(sessionId);
      setSurface("workers");
    });
  }

  async function addTask(input: TaskDraftInput) {
    if (!operatorToken) return;
    await perform(async () => {
      const task = await createTask(operatorToken, input);
      setTasks((current) => [task, ...current]);
    });
  }

  async function editTask(task: Task, input: TaskUpdateInput) {
    if (!operatorToken) return;
    setBusy(true);
    setOperationError(undefined);
    try {
      replaceTask(await updateTask(operatorToken, task.id, input));
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : "The task could not be updated");
      throw error;
    } finally {
      setBusy(false);
    }
  }

  async function moveTask(task: Task, state: TaskState) {
    if (!operatorToken) return;
    await perform(async () => {
      const updated = await transitionTask(operatorToken, task.id, state);
      replaceTask(updated);
    });
  }

  async function setTaskWorker(task: Task, sessionId: string) {
    if (!operatorToken) return;
    await perform(async () => replaceTask(await assignTask(operatorToken, task.id, sessionId)));
  }

  function replaceTask(updated: Task) {
    setTasks((current) => current.map((task) => task.id === updated.id ? updated : task));
  }

  async function perform(action: () => Promise<void>) {
    setBusy(true);
    setOperationError(undefined);
    try {
      await action();
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : "The operation could not be completed");
    } finally {
      setBusy(false);
    }
  }

  function handleShortcut(event: ReactKeyboardEvent<HTMLElement>) {
    if (!operatorToken || !event.altKey || event.ctrlKey || event.metaKey || isTypingTarget(event.target)) return;
    if (event.key === "1" || event.key === "2" || event.key === "3") {
      event.preventDefault();
      setSurface(event.key === "1" ? "tasks" : event.key === "2" ? "workers" : "settings");
      return;
    }
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    const running = workers.filter((worker) => worker.running && worker.active_session_id);
    if (running.length === 0) return;
    event.preventDefault();
    const currentIndex = running.findIndex((worker) => worker.active_session_id === activeSessionId);
    const direction = event.key === "ArrowDown" ? 1 : -1;
    const nextIndex = currentIndex < 0 ? 0 : (currentIndex + direction + running.length) % running.length;
    const nextSessionId = running[nextIndex]?.active_session_id;
    if (nextSessionId) setActiveSessionId(nextSessionId);
    setSurface("workers");
  }

  function logout() {
    clearSavedOperatorToken();
    terminalWorkspace.logout();
    setOperatorToken(undefined);
    setHiveIdentity(undefined);
    setSessions([]);
    setWorkers([]);
    setTasks([]);
    setActiveSessionId(undefined);
    setOperationError(undefined);
  }

  const activeSession = sessions.find((session) => session.session_id === activeSessionId);
  const activeWorker = workers.find((worker) => worker.active_session_id === activeSessionId);
  const openTaskCount = tasks.filter((task) => task.state !== "completed").length;
  const workerNames = useMemo(
    () => new Map(
      workers
        .filter((worker) => worker.active_session_id)
        .map((worker) => [worker.active_session_id as string, worker.name]),
    ),
    [workers],
  );
  const orphanSessions = useMemo(
    () => sessions.filter((session) => !workers.some((worker) => worker.active_session_id === session.session_id)),
    [sessions, workers],
  );
  const tasksBySession = useMemo(
    () => new Map(tasks.filter((task) => task.assigned_session_id).map((task) => [task.assigned_session_id, task])),
    [tasks],
  );
  const activeTask = activeSession ? tasksBySession.get(activeSession.session_id) : undefined;

  return (
    <main className="app-shell" onKeyDown={handleShortcut}>
      <aside className={`control-rail surface-${surface}`} aria-label="Swarm navigation">
        <div className="brand-lockup">
          <div className="brand-mark"><BeeMascot expression="available" /></div>
          <div><p className="eyebrow">Swarm Next</p><h1>Control room</h1></div>
        </div>

        {operatorToken ? (
          <>
            <nav className="surface-nav" aria-label="Primary">
              <button className={surface === "tasks" ? "selected" : ""} aria-current={surface === "tasks" ? "page" : undefined} onClick={() => setSurface("tasks")}>
                <span><TaskIcon /> Tasks</span><small>{openTaskCount}</small>
              </button>
              <button className={surface === "workers" ? "selected" : ""} aria-current={surface === "workers" ? "page" : undefined} onClick={() => setSurface("workers")}>
                <span><TerminalIcon /> Workers</span><small>{workers.filter((worker) => worker.running).length + orphanSessions.filter((session) => session.running).length}</small>
              </button>
              <button className={surface === "settings" ? "selected" : ""} aria-current={surface === "settings" ? "page" : undefined} onClick={() => setSurface("settings")}>
                <span><SettingsIcon /> Settings</span><small aria-hidden="true">3</small>
              </button>
            </nav>

            {surface !== "settings" && <div className="rail-context">
              <div className="rail-heading"><span>{surface === "tasks" ? "Open tasks" : "Live sessions"}</span></div>
              {surface === "tasks" ? (
                tasks.filter((task) => task.state !== "completed").length === 0 ? <p className="empty-rail">Nothing queued yet.</p> :
                  <div className="mini-task-list">{tasks.filter((task) => task.state !== "completed").slice(0, 8).map((task) => <div key={task.id}><span className={`state-dot state-${task.state}`} /><span>{task.title}</span></div>)}</div>
              ) : workers.length === 0 && orphanSessions.length === 0 ? (
                <p className="empty-rail">No workers configured.</p>
              ) : (
                <div className="worker-list">
                  {workers.map((worker) => {
                    const sessionId = worker.active_session_id;
                    const task = sessionId ? tasksBySession.get(sessionId) : undefined;
                    return (
                      <WorkerRosterItem
                        key={worker.id}
                        worker={worker}
                        selected={sessionId === activeSessionId}
                        detail={worker.runtime_error ?? task?.title ?? (worker.role === "queen" ? "Always-active command terminal" : worker.running ? "Unassigned session" : "Stopped · click to start")}
                        busy={busy}
                        onOpen={() => sessionId && setActiveSessionId(sessionId)}
                        onStart={() => void startExistingWorker(worker)}
                        onStop={() => sessionId && void stopSession(sessionId)}
                      />
                    );
                  })}
                  {orphanSessions.map((session) => {
                    const task = tasksBySession.get(session.session_id);
                    return (
                      <button className="worker-button" aria-current={session.session_id === activeSessionId ? "page" : undefined} key={session.session_id} onClick={() => setActiveSessionId(session.session_id)}>
                        <span className="worker-avatar"><BeeMascot expression={session.running ? "focused" : "sleeping"} /></span>
                        <span className="worker-copy">
                          <strong>{workerName(session.session_id)}</strong>
                          <small>{task?.title ?? "Pre-roster session"}</small>
                        </span>
                        <span className={`presence ${session.running ? "online" : "offline"}`} title={session.running ? "Running" : "Exited"} />
                      </button>
                    );
                  })}
                </div>
              )}
            </div>}

            {surface === "workers" && (
              <div className="start-worker-disclosure">
                <button
                  type="button"
                  aria-expanded={showWorkerForm}
                  aria-controls="start-worker-form"
                  onClick={() => setShowWorkerForm((current) => !current)}
                >
                  {showWorkerForm ? "Hide worker form" : "Add worker"}
                </button>
                <form id="start-worker-form" className={showWorkerForm ? "start-worker mobile-expanded" : "start-worker"} onSubmit={(event) => void startSession(event)}>
                  <label htmlFor="worker-name">Add a named worker</label>
                  <input id="worker-name" value={workerNameDraft} onChange={(event) => setWorkerNameDraft(event.target.value)} placeholder="Worker name" maxLength={80} />
                  <label className="sr-only" htmlFor="workspace">Worker workspace</label>
                  <input id="workspace" value={workspace} onChange={(event) => setWorkspace(event.target.value)} placeholder="/workspace/path" />
                  <button disabled={busy || !workerNameDraft.trim() || !workspace.trim()}>Create and start</button>
                </form>
              </div>
            )}
          </>
        ) : <p className="empty-rail">Unlock this runtime to access tasks and workers.</p>}

        <div className="rail-footer"><RuntimeStatus state={loadState} /></div>
      </aside>

      <section className="workspace">
        <header className="workspace-header">
          <div>
            <p className="eyebrow">{surface === "tasks" ? "Plan and dispatch" : surface === "settings" ? "Preferences and diagnostics" : activeTask?.title ?? "Persistent terminal"}</p>
            <h2>{surface === "tasks" ? "Task board" : surface === "settings" ? "Settings" : activeSession ? activeWorker?.name ?? workerName(activeSession.session_id) : "Worker terminal"}</h2>
          </div>
          <div className="header-actions">
            {busy && <span className="saving-state">Saving…</span>}
            <button className="icon-button" aria-label={`Switch to ${colorTheme === "light" ? "dark" : "light"} theme`} onClick={() => setColorTheme((current) => current === "light" ? "dark" : "light")}><ThemeIcon theme={colorTheme} /></button>
            {operatorToken && <button className="icon-button" aria-label="Refresh control room" onClick={() => void refreshControlRoom()} disabled={busy}><RefreshIcon /></button>}
            {operatorToken && <button className="secondary-button" onClick={logout}>Lock</button>}
          </div>
        </header>
        {operationError && <div className="operation-error" role="alert">{operationError}</div>}
        {!operatorToken ? (
          <form className="unlock-panel" onSubmit={(event) => void authenticate(event)}>
            <div className="unlock-symbol"><BeeMascot expression="available" /></div>
            <p className="eyebrow">Private local runtime</p>
            <h3>Welcome back</h3>
            <p>Unlock this control room. Your credential stays in this browser tab and terminal access uses one-time grants.</p>
            <label htmlFor="operator-token">Operator token</label>
            <input id="operator-token" type="password" autoComplete="off" value={tokenDraft} onChange={(event) => setTokenDraft(event.target.value)} />
            <button disabled={busy || !tokenDraft}>Unlock Swarm</button>
          </form>
        ) : surface === "tasks" ? (
          <TaskBoard tasks={tasks} sessions={sessions} workerNames={workerNames} busy={busy} onCreate={addTask} onUpdate={editTask} onTransition={moveTask} onAssign={setTaskWorker} onStartWorker={startWorkerForTask} />
        ) : surface === "settings" ? (
          <SettingsWorkspace
            colorTheme={colorTheme}
            hiveIdentity={hiveIdentity}
            liveFeedState={liveFeedState}
            health={loadState.kind === "ready" ? loadState.health : undefined}
            operatorToken={operatorToken}
            recentEvents={recentEvents}
            sessions={sessions}
            workers={workers}
            onThemeChange={setColorTheme}
          />
        ) : activeSession ? (
          <TerminalLoadBoundary key={`${operatorToken}:${activeSession.session_id}`}>
            <Suspense fallback={<div className="terminal-empty">Preparing terminal…</div>}>
              <TerminalView operatorToken={operatorToken} session={activeSession} onStop={() => void stopSession(activeSession.session_id)} busy={busy} canStop={activeWorker?.role !== "queen"} />
            </Suspense>
          </TerminalLoadBoundary>
        ) : (
          <div className="terminal-empty"><BeeMascot className="empty-bee" expression="sleeping" /><p className="eyebrow">No active session</p><h3>Start with a task or workspace</h3><p>Launch Claude from a ready task to preserve its assignment, or start an unassigned worker from the sidebar.</p></div>
        )}
      </section>
    </main>
  );
}

async function loadControlRoom(operatorToken: string) {
  const [hive, sessions, workers, tasks] = await Promise.all([
    fetchHive(operatorToken),
    fetchSessions(operatorToken),
    fetchWorkers(operatorToken),
    fetchTasks(operatorToken),
  ]);
  return { hive, sessions, workers, tasks };
}

function preferredSessionId(workers: Worker[], sessions: SessionSummary[]): string | undefined {
  return workers.find((worker) => worker.role === "queen" && worker.running)?.active_session_id
    ?? workers.find((worker) => worker.running)?.active_session_id
    ?? sessions.find((session) => session.running)?.session_id;
}

function requireActiveSession(worker: Worker): string {
  if (!worker.active_session_id) throw new Error(`${worker.name} did not receive a terminal session`);
  return worker.active_session_id;
}

function availableTaskWorkerName(title: string, workers: Worker[]): string {
  const base = title.trim().slice(0, 72) || "Task worker";
  const names = new Set(workers.map((worker) => worker.name.toLocaleLowerCase()));
  if (!names.has(base.toLocaleLowerCase())) return base;
  let suffix = 2;
  while (names.has(`${base} ${suffix}`.toLocaleLowerCase())) suffix += 1;
  return `${base} ${suffix}`.slice(0, 80);
}

function readSavedOperatorToken(): string | undefined { try { return window.sessionStorage.getItem(OPERATOR_TOKEN_STORAGE_KEY) ?? undefined; } catch { return undefined; } }
function saveOperatorToken(operatorToken: string): boolean { try { window.sessionStorage.setItem(OPERATOR_TOKEN_STORAGE_KEY, operatorToken); return true; } catch { return false; } }
function clearSavedOperatorToken() { try { window.sessionStorage.removeItem(OPERATOR_TOKEN_STORAGE_KEY); } catch { /* Locking memory is sufficient when browser storage is unavailable. */ } }

function RuntimeStatus({ state }: { state: LoadState }) {
  if (state.kind === "ready") return <span className="runtime-status"><span className="presence online" /> Runtime {state.health.version}</span>;
  if (state.kind === "unavailable") return <span className="runtime-status error"><span className="presence offline" /> Runtime unavailable</span>;
  return <span className="runtime-status"><span className="presence" /> Connecting…</span>;
}

function TaskIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01" /></svg>; }
function TerminalIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m5 7 4 4-4 4M11 17h8" /></svg>; }
function readSavedSurface(): Surface { try { const saved = window.sessionStorage.getItem(SURFACE_STORAGE_KEY); return saved === "workers" || saved === "settings" ? saved : "tasks"; } catch { return "tasks"; } }
function saveSurface(surface: Surface) { try { window.sessionStorage.setItem(SURFACE_STORAGE_KEY, surface); } catch { /* Surface persistence is a non-critical convenience. */ } }
function RefreshIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 6v5h-5M4 18v-5h5M6.1 9a7 7 0 0 1 11.4-2.4L20 9M4 15l2.5 2.4A7 7 0 0 0 17.9 15" /></svg>; }
function SettingsIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6v.2h-4V21a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H2.8v-4H3a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-1.6v-.2h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.2v4H21a1.7 1.7 0 0 0-1.6 1Z"/></svg>; }
function ThemeIcon({ theme }: { theme: ColorTheme }) { return theme === "light" ? <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3v2m0 14v2M3 12h2m14 0h2M5.6 5.6 7 7m10 10 1.4 1.4M18.4 5.6 17 7M7 17l-1.4 1.4"/><circle cx="12" cy="12" r="4"/></svg> : <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 15.5A8 8 0 0 1 8.5 4 8.5 8.5 0 1 0 20 15.5Z"/></svg>; }

function isTypingTarget(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) || Boolean(target.closest("[role='menu']")));
}
