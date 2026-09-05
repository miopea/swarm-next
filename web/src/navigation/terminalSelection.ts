import type { SessionSummary, Worker } from "../api";

export type TerminalSelection = { workerId?: string; sessionId?: string };
const KEY = "swarm-next.terminal-selection.v2";
const LEGACY_KEY = "swarm-next.active-session.v1";

export function selectTerminal(sessionId: string | undefined, workers: Worker[]): TerminalSelection {
  if (!sessionId) return {};
  const worker = workers.find((item) => item.active_session_id === sessionId);
  return worker ? { workerId: worker.id, sessionId } : { sessionId };
}

/** A temporarily absent process cannot select a different worker or start one. */
export function reconcileTerminalSelection(selection: TerminalSelection, workers: Worker[], sessions: SessionSummary[]): TerminalSelection {
  const next = reconcile(selection, workers, sessions);
  return next.workerId === selection.workerId && next.sessionId === selection.sessionId ? selection : next;
}

function reconcile(selection: TerminalSelection, workers: Worker[], sessions: SessionSummary[]): TerminalSelection {
  const exists = (id: string | null | undefined) => typeof id === "string" && sessions.some((item) => item.session_id === id && item.running);
  if (selection.workerId) {
    const worker = workers.find((item) => item.id === selection.workerId);
    if (worker) return { workerId: worker.id, sessionId: worker.running && exists(worker.active_session_id) ? worker.active_session_id ?? undefined : undefined };
  } else if (exists(selection.sessionId)) {
    return selectTerminal(selection.sessionId, workers);
  }
  const preferred = workers.find((item) => item.role === "queen" && item.running && exists(item.active_session_id))
    ?? workers.find((item) => item.running && exists(item.active_session_id));
  return selectTerminal(preferred?.active_session_id ?? sessions.find((item) => item.running)?.session_id, workers);
}

export function restoreTerminalSelection(workers: Worker[], sessions: SessionSummary[]): TerminalSelection {
  let selection: TerminalSelection = {};
  try {
    const raw = window.localStorage.getItem(KEY);
    const parsed = raw && raw.length < 1024 ? JSON.parse(raw) : undefined;
    if (parsed && typeof parsed === "object") {
      selection = { workerId: boundedId(parsed.workerId), sessionId: boundedId(parsed.sessionId) };
    } else {
      // UI selection owns migration from the previous supported frontend.
      // Remove this read when pre-v2 selection clients are no longer supported.
      selection = { sessionId: boundedId(window.localStorage.getItem(LEGACY_KEY)) };
    }
  } catch { /* Storage is an optional convenience, never a session authority. */ }
  return reconcileTerminalSelection(selection, workers, sessions);
}

export function saveTerminalSelection(selection: TerminalSelection): void {
  try { window.localStorage.setItem(KEY, JSON.stringify(selection)); } catch { /* Optional. */ }
}

function boundedId(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 && value.length <= 128 ? value : undefined;
}
