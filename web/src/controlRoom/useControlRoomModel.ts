import { useCallback, useState } from "react";

import {
  fetchDecisions,
  fetchHive,
  fetchJiraTaskLinks,
  fetchSessions,
  fetchTasks,
  fetchWorkers,
  fetchWorkspaces,
  type DecisionRequest,
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
};

const emptySnapshot: ControlRoomSnapshot = {
  hiveIdentity: undefined,
  sessions: [],
  workers: [],
  workspaces: [],
  tasks: [],
  jiraTaskLinks: [],
  decisions: [],
};

export async function loadControlRoomSnapshot(operatorToken: string): Promise<ControlRoomSnapshot> {
  const [hiveIdentity, sessions, workers, workspaces, tasks, decisions, jiraTaskLinks] = await Promise.all([
    fetchHive(operatorToken),
    fetchSessions(operatorToken),
    fetchWorkers(operatorToken),
    fetchWorkspaces(operatorToken),
    fetchTasks(operatorToken),
    fetchDecisions(operatorToken),
    fetchJiraTaskLinks(operatorToken).catch(() => []),
  ]);
  return { hiveIdentity, sessions, workers, workspaces, tasks, jiraTaskLinks, decisions };
}

/** Owns the browser's single typed view of the control room.
 *
 * Commands may update one aggregate optimistically, while refresh and live-feed
 * invalidation replace the complete snapshot atomically. Keeping those paths on
 * one owner prevents worker, task, and Jira views from drifting apart.
 */
export function useControlRoomModel() {
  const [snapshot, setSnapshot] = useState<ControlRoomSnapshot>(emptySnapshot);

  const load = useCallback(loadControlRoomSnapshot, []);
  const replace = useCallback((next: ControlRoomSnapshot) => setSnapshot(next), []);
  const clear = useCallback(() => setSnapshot(emptySnapshot), []);
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
    load,
    replace,
    clear,
    setHiveIdentity,
    setSessions,
    setWorkers,
    setWorkspaces,
    setTasks,
    setJiraTaskLinks,
    setDecisions,
  };
}
