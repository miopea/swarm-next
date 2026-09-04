import { act, renderHook } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import { BROWSER_SESSION_AUTH, type ControlRoomEventPage } from "../api";
import type { ControlRoomSnapshot } from "./useControlRoomModel";
import { loadControlRoomSnapshot, mergeRecentEvents, useControlRoomModel } from "./useControlRoomModel";

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
  const { result } = renderHook(() => useControlRoomModel({ loadSnapshot }));
  const controller = new AbortController();
  const page = eventPage(1, false);

  let refresh!: Promise<ControlRoomSnapshot | undefined>;
  act(() => { refresh = result.current.refreshFromEvents("operator", page, controller.signal); });
  controller.abort();
  await act(async () => { finishLoad(populated); await refresh; });

  expect(result.current.workers).toEqual([]);
  expect(result.current.recentEvents).toEqual([]);
});

test("presence-only pages preserve the snapshot but reset and task events reload it", async () => {
  const loadSnapshot = vi.fn().mockResolvedValue(populated);
  const { result } = renderHook(() => useControlRoomModel({ loadSnapshot }));
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

function eventPage(sequence: number, resetRequired: boolean): ControlRoomEventPage {
  return {
    events: [{ sequence, hive_id: "hive", kind: "tasks_changed", occurred_at: sequence }],
    next_cursor: sequence,
    reset_required: resetRequired,
  };
}
