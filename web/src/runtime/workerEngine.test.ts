import { expect, test } from "vitest";

import { engineUpdateCost, workerEngineMatches, workerEngineUpdateRequired, workersMidCommand } from "./workerEngine";

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

test("separates workers that are merely loaded from workers actually working", () => {
  // Replacing the engine while a worker rests costs nothing; doing it
  // mid-command kills work in progress. The warning only meant anything once
  // it could tell those apart.
  const workers = [
    { name: "Queen", attention_state: "buzzing" },
    { name: "Public Website", attention_state: "resting" },
    { name: "BudgetBug", attention_state: "buzzing" },
    { name: "Sculpt Studio", attention_state: "sleeping" },
  ];

  expect(workersMidCommand(workers)).toEqual(["Queen", "BudgetBug"]);
});

test("says plainly when an engine update costs nothing in progress", () => {
  expect(engineUpdateCost([])).toContain("nothing in progress is lost");
});

test("names the work an engine update would interrupt", () => {
  expect(engineUpdateCost(["Queen"])).toContain("1 worker is running a command");
  expect(engineUpdateCost(["Queen"])).toContain("Queen");
  expect(engineUpdateCost(["Queen"])).toContain("not resumed");

  const many = engineUpdateCost(["A", "B", "C", "D", "E"]);
  expect(many).toContain("5 workers are running");
  expect(many).toContain("A, B, C and 2 more");
});
