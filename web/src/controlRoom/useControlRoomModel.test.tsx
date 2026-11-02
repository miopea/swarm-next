import { act, renderHook } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import { BROWSER_SESSION_AUTH, type ControlRoomEventPage } from "../api";
import type { ControlRoomPart, ControlRoomSnapshot } from "./useControlRoomModel";
import { loadControlRoomParts, loadControlRoomSnapshot, mergeRecentEvents, partsChangedBy, useControlRoomModel } from "./useControlRoomModel";

const populated = {
  hiveIdentity: { operator: { id: "operator", display_name: "Operator" }, hive: { id: "hive", name: "Hive", operator_id: "operator", apiary_id: null } },
  sessions: [{ session_id: "session", running: true }],
  workers: [{ id: "worker", name: "Daisy" }],
  workspaces: [{ name: "repo", path: "/repo", kind: "repository", configured_worker_id: "worker" }],
  tasks: [{ id: "task", title: "Do work" }],
  jiraTaskLinks: [{ task_id: "task", issue_key: "WWD-1" }],
  decisions: [{ id: "decision", title: "Choose" }],
  stewardAssists: { incoming: [], sent: [], outbox: [] },
} as unknown as ControlRoomSnapshot;

const EVERY_PART: readonly ControlRoomPart[] = [
  "hiveIdentity", "sessions", "workers", "workspaces", "tasks", "jiraTaskLinks", "decisions", "stewardAssists",
];

test("replaces and clears the complete control-room snapshot as one ownership boundary", () => {
  const { result } = renderHook(() => useControlRoomModel());

  act(() => result.current.replace(populated));
  expect(result.current.workers).toHaveLength(1);
  expect(result.current.workspaces).toHaveLength(1);
  expect(result.current.jiraTaskLinks).toHaveLength(1);

  act(() => result.current.clear());
  expect(result.current.hiveIdentity).toBeUndefined();
  expect(result.current.sessions).toEqual([]);
  expect(result.current.workers).toEqual([]);
  expect(result.current.workspaces).toEqual([]);
  expect(result.current.tasks).toEqual([]);
  expect(result.current.jiraTaskLinks).toEqual([]);
  expect(result.current.decisions).toEqual([]);
  expect(result.current.stewardAssists).toEqual({ incoming: [], sent: [], outbox: [] });
});

test("scoped command results update one aggregate without discarding the others", () => {
  const { result } = renderHook(() => useControlRoomModel());
  act(() => result.current.replace(populated));

  act(() => result.current.setTasks((current) => [...current, { ...current[0], id: "second" }]));

  expect(result.current.tasks.map((task) => task.id)).toEqual(["task", "second"]);
  expect(result.current.workers).toEqual(populated.workers);
  expect(result.current.jiraTaskLinks).toEqual(populated.jiraTaskLinks);
});

test("restores the trusted browser session and snapshot through one model operation", async () => {
  const validateSession = vi.fn().mockResolvedValue(undefined);
  const loadSnapshot = vi.fn().mockResolvedValue(populated);
  const { result } = renderHook(() => useControlRoomModel({ loadSnapshot, validateSession }));

  await act(async () => { await result.current.restoreBrowserSession(); });

  expect(validateSession).toHaveBeenCalledOnce();
  expect(loadSnapshot).toHaveBeenCalledWith(BROWSER_SESSION_AUTH, undefined);
  expect(result.current.workers).toEqual(populated.workers);
  expect(result.current.tasks).toEqual(populated.tasks);
});

test("does not apply a live-feed refresh after its owning effect is cancelled", async () => {
  let finishLoad!: (snapshot: ControlRoomSnapshot) => void;
  const loadSnapshot = vi.fn(() => new Promise<ControlRoomSnapshot>((resolve) => { finishLoad = resolve; }));
  const { result } = renderHook(() => useControlRoomModel(oneLoader(loadSnapshot)));
  const controller = new AbortController();
  const page = eventPage(1, false);

  let refresh!: Promise<Partial<ControlRoomSnapshot> | undefined>;
  act(() => { refresh = result.current.refreshFromEvents("operator", page, controller.signal); });
  controller.abort();
  await act(async () => { finishLoad(populated); await refresh; });

  expect(result.current.workers).toEqual([]);
  expect(result.current.recentEvents).toEqual([]);
});

test("presence-only pages preserve the snapshot but reset and task events reload it", async () => {
  const loadSnapshot = vi.fn().mockResolvedValue(populated);
  const { result } = renderHook(() => useControlRoomModel(oneLoader(loadSnapshot)));
  act(() => result.current.replace(populated));
  const page = eventPage(2, false);
  page.events[0].kind = "presence_changed";
  await act(async () => { await result.current.refreshFromEvents("operator", page); });
  expect(loadSnapshot).not.toHaveBeenCalled();
  expect(result.current.tasks).toBe(populated.tasks);
  expect(result.current.recentEvents).toEqual(page.events);
  await act(async () => { await result.current.refreshFromEvents("operator", { ...page, reset_required: true }); });
  expect(loadSnapshot).toHaveBeenCalledTimes(1);
  await act(async () => { await result.current.refreshFromEvents("operator", eventPage(3, false)); });
  expect(loadSnapshot).toHaveBeenCalledTimes(2);
  act(() => result.current.clear());
  await act(async () => { await result.current.refreshFromEvents("operator", page); });
  expect(loadSnapshot).toHaveBeenCalledTimes(3);
});

test("a refresh overtaken by a confirmed task command retries without replacing newer state", async () => {
  let finishLoad!: (snapshot: ControlRoomSnapshot) => void;
  const loadSnapshot = vi.fn(() => new Promise<ControlRoomSnapshot>((resolve) => { finishLoad = resolve; }));
  const { result } = renderHook(() => useControlRoomModel(oneLoader(loadSnapshot)));
  act(() => result.current.replace(populated));
  let refresh!: Promise<Partial<ControlRoomSnapshot> | undefined>;
  act(() => { refresh = result.current.refreshFromEvents("operator", eventPage(4, false)); });
  const newerTasks = [{ ...populated.tasks[0], title: "Confirmed newer task" }];
  act(() => result.current.setTasks(newerTasks));
  await act(async () => {
    finishLoad(populated);
    await expect(refresh).rejects.toThrow("superseded");
  });
  expect(result.current.tasks).toEqual(newerTasks);
  expect(result.current.recentEvents).toEqual([]);
  const retrySnapshot = { ...populated, tasks: newerTasks };
  loadSnapshot.mockResolvedValueOnce(retrySnapshot);
  await act(async () => { await result.current.refreshFromEvents("operator", eventPage(4, false)); });
  expect(result.current.tasks).toEqual(newerTasks);
  expect(result.current.recentEvents.map((event) => event.sequence)).toEqual([4]);
});

test.each([
  "clear", "replace", "setHiveIdentity", "setSessions", "setWorkers",
  "setWorkspaces", "setTasks", "setJiraTaskLinks", "setDecisions",
] as const)("%s invalidates an older in-flight snapshot", async (command) => {
  let finishLoad!: (snapshot: ControlRoomSnapshot) => void;
  const loadSnapshot = vi.fn(() => new Promise<ControlRoomSnapshot>((resolve) => { finishLoad = resolve; }));
  const { result } = renderHook(() => useControlRoomModel(oneLoader(loadSnapshot)));
  act(() => result.current.replace(populated));
  let refresh!: Promise<Partial<ControlRoomSnapshot> | undefined>;
  act(() => { refresh = result.current.refreshFromEvents("operator", eventPage(4, false)); });
  act(() => {
    const model = result.current;
    switch (command) {
      case "clear": model.clear(); break;
      case "replace": model.replace(populated); break;
      case "setHiveIdentity": model.setHiveIdentity(populated.hiveIdentity); break;
      case "setSessions": model.setSessions(populated.sessions); break;
      case "setWorkers": model.setWorkers((current) => current); break;
      case "setWorkspaces": model.setWorkspaces(populated.workspaces); break;
      case "setTasks": model.setTasks((current) => current); break;
      case "setJiraTaskLinks": model.setJiraTaskLinks(populated.jiraTaskLinks); break;
      case "setDecisions": model.setDecisions((current) => current); break;
    }
  });
  await act(async () => { finishLoad(populated); await expect(refresh).rejects.toThrow("superseded"); });
  expect(result.current.recentEvents).toEqual([]);
  expect(result.current.tasks).toEqual(command === "clear" ? [] : populated.tasks);
});

test("the last completed refresh cannot undo a newer already committed refresh", async () => {
  let finishOld!: (snapshot: ControlRoomSnapshot) => void;
  const loadSnapshot = vi.fn(() => new Promise<ControlRoomSnapshot>((resolve) => { finishOld = resolve; }));
  const { result } = renderHook(() => useControlRoomModel(oneLoader(loadSnapshot)));
  act(() => result.current.replace(populated));
  let oldRefresh!: Promise<Partial<ControlRoomSnapshot> | undefined>;
  act(() => { oldRefresh = result.current.refreshFromEvents("operator", eventPage(4, false)); });
  const newer = { ...populated, tasks: [{ ...populated.tasks[0], title: "Newer refresh" }] };
  loadSnapshot.mockResolvedValueOnce(newer);
  await act(async () => { await result.current.refreshFromEvents("operator", eventPage(5, false)); });
  await act(async () => { finishOld(populated); await expect(oldRefresh).rejects.toThrow("superseded"); });
  expect(result.current.tasks).toEqual(newer.tasks);
  expect(result.current.recentEvents.map((event) => event.sequence)).toEqual([5]);
});

test("snapshot reads propagate their owner cancellation to every endpoint", async () => {
  const fetch = vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
    const path = String(input);
    const payload = path.endsWith("/hive") ? { ...populated.hiveIdentity, hive: { ...populated.hiveIdentity!.hive, apiary_id: "apiary" } }
      : path.endsWith("/sessions") ? { sessions: [] } : [];
    return new Response(JSON.stringify(payload), { status: 200 });
  });
  try {
    const controller = new AbortController();
    await loadControlRoomSnapshot("operator", controller.signal);
    expect(fetch).toHaveBeenCalledTimes(8);
    for (const [, init] of fetch.mock.calls) expect(init?.signal).toBe(controller.signal);
  } finally { fetch.mockRestore(); }
});

test("a worker change reloads the roster, sessions, board and workspaces, and keeps the decision inbox", async () => {
  const loadSnapshot = vi.fn().mockResolvedValue(populated);
  const workers = [{ id: "worker", name: "Daisy, now working" }];
  const loadParts = vi.fn().mockResolvedValue({ workers });
  const { result } = renderHook(() => useControlRoomModel({ loadSnapshot, loadParts }));
  act(() => result.current.replace(populated));

  await act(async () => { await result.current.refreshFromEvents("operator", eventPage(6, false, "workers_changed")); });

  expect(loadSnapshot).not.toHaveBeenCalled();
  expect([...loadParts.mock.calls[0][1]].sort()).toEqual(["hiveIdentity", "sessions", "tasks", "workers", "workspaces"]);
  expect(result.current.workers).toEqual(workers);
  expect(result.current.decisions).toBe(populated.decisions);
  expect(result.current.jiraTaskLinks).toBe(populated.jiraTaskLinks);
  expect(result.current.recentEvents.map((event) => event.sequence)).toEqual([6]);
});

test.each([
  ["tasks_changed", ["decisions", "hiveIdentity", "jiraTaskLinks", "sessions", "stewardAssists", "tasks", "workers"]],
  ["workers_changed", ["hiveIdentity", "sessions", "tasks", "workers", "workspaces"]],
  ["sessions_changed", ["hiveIdentity", "sessions", "tasks", "workers", "workspaces"]],
  ["decisions_changed", ["decisions", "hiveIdentity", "sessions", "tasks", "workers"]],
  ["presence_changed", []],
  ["notifications_changed", []],
  ["runtime_changed", EVERY_PART],
  ["a kind this browser does not know", EVERY_PART],
] as const)("%s reloads exactly what it can change", (kind, parts) => {
  const page = eventPage(7, false, kind as ControlRoomEventPage["events"][number]["kind"]);
  expect([...partsChangedBy(page)].sort()).toEqual([...parts].sort());
  expect([...partsChangedBy({ ...page, reset_required: true })].sort()).toEqual([...EVERY_PART].sort());
});

test("a page carrying several kinds reloads what any of them can change", () => {
  const page = eventPage(8, false, "workers_changed");
  page.events.push({ ...page.events[0], sequence: 9, kind: "decisions_changed" });
  expect([...partsChangedBy(page)].sort()).toEqual(["decisions", "hiveIdentity", "sessions", "tasks", "workers", "workspaces"]);
});

test("a partial read asks only the endpoints for its parts", async () => {
  const fetch = vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
    const path = String(input);
    const payload = path.endsWith("/hive") ? populated.hiveIdentity : path.endsWith("/sessions") ? { sessions: [] } : [];
    return new Response(JSON.stringify(payload), { status: 200 });
  });
  try {
    const loaded = await loadControlRoomParts("operator", partsChangedBy(eventPage(10, false, "workers_changed")));
    const paths = fetch.mock.calls.map(([input]) => new URL(String(input), "http://hive").pathname).sort();
    expect(paths).toEqual(["/api/v1/hive", "/api/v1/tasks", "/api/v1/terminal/sessions", "/api/v1/workers", "/api/v1/workspaces"]);
    expect(Object.keys(loaded).sort()).toEqual(["hiveIdentity", "sessions", "tasks", "workers", "workspaces"]);
  } finally { fetch.mockRestore(); }
});

test("deduplicates, bounds, and resets recent control-room evidence", () => {
  const first = Array.from({ length: 16 }, (_, index) => eventPage(index + 1, false).events[0]);
  const merged = mergeRecentEvents(first, {
    events: [eventPage(16, false).events[0], eventPage(17, false).events[0]],
    next_cursor: 17,
    reset_required: false,
  });
  expect(merged.map((event) => event.sequence)).toEqual(Array.from({ length: 16 }, (_, index) => index + 2));
  expect(mergeRecentEvents(merged, eventPage(40, true)).map((event) => event.sequence)).toEqual([40]);
});

test("a replayed older backlog cannot displace newer recent evidence", () => {
  const current = Array.from({ length: 16 }, (_, index) => eventPage(500 + index, false).events[0]);
  const events = Array.from({ length: 500 }, (_, index) => eventPage(index + 1, false).events[0]);
  const replay = { events, next_cursor: 500, reset_required: false };
  expect(mergeRecentEvents(current, replay)).toEqual(current);
  expect(mergeRecentEvents(current, { ...replay, reset_required: true }).map((event) => event.sequence))
    .toEqual(Array.from({ length: 16 }, (_, index) => 485 + index));
});

test("bounded recent evidence deduplicates and orders a large mixed page", () => {
  const events = Array.from({ length: 5_000 }, (_, index) => eventPage((index * 37) % 1_000, false).events[0]);
  const first = eventPage(999, false).events[0];
  const merged = mergeRecentEvents([first], { events, next_cursor: 999, reset_required: false });
  expect(merged.map((event) => event.sequence)).toEqual(Array.from({ length: 16 }, (_, index) => 984 + index));
  expect(merged.at(-1)).toBe(first);
});

/** One deferred read standing in for both the complete and the partial loader. */
function oneLoader(load: (operatorToken: string, signal?: AbortSignal) => Promise<ControlRoomSnapshot>) {
  return { loadSnapshot: load, loadParts: (operatorToken: string, _parts: ReadonlySet<ControlRoomPart>, signal?: AbortSignal) => load(operatorToken, signal) };
}

function eventPage(sequence: number, resetRequired: boolean, kind: ControlRoomEventPage["events"][number]["kind"] = "tasks_changed"): ControlRoomEventPage {
  return {
    events: [{ sequence, hive_id: "hive", kind, occurred_at: sequence }],
    next_cursor: sequence,
    reset_required: resetRequired,
  };
}
