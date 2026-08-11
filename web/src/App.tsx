import { lazy, Suspense, useEffect, useMemo, useState, type FormEvent } from "react";

import {
  assignTask,
  createTask,
  fetchSessions,
  fetchTasks,
  startClaudeSession,
  stopClaudeSession,
  transitionTask,
  type Health,
  type SessionSummary,
  type Task,
  type TaskState,
} from "./api";
import BeeMascot from "./brand/BeeMascot";
import { applyColorTheme, initialColorTheme, type ColorTheme } from "./brand/theme";
import TaskBoard, { workerName } from "./tasks/TaskBoard";
import TerminalLoadBoundary from "./terminal/TerminalLoadBoundary";
import { terminalWorkspace } from "./terminal/TerminalWorkspace";

const loadTerminalView = () => import("./terminal/TerminalView");
const TerminalView = lazy(loadTerminalView);
const OPERATOR_TOKEN_STORAGE_KEY = "swarm-next.operator-token.v1";

type LoadState = { kind: "loading" } | { kind: "ready"; health: Health } | { kind: "unavailable" };
type Surface = "tasks" | "workers";

export function App() {
  const [loadState, setLoadState] = useState<LoadState>({ kind: "loading" });
  const [tokenDraft, setTokenDraft] = useState("");
  const [operatorToken, setOperatorToken] = useState<string>();
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string>();
  const [workspace, setWorkspace] = useState("");
  const [surface, setSurface] = useState<Surface>("tasks");
  const [operationError, setOperationError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [colorTheme, setColorTheme] = useState<ColorTheme>(initialColorTheme);

  useEffect(() => applyColorTheme(colorTheme), [colorTheme]);

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
      .then(({ sessions: nextSessions, tasks: nextTasks }) => {
        if (cancelled) return;
        terminalWorkspace.authenticate(savedToken);
        setOperatorToken(savedToken);
        setSessions(nextSessions);
        setTasks(nextTasks);
        setActiveSessionId(nextSessions[0]?.session_id);
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

  async function authenticate(event: FormEvent) {
    event.preventDefault();
    if (!tokenDraft) return;
    await perform(async () => {
      const controlRoom = await loadControlRoom(tokenDraft);
      terminalWorkspace.authenticate(tokenDraft);
      setOperatorToken(tokenDraft);
      setSessions(controlRoom.sessions);
      setTasks(controlRoom.tasks);
      setActiveSessionId((current) => current ?? controlRoom.sessions[0]?.session_id);
      const tokenWasSaved = saveOperatorToken(tokenDraft);
      setTokenDraft("");
      if (!tokenWasSaved) throw new Error("Unlocked, but this browser blocked tab storage; refreshing will lock Swarm again.");
    });
  }

  async function refreshControlRoom() {
    if (!operatorToken) return;
    await perform(async () => {
      const controlRoom = await loadControlRoom(operatorToken);
      setSessions(controlRoom.sessions);
      setTasks(controlRoom.tasks);
      setActiveSessionId((current) =>
        current && controlRoom.sessions.some((session) => session.session_id === current)
          ? current
          : controlRoom.sessions[0]?.session_id,
      );
    });
  }

  async function startSession(event: FormEvent) {
    event.preventDefault();
    if (!operatorToken || !workspace.trim()) return;
    await perform(async () => {
      const sessionId = await startClaudeSession(operatorToken, workspace);
      const nextSessions = await fetchSessions(operatorToken);
      setSessions(nextSessions);
      setActiveSessionId(sessionId);
      setWorkspace("");
      setSurface("workers");
    });
  }

  async function startWorkerForTask(task: Task) {
    if (!operatorToken) return;
    await perform(async () => {
      const sessionId = await startClaudeSession(operatorToken, task.workspace);
      await assignTask(operatorToken, task.id, sessionId);
      await transitionTask(operatorToken, task.id, "active");
      const controlRoom = await loadControlRoom(operatorToken);
      setSessions(controlRoom.sessions);
      setTasks(controlRoom.tasks);
      setActiveSessionId(sessionId);
      setSurface("workers");
    });
  }

  async function stopSession(sessionId: string) {
    if (!operatorToken) return;
    await perform(async () => {
      await stopClaudeSession(operatorToken, sessionId);
      terminalWorkspace.closeSession(sessionId);
      const controlRoom = await loadControlRoom(operatorToken);
      setSessions(controlRoom.sessions);
      setTasks(controlRoom.tasks);
      setActiveSessionId((current) => current === sessionId ? controlRoom.sessions[0]?.session_id : current);
    });
  }

  async function addTask(title: string, taskWorkspace: string) {
    if (!operatorToken) return;
    await perform(async () => {
      const task = await createTask(operatorToken, { title, workspace: taskWorkspace });
      setTasks((current) => [task, ...current]);
    });
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

  function logout() {
    clearSavedOperatorToken();
    terminalWorkspace.logout();
    setOperatorToken(undefined);
    setSessions([]);
    setTasks([]);
    setActiveSessionId(undefined);
    setOperationError(undefined);
  }

  const activeSession = sessions.find((session) => session.session_id === activeSessionId);
  const openTaskCount = tasks.filter((task) => task.state !== "completed").length;
  const tasksBySession = useMemo(
    () => new Map(tasks.filter((task) => task.assigned_session_id).map((task) => [task.assigned_session_id, task])),
    [tasks],
  );
  const activeTask = activeSession ? tasksBySession.get(activeSession.session_id) : undefined;

  return (
    <main className="app-shell">
      <aside className="control-rail" aria-label="Swarm navigation">
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
                <span><TerminalIcon /> Workers</span><small>{sessions.filter((session) => session.running).length}</small>
              </button>
            </nav>

            <div className="rail-context">
              <div className="rail-heading"><span>{surface === "tasks" ? "Open tasks" : "Live sessions"}</span></div>
              {surface === "tasks" ? (
                tasks.filter((task) => task.state !== "completed").length === 0 ? <p className="empty-rail">Nothing queued yet.</p> :
                  <div className="mini-task-list">{tasks.filter((task) => task.state !== "completed").slice(0, 8).map((task) => <div key={task.id}><span className={`state-dot state-${task.state}`} /><span>{task.title}</span></div>)}</div>
              ) : sessions.length === 0 ? (
                <p className="empty-rail">No workers running.</p>
              ) : (
                <div className="worker-list">
                  {sessions.map((session) => {
                    const task = tasksBySession.get(session.session_id);
                    return (
                      <button className="worker-button" aria-current={session.session_id === activeSessionId ? "page" : undefined} key={session.session_id} onClick={() => setActiveSessionId(session.session_id)}>
                        <span className="worker-avatar"><BeeMascot expression={session.running ? "focused" : "sleeping"} /></span>
                        <span className="worker-copy"><strong>{workerName(session.session_id)}</strong><small>{task?.title ?? "Unassigned session"}</small></span>
                        <span className={`presence ${session.running ? "online" : "offline"}`} title={session.running ? "Running" : "Exited"} />
                      </button>
                    );
                  })}
                </div>
              )}
            </div>

            {surface === "workers" && (
              <form className="start-worker" onSubmit={(event) => void startSession(event)}>
                <label htmlFor="workspace">Start an unassigned worker</label>
                <input id="workspace" value={workspace} onChange={(event) => setWorkspace(event.target.value)} placeholder="/workspace/path" />
                <button disabled={busy || !workspace.trim()}>Start Claude</button>
              </form>
            )}
          </>
        ) : <p className="empty-rail">Unlock this runtime to access tasks and workers.</p>}

        <div className="rail-footer"><RuntimeStatus state={loadState} /></div>
      </aside>

      <section className="workspace">
        <header className="workspace-header">
          <div>
            <p className="eyebrow">{surface === "tasks" ? "Plan and dispatch" : activeTask?.title ?? "Persistent terminal"}</p>
            <h2>{surface === "tasks" ? "Task board" : activeSession ? workerName(activeSession.session_id) : "Worker terminal"}</h2>
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
          <TaskBoard tasks={tasks} sessions={sessions} busy={busy} onCreate={addTask} onTransition={moveTask} onAssign={setTaskWorker} onStartWorker={startWorkerForTask} />
        ) : activeSession ? (
          <TerminalLoadBoundary key={`${operatorToken}:${activeSession.session_id}`}>
            <Suspense fallback={<div className="terminal-empty">Preparing terminal…</div>}>
              <TerminalView operatorToken={operatorToken} session={activeSession} onStop={() => void stopSession(activeSession.session_id)} busy={busy} />
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
  const [sessions, tasks] = await Promise.all([fetchSessions(operatorToken), fetchTasks(operatorToken)]);
  return { sessions, tasks };
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
function RefreshIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 6v5h-5M4 18v-5h5M6.1 9a7 7 0 0 1 11.4-2.4L20 9M4 15l2.5 2.4A7 7 0 0 0 17.9 15" /></svg>; }
function ThemeIcon({ theme }: { theme: ColorTheme }) { return theme === "light" ? <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3v2m0 14v2M3 12h2m14 0h2M5.6 5.6 7 7m10 10 1.4 1.4M18.4 5.6 17 7M7 17l-1.4 1.4"/><circle cx="12" cy="12" r="4"/></svg> : <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 15.5A8 8 0 0 1 8.5 4 8.5 8.5 0 1 0 20 15.5Z"/></svg>; }
