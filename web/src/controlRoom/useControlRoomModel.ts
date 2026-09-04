import { useCallback, useRef, useState } from "react";

import {
  BROWSER_SESSION_AUTH,
  fetchDecisions,
  fetchFederationStewardAssists,
  fetchHive,
  fetchJiraTaskLinks,
  fetchSessions,
  fetchTasks,
  fetchWorkers,
  fetchWorkspaces,
  validateBrowserSession,
  type ControlRoomEvent,
  type ControlRoomEventPage,
  type DecisionRequest,
  type FederationStewardAssistLocalState,
  type HiveIdentity,
  type JiraTaskLink,
  type SessionSummary,
  type Task,
  type Worker,
  type WorkspaceChoice,
} from "../api";

export type ControlRoomSnapshot = {
  hiveIdentity: HiveIdentity | undefined;
  sessions: SessionSummary[];
  workers: Worker[];
  workspaces: WorkspaceChoice[];
  tasks: Task[];
  jiraTaskLinks: JiraTaskLink[];
  decisions: DecisionRequest[];
  stewardAssists?: FederationStewardAssistLocalState;
};

const emptySnapshot: ControlRoomSnapshot = {
  hiveIdentity: undefined,
  sessions: [],
  workers: [],
  workspaces: [],
  tasks: [],
  jiraTaskLinks: [],
  decisions: [],
  stewardAssists: { incoming: [], sent: [], outbox: [] },
};

type Dependencies = {
  loadSnapshot?: typeof loadControlRoomSnapshot;
  validateSession?: typeof validateBrowserSession;
};

export async function loadControlRoomSnapshot(operatorToken: string, signal?: AbortSignal): Promise<ControlRoomSnapshot> {
  const [hiveIdentity, sessions, workers, workspaces, tasks, decisions, jiraTaskLinks] = await Promise.all([
    fetchHive(operatorToken, signal),
    fetchSessions(operatorToken, signal),
    fetchWorkers(operatorToken, signal),
    fetchWorkspaces(operatorToken, signal),
    fetchTasks(operatorToken, signal),
    fetchDecisions(operatorToken, signal),
    fetchJiraTaskLinks(operatorToken, signal).catch(() => []),
  ]);
  const stewardAssists = hiveIdentity.hive.apiary_id
    ? await fetchFederationStewardAssists(operatorToken, signal).catch(() => ({ incoming: [], sent: [], outbox: [] }))
    : { incoming: [], sent: [], outbox: [] };
  return { hiveIdentity, sessions, workers, workspaces, tasks, jiraTaskLinks, decisions, stewardAssists };
}

/** Owns the browser's single typed view of the control room.
 *
 * Commands may update one aggregate optimistically, while refresh and live-feed
 * invalidation replace the complete snapshot atomically. Keeping those paths on
 * one owner prevents worker, task, and Jira views from drifting apart.
 */
export function useControlRoomModel({
  loadSnapshot = loadControlRoomSnapshot,
  validateSession = validateBrowserSession,
}: Dependencies = {}) {
  const [snapshot, setSnapshot] = useState<ControlRoomSnapshot>(emptySnapshot);
  const [recentEvents, setRecentEvents] = useState<ControlRoomEvent[]>([]);
  const initialized = useRef(false);

  const load = useCallback(loadSnapshot, [loadSnapshot]);
  const replace = useCallback((next: ControlRoomSnapshot) => { initialized.current = true; setSnapshot(next); }, []);
  const clear = useCallback(() => {
    initialized.current = false;
    setSnapshot(emptySnapshot);
    setRecentEvents([]);
  }, []);
  const restoreBrowserSession = useCallback(async (signal?: AbortSignal) => {
    await validateSession();
    const next = await load(BROWSER_SESSION_AUTH, signal);
    if (signal?.aborted) return undefined;
    replace(next);
    return next;
  }, [load, replace, validateSession]);
  const refreshFromEvents = useCallback(async (
    operatorToken: string,
    page: ControlRoomEventPage,
    signal?: AbortSignal,
  ) => {
    if (signal?.aborted) return undefined;
    // These events have their own App-level reads; they do not invalidate the
    // roster/task snapshot. Reset, startup and unfamiliar event kinds refresh fully.
    if (initialized.current && !page.reset_required && page.events.every((event) => event.kind === "presence_changed" || event.kind === "notifications_changed")) {
      setRecentEvents((current) => mergeRecentEvents(current, page));
      return undefined;
    }
    const next = await load(operatorToken, signal);
    if (signal?.aborted) return undefined;
    replace(next);
    setRecentEvents((current) => mergeRecentEvents(current, page));
    return next;
  }, [load, replace]);
  const setHiveIdentity = useCallback((hiveIdentity: HiveIdentity | undefined) => {
    setSnapshot((current) => ({ ...current, hiveIdentity }));
  }, []);
  const setSessions = useCallback((sessions: SessionSummary[]) => {
    setSnapshot((current) => ({ ...current, sessions }));
  }, []);
  const setWorkers = useCallback((workers: Worker[] | ((current: Worker[]) => Worker[])) => {
    setSnapshot((current) => ({
      ...current,
      workers: typeof workers === "function" ? workers(current.workers) : workers,
    }));
  }, []);
  const setWorkspaces = useCallback((workspaces: WorkspaceChoice[]) => {
    setSnapshot((current) => ({ ...current, workspaces }));
  }, []);
  const setTasks = useCallback((tasks: Task[] | ((current: Task[]) => Task[])) => {
    setSnapshot((current) => ({
      ...current,
      tasks: typeof tasks === "function" ? tasks(current.tasks) : tasks,
    }));
  }, []);
  const setJiraTaskLinks = useCallback((jiraTaskLinks: JiraTaskLink[]) => {
    setSnapshot((current) => ({ ...current, jiraTaskLinks }));
  }, []);
  const setDecisions = useCallback((decisions: DecisionRequest[] | ((current: DecisionRequest[]) => DecisionRequest[])) => {
    setSnapshot((current) => ({
      ...current,
      decisions: typeof decisions === "function" ? decisions(current.decisions) : decisions,
    }));
  }, []);

  return {
    ...snapshot,
    recentEvents,
    load,
    replace,
    clear,
    restoreBrowserSession,
    refreshFromEvents,
    setHiveIdentity,
    setSessions,
    setWorkers,
    setWorkspaces,
    setTasks,
    setJiraTaskLinks,
    setDecisions,
  };
}

export function mergeRecentEvents(current: ControlRoomEvent[], page: ControlRoomEventPage) {
  if (page.reset_required) return page.events.slice(-16);
  return [...current, ...page.events]
    .filter((event, index, events) =>
      events.findIndex((candidate) => candidate.sequence === event.sequence) === index,
    )
    .slice(-16);
}
