import { lazy, Suspense, useEffect, useState, type FormEvent } from "react";

import { terminalWorkspace } from "./terminal/TerminalWorkspace";

const TerminalView = lazy(() => import("./terminal/TerminalView"));

type Health = { status: "ok"; version: string };
type LoadState = { kind: "loading" } | { kind: "ready"; health: Health } | { kind: "unavailable" };
type SessionSummary = { session_id: string; running: boolean };
type SessionsResponse = { type: "sessions"; sessions: SessionSummary[] };
type SessionStartedResponse = { type: "session_started"; session_id: string };

export function App() {
  const [loadState, setLoadState] = useState<LoadState>({ kind: "loading" });
  const [tokenDraft, setTokenDraft] = useState("");
  const [operatorToken, setOperatorToken] = useState<string>();
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string>();
  const [workspace, setWorkspace] = useState("");
  const [operationError, setOperationError] = useState<string>();
  const [busy, setBusy] = useState(false);

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

  async function authenticate(event: FormEvent) {
    event.preventDefault();
    if (!tokenDraft) return;
    setBusy(true);
    setOperationError(undefined);
    try {
      const nextSessions = await fetchSessions(tokenDraft);
      terminalWorkspace.authenticate(tokenDraft);
      setOperatorToken(tokenDraft);
      setTokenDraft("");
      setSessions(nextSessions);
      setActiveSessionId((current) => current ?? nextSessions[0]?.session_id);
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : "Authentication failed");
    } finally {
      setBusy(false);
    }
  }

  async function refreshSessions() {
    if (!operatorToken) return;
    setBusy(true);
    setOperationError(undefined);
    try {
      const nextSessions = await fetchSessions(operatorToken);
      setSessions(nextSessions);
      setActiveSessionId((current) =>
        current && nextSessions.some((session) => session.session_id === current)
          ? current
          : nextSessions[0]?.session_id,
      );
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : "Could not refresh workers");
    } finally {
      setBusy(false);
    }
  }

  async function startSession(event: FormEvent) {
    event.preventDefault();
    if (!operatorToken || !workspace) return;
    setBusy(true);
    setOperationError(undefined);
    try {
      const response = await authenticatedFetch(operatorToken, "/api/v1/terminal/sessions", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ workspace, rows: 24, columns: 80 }),
      });
      const started = (await response.json()) as SessionStartedResponse;
      const nextSessions = await fetchSessions(operatorToken);
      setSessions(nextSessions);
      setActiveSessionId(started.session_id);
      setWorkspace("");
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : "Could not start worker");
    } finally {
      setBusy(false);
    }
  }

  async function stopSession(sessionId: string) {
    if (!operatorToken) return;
    setBusy(true);
    setOperationError(undefined);
    try {
      await authenticatedFetch(
        operatorToken,
        `/api/v1/terminal/sessions/${encodeURIComponent(sessionId)}`,
        { method: "DELETE" },
      );
      terminalWorkspace.closeSession(sessionId);
      await refreshSessions();
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : "Could not stop worker");
      setBusy(false);
    }
  }

  function logout() {
    terminalWorkspace.logout();
    setOperatorToken(undefined);
    setSessions([]);
    setActiveSessionId(undefined);
    setOperationError(undefined);
  }

  const activeSession = sessions.find((session) => session.session_id === activeSessionId);

  return (
    <main className="app-shell">
      <aside className="worker-rail" aria-label="Workers">
        <div className="brand-mark" aria-hidden="true">S</div>
        <div><p className="eyebrow">Swarm Next</p><h1>Workers</h1></div>
        {!operatorToken ? (
          <p className="empty-rail">Unlock the local runtime to view workers.</p>
        ) : sessions.length === 0 ? (
          <p className="empty-rail">No workers running</p>
        ) : (
          <div className="worker-list">
            {sessions.map((session, index) => (
              <button
                className="worker-button"
                aria-current={session.session_id === activeSessionId ? "page" : undefined}
                key={session.session_id}
                onClick={() => setActiveSessionId(session.session_id)}
              >
                <span>Worker {index + 1}</span>
                <small>{session.running ? "Running" : "Exited"}</small>
              </button>
            ))}
          </div>
        )}
        {operatorToken && (
          <form className="start-worker" onSubmit={(event) => void startSession(event)}>
            <label htmlFor="workspace">Workspace path</label>
            <input
              id="workspace"
              value={workspace}
              onChange={(event) => setWorkspace(event.target.value)}
              placeholder="/absolute/path/to/workspace"
            />
            <button disabled={busy || !workspace}>Start Claude</button>
          </form>
        )}
      </aside>
      <section className="workspace">
        <header className="workspace-header">
          <div><p className="eyebrow">Persistent terminal</p><h2>Durable worker sessions</h2></div>
          <div className="header-actions">
            <RuntimeStatus state={loadState} />
            {operatorToken && <button className="secondary-button" onClick={() => void refreshSessions()} disabled={busy}>Refresh</button>}
            {operatorToken && <button className="secondary-button" onClick={logout}>Lock</button>}
          </div>
        </header>
        {operationError && <div className="operation-error" role="alert">{operationError}</div>}
        {!operatorToken ? (
          <form className="unlock-panel" onSubmit={(event) => void authenticate(event)}>
            <span className="terminal-prompt" aria-hidden="true">›_</span>
            <h3>Unlock local Swarm</h3>
            <p>The operator token stays in this browser tab and is exchanged for one-time terminal grants.</p>
            <label htmlFor="operator-token">Operator token</label>
            <input
              id="operator-token"
              type="password"
              autoComplete="off"
              value={tokenDraft}
              onChange={(event) => setTokenDraft(event.target.value)}
            />
            <button disabled={busy || !tokenDraft}>Unlock</button>
          </form>
        ) : activeSession ? (
          <Suspense fallback={<div className="terminal-empty">Loading terminal renderer…</div>}>
            <TerminalView
              key={`${operatorToken}:${activeSession.session_id}`}
              operatorToken={operatorToken}
              session={activeSession}
              onStop={() => void stopSession(activeSession.session_id)}
              busy={busy}
            />
          </Suspense>
        ) : (
          <div className="terminal-empty">
            <span className="terminal-prompt" aria-hidden="true">›_</span>
            <h3>No terminal attached</h3>
            <p>Start a Claude worker in an allowed workspace to open a persistent terminal.</p>
          </div>
        )}
      </section>
    </main>
  );
}

async function fetchSessions(operatorToken: string): Promise<SessionSummary[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/terminal/sessions");
  const payload = (await response.json()) as SessionsResponse;
  return payload.sessions;
}

async function authenticatedFetch(operatorToken: string, url: string, init: RequestInit = {}): Promise<Response> {
  const headers = new Headers(init.headers);
  headers.set("Authorization", `Bearer ${operatorToken}`);
  const response = await fetch(url, { ...init, headers, cache: "no-store" });
  if (!response.ok) throw new Error(`Runtime request returned ${response.status}`);
  return response;
}

function RuntimeStatus({ state }: { state: LoadState }) {
  if (state.kind === "ready") return <span className="status status-ready">Runtime {state.health.version}</span>;
  if (state.kind === "unavailable") return <span className="status status-error">Runtime unavailable</span>;
  return <span className="status">Connecting…</span>;
}
