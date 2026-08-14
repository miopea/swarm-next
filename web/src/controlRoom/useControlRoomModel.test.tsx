import { act, renderHook } from "@testing-library/react";
import { expect, test } from "vitest";

import type { ControlRoomSnapshot } from "./useControlRoomModel";
import { useControlRoomModel } from "./useControlRoomModel";

const populated = {
  hiveIdentity: { operator: { id: "operator", display_name: "Operator" }, hive: { id: "hive", name: "Hive", operator_id: "operator", apiary_id: null } },
  sessions: [{ session_id: "session", running: true }],
  workers: [{ id: "worker", name: "Daisy" }],
  workspaces: [{ name: "repo", path: "/repo", kind: "repository", configured_worker_id: "worker" }],
  tasks: [{ id: "task", title: "Do work" }],
  jiraTaskLinks: [{ task_id: "task", issue_key: "WWD-1" }],
  decisions: [{ id: "decision", title: "Choose" }],
} as ControlRoomSnapshot;

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
});

test("scoped command results update one aggregate without discarding the others", () => {
  const { result } = renderHook(() => useControlRoomModel());
  act(() => result.current.replace(populated));

  act(() => result.current.setTasks((current) => [...current, { ...current[0], id: "second" }]));

  expect(result.current.tasks.map((task) => task.id)).toEqual(["task", "second"]);
  expect(result.current.workers).toEqual(populated.workers);
  expect(result.current.jiraTaskLinks).toEqual(populated.jiraTaskLinks);
});
