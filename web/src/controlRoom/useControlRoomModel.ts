import { useCallback, useRef, useState, type SetStateAction } from "react";

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
  type ControlRoomEventKind,
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
  loadParts?: typeof loadControlRoomParts;
  validateSession?: typeof validateBrowserSession;
};

export type ControlRoomPart = keyof ControlRoomSnapshot;

const EVERY_PART: readonly ControlRoomPart[] = [
  "hiveIdentity", "sessions", "workers", "workspaces", "tasks", "jiraTaskLinks", "decisions", "stewardAssists",
];

/** What each event kind can change, read from what each endpoint is built from
 * and which event each write to its tables records (ADR 0142).
 *
 * The roster, sessions and board are read for every kind: a worker's row
 * carries decisions and deliveries, a task's next move carries decisions and
 * worker liveness, and terminal selection reconciles workers against sessions.
 * The decision inbox is read only for decision and task changes: every write
 * to its tables records one or the other, and task-decision links record
 * tasks_changed. Jira links and steward assists move with tasks. Workspaces
 * come from worker profiles and the workspace folders. Runtime replacement,
 * a reset and any kind this table does not name reload everything.
 */
const PARTS_BY_KIND: Partial<Record<ControlRoomEventKind, readonly ControlRoomPart[]>> = {
  tasks_changed: ["hiveIdentity", "sessions", "workers", "tasks", "jiraTaskLinks", "decisions", "stewardAssists"],
  workers_changed: ["hiveIdentity", "sessions", "workers", "workspaces", "tasks"],
  sessions_changed: ["hiveIdentity", "sessions", "workers", "workspaces", "tasks"],
  decisions_changed: ["hiveIdentity", "sessions", "workers", "tasks", "decisions"],
  presence_changed: [],
  notifications_changed: [],
};

/** The snapshot parts a page of events can have changed. */
export function partsChangedBy(page: ControlRoomEventPage): ReadonlySet<ControlRoomPart> {
  if (page.reset_required) return new Set(EVERY_PART);
  const parts = new Set<ControlRoomPart>();
  for (const event of page.events) {
    for (const part of PARTS_BY_KIND[event.kind] ?? EVERY_PART) parts.add(part);
  }
  return parts;
}

export async function loadControlRoomParts(
  operatorToken: string,
  parts: ReadonlySet<ControlRoomPart>,
  signal?: AbortSignal,
): Promise<Partial<ControlRoomSnapshot>> {
  const read = <T,>(part: ControlRoomPart, fetch: () => Promise<T>) => (parts.has(part) ? fetch() : Promise.resolve(undefined));
  const [hiveIdentity, sessions, workers, workspaces, tasks, decisions, jiraTaskLinks] = await Promise.all([
    // Steward assists exist only inside an Apiary, which the hive identity says.
    parts.has("hiveIdentity") || parts.has("stewardAssists") ? fetchHive(operatorToken, signal) : Promise.resolve(undefined),
    read("sessions", () => fetchSessions(operatorToken, signal)),
    read("workers", () => fetchWorkers(operatorToken, signal)),
    read("workspaces", () => fetchWorkspaces(operatorToken, signal)),
    read("tasks", () => fetchTasks(operatorToken, signal)),
    read("decisions", () => fetchDecisions(operatorToken, signal)),
    read("jiraTaskLinks", () => fetchJiraTaskLinks(operatorToken, signal).catch(() => [])),
  ]);
  const loaded: Partial<ControlRoomSnapshot> = {};
  if (hiveIdentity) loaded.hiveIdentity = hiveIdentity;
  if (sessions) loaded.sessions = sessions;
  if (workers) loaded.workers = workers;
  if (workspaces) loaded.workspaces = workspaces;
  if (tasks) loaded.tasks = tasks;
  if (decisions) loaded.decisions = decisions;
  if (jiraTaskLinks) loaded.jiraTaskLinks = jiraTaskLinks;
  if (parts.has("stewardAssists")) {
    loaded.stewardAssists = hiveIdentity?.hive.apiary_id
      ? await fetchFederationStewardAssists(operatorToken, signal).catch(() => ({ incoming: [], sent: [], outbox: [] }))
      : { incoming: [], sent: [], outbox: [] };
  }
  return loaded;
}

export async function loadControlRoomSnapshot(operatorToken: string, signal?: AbortSignal): Promise<ControlRoomSnapshot> {
  const loaded = await loadControlRoomParts(operatorToken, new Set(EVERY_PART), signal);
  return {
    hiveIdentity: loaded.hiveIdentity,
    sessions: loaded.sessions ?? [],
    workers: loaded.workers ?? [],
    workspaces: loaded.workspaces ?? [],
    tasks: loaded.tasks ?? [],
    jiraTaskLinks: loaded.jiraTaskLinks ?? [],
    decisions: loaded.decisions ?? [],
    stewardAssists: loaded.stewardAssists,
  };
}

/** Owns the browser's single typed view of the control room.
 *
 * Commands may update one aggregate optimistically, while a refresh replaces the
 * complete snapshot and live-feed invalidation replaces, in one commit, every
 * part its events can have changed. Keeping those paths on one owner prevents
 * worker, task, and Jira views from drifting apart.
 */
export function useControlRoomModel({
  loadSnapshot = loadControlRoomSnapshot,
  loadParts = loadControlRoomParts,
  validateSession = validateBrowserSession,
}: Dependencies = {}) {
  const [snapshot, setSnapshot] = useState<ControlRoomSnapshot>(emptySnapshot);
  const [recentEvents, setRecentEvents] = useState<ControlRoomEvent[]>([]);
  const initialized = useRef(false);
  const revision = useRef<object>({});
  const commitSnapshot = useCallback((next: SetStateAction<ControlRoomSnapshot>) => {
    // Advance synchronously, outside React's potentially deferred/replayed updater.
    // Only the current identity and identities held by in-flight reads survive.
    revision.current = {};
    setSnapshot(next);
  }, []);

  const load = useCallback(loadSnapshot, [loadSnapshot]);
  const replace = useCallback((next: ControlRoomSnapshot) => { initialized.current = true; commitSnapshot(next); }, [commitSnapshot]);
  const clear = useCallback(() => {
    initialized.current = false;
    commitSnapshot(emptySnapshot);
    setRecentEvents([]);
  }, [commitSnapshot]);
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
    // Presence and notifications have their own App-level reads. Everything
    // else reloads only what its kinds can change; startup, a reset and an
    // unfamiliar kind reload the complete snapshot (ADR 0142).
    const parts = partsChangedBy(page);
    if (initialized.current && parts.size === 0) {
      setRecentEvents((current) => mergeRecentEvents(current, page));
      return undefined;
    }
    const complete = !initialized.current || parts.size === EVERY_PART.length;
    const startedAt = revision.current;
    const full = complete ? await load(operatorToken, signal) : undefined;
    const next = full ?? await loadParts(operatorToken, parts, signal);
    if (signal?.aborted) return undefined;
    // A command or another refresh committed while these endpoint reads ran.
    // Reject rather than skip: the feed must retry without advancing its cursor.
    if (startedAt !== revision.current) throw new Error("Control-room refresh superseded by newer state");
    if (full) replace(full);
    else commitSnapshot((current) => ({ ...current, ...next }));
    setRecentEvents((current) => mergeRecentEvents(current, page));
    return next;
  }, [commitSnapshot, load, loadParts, replace]);
  const setHiveIdentity = useCallback((hiveIdentity: HiveIdentity | undefined) => {
    commitSnapshot((current) => ({ ...current, hiveIdentity }));
  }, [commitSnapshot]);
  const setSessions = useCallback((sessions: SessionSummary[]) => {
    commitSnapshot((current) => ({ ...current, sessions }));
  }, [commitSnapshot]);
  const setWorkers = useCallback((workers: Worker[] | ((current: Worker[]) => Worker[])) => {
    commitSnapshot((current) => ({
      ...current,
      workers: typeof workers === "function" ? workers(current.workers) : workers,
    }));
  }, [commitSnapshot]);
  const setWorkspaces = useCallback((workspaces: WorkspaceChoice[]) => {
    commitSnapshot((current) => ({ ...current, workspaces }));
  }, [commitSnapshot]);
  const setTasks = useCallback((tasks: Task[] | ((current: Task[]) => Task[])) => {
    commitSnapshot((current) => ({
      ...current,
      tasks: typeof tasks === "function" ? tasks(current.tasks) : tasks,
    }));
  }, [commitSnapshot]);
  const setJiraTaskLinks = useCallback((jiraTaskLinks: JiraTaskLink[]) => {
    commitSnapshot((current) => ({ ...current, jiraTaskLinks }));
  }, [commitSnapshot]);
  const setDecisions = useCallback((decisions: DecisionRequest[] | ((current: DecisionRequest[]) => DecisionRequest[])) => {
    commitSnapshot((current) => ({
      ...current,
      decisions: typeof decisions === "function" ? decisions(current.decisions) : decisions,
    }));
  }, [commitSnapshot]);

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
  // Retain the newest durable sequences, not the last arrivals: reconnects can
  // replay an older page. Keep at most 17 entries while admitting an event;
  // never concatenate a backlog or scan it once per received event.
  const retained: ControlRoomEvent[] = [];
  const admit = (event: ControlRoomEvent) => {
    let low = 0;
    let high = retained.length;
    while (low < high) {
      const middle = (low + high) >>> 1;
      if (retained[middle].sequence < event.sequence) low = middle + 1;
      else high = middle;
    }
    if (retained[low]?.sequence === event.sequence) return;
    if (retained.length === 16 && low === 0) return;
    retained.splice(low, 0, event);
    if (retained.length > 16) retained.shift();
  };
  if (!page.reset_required) for (const event of current) admit(event);
  for (const event of page.events) admit(event);
  return retained;
}
