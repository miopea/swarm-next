import { expect, test } from "vitest";

import { workerEngineMatches, workerEngineUpdateRequired } from "./workerEngine";

const host = {
  protocol_version: 7,
  host_version: "older-release",
  host_build_id: "engine-a",
  draining: false,
  running_sessions: 2,
  retained_sessions: 2,
};

test("requires a worker restart only when the engine artifact changes", () => {
  const appOnlyRelease = { status: "ok" as const, version: "newer-release", worker_engine_build_id: "engine-a" };
  expect(workerEngineUpdateRequired(appOnlyRelease, host)).toBe(false);
  expect(workerEngineMatches(appOnlyRelease, host)).toBe(true);
  expect(workerEngineUpdateRequired({ ...appOnlyRelease, worker_engine_build_id: "engine-b" }, host)).toBe(true);
});

test("falls back to release identity while an older host has no artifact id", () => {
  expect(workerEngineUpdateRequired(
    { status: "ok", version: "newer-release" },
    { ...host, host_build_id: null },
  )).toBe(true);
});
