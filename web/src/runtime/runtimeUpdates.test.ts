import { expect, test } from "vitest";

import type { DevelopmentRuntime, Health, TerminalHostStatus } from "../api";
import { nextRuntimeUpdates, runtimeUpdates, runtimeUpdateSummary } from "./runtimeUpdates";

const health = (buildId: string): Health => ({
  status: "ok",
  version: "0.1.0-dev-aaaaaaaaaaaa",
  worker_engine_build_id: buildId,
} as Health);

const host = (buildId: string): TerminalHostStatus => ({
  host_version: "0.1.0-dev-aaaaaaaaaaaa",
  host_build_id: buildId,
} as TerminalHostStatus);

const development = (over: Partial<DevelopmentRuntime>): DevelopmentRuntime => ({
  enabled: true,
  version: "0.1.0",
  state: "idle",
  reload_available: false,
  source_revision: "abcdef1234567",
  source_dirty: false,
  ...over,
});

test("says nothing when the runtime is current", () => {
  const summary = runtimeUpdateSummary(health("same"), host("same"), development({}));

  expect(summary.kind).toBe("none");
  expect(summary.label).toBe("");
});

test("offers an App update that leaves workers online", () => {
  const summary = runtimeUpdateSummary(health("same"), host("same"), development({ reload_available: true }));

  expect(summary.kind).toBe("app");
  expect(summary.label).toBe("App and API update");
  expect(summary.detail).toContain("abcdef1");
  expect(summary.busy).toBe(false);
});

test("reports a worker engine update separately, because it interrupts workers", () => {
  const summary = runtimeUpdateSummary(health("new"), host("old"), development({}));

  expect(summary.kind).toBe("worker_engine");
  expect(summary.detail).toContain("restarts loaded workers");
});

test("names both subsystems the way the settings page names them", () => {
  // The indicator opens the settings page. A word that appears in one and not
  // the other sends the operator looking for something that is not there.
  const engine = runtimeUpdateSummary(health("new"), host("old"), development({}));
  const app = runtimeUpdateSummary(health("same"), host("same"), development({ reload_available: true }));

  expect(engine.label).toContain("Worker engine");
  expect(app.label).toContain("App and API");
});

test("says a worker engine replacement is under way, and outranks everything while it is", () => {
  // Nothing said this was happening: the only in-progress state was the app
  // build, so the update that actually takes workers away ran unannounced.
  const summary = runtimeUpdateSummary(
    health("new"),
    { ...host("old"), draining: true },
    development({ state: "building", reload_available: true }),
  );

  expect(summary.kind).toBe("worker_engine");
  expect(summary.label).toBe("Updating worker engine");
  expect(summary.busy).toBe(true);
});

test("a build in progress is reported even beside a worker engine update", () => {
  // This used to assert that a running build outranked everything, which was
  // only ever true because a single pill had to choose. Both are reported now,
  // so neither hides the other.
  const all = runtimeUpdates(
    health("new"),
    host("old"),
    development({ state: "building", reload_available: true }),
  );
  const building = all.find((entry) => entry.kind === "building");

  expect(all.map((entry) => entry.kind)).toEqual(["worker_engine", "building"]);
  expect(building?.busy).toBe(true);
  expect(building?.detail).toContain("abcdef1");
});

test("a stopped build is reported, since nothing else will move it", () => {
  const all = runtimeUpdates(
    health("new"),
    host("old"),
    development({ state: "failed", reload_available: true }),
  );
  const failed = all.find((entry) => entry.kind === "failed");

  expect(failed).toBeDefined();
  expect(failed?.busy).toBe(false);
  // And the engine update beside it is not swallowed by the failure.
  expect(all.some((entry) => entry.kind === "worker_engine")).toBe(true);
});

test("stays quiet when development mode is off", () => {
  const summary = runtimeUpdateSummary(
    health("same"),
    host("same"),
    development({ enabled: false, reload_available: true }),
  );

  expect(summary.kind).toBe("none");
});

test("reports nothing rather than guessing before runtime facts arrive", () => {
  expect(runtimeUpdateSummary(undefined, undefined, undefined).kind).toBe("none");
});

test("keeps the last answer when a refresh learns nothing", () => {
  // The App and API build restarts the API, so a refresh that returns nothing
  // is the expected middle of the operation this indicator is reporting.
  // Treating silence as "nothing to update" took the indicator off the header
  // exactly then.
  const building = runtimeUpdateSummary(health("same"), host("same"), development({ state: "building" }));

  expect(nextRuntimeUpdates([building], undefined, undefined, undefined)).toEqual([building]);
});

test("replaces the answer as soon as any subsystem reports", () => {
  const building = runtimeUpdateSummary(health("same"), host("same"), development({ state: "building" }));
  const settled = nextRuntimeUpdates([building], health("same"), host("same"), development({}));

  expect(settled).toEqual([]);
});

test("does not report uncommitted work in progress as an update waiting", () => {
  // The indicator would never go quiet while anyone is editing the checkout,
  // and an alert that is always on is not an alert.
  const summary = runtimeUpdateSummary(health("same"), host("same"), development({
    reload_available: true,
    source_dirty: true,
    source_revision: "ed715fe3c3f3",
    deployed_source_revision: "ed715fe3c3f3",
  }));

  expect(summary.kind).toBe("none");
});

test("still reports an update when the working copy is a different commit", () => {
  const summary = runtimeUpdateSummary(health("same"), host("same"), development({
    reload_available: true,
    source_dirty: true,
    source_revision: "9668d65abcde",
    deployed_source_revision: "ed715fe3c3f3",
  }));

  expect(summary.kind).toBe("app");
});

test("reports a provider update, which is installed and running nowhere", () => {
  // The settings card detected Claude 2.1.237 with eight workers behind it and
  // the header said nothing. A provider update is the one that is installed and
  // running nowhere until each worker restarts.
  const summary = runtimeUpdateSummary(health("same"), host("same"), development({}), [
    { provider: "claude_code", version: "2.1.237", installed_at: 1_000, worker_ids: ["a", "b", "c"] },
  ]);

  expect(summary.kind).toBe("provider");
  expect(summary.label).toBe("Provider update");
  expect(summary.detail).toContain("Claude 2.1.237 is installed");
  expect(summary.detail).toContain("3 running workers started before that");
});

test("ranks a worker engine update above a provider one", () => {
  // Both need a restart; replacing the engine is the more fundamental of the
  // two, and doing it also restarts the providers.
  const summary = runtimeUpdateSummary(health("new"), host("old"), development({}), [
    { provider: "claude_code", version: "2.1.237", installed_at: 1_000, worker_ids: ["a"] },
  ]);

  expect(summary.kind).toBe("worker_engine");
});

test("counts a worker once when both providers are behind", () => {
  const summary = runtimeUpdateSummary(health("same"), host("same"), development({}), [
    { provider: "claude_code", version: "2.1.237", installed_at: 1, worker_ids: ["a", "b"] },
    { provider: "codex", version: null, installed_at: 1, worker_ids: ["b"] },
  ]);

  expect(summary.detail).toContain("2 running workers");
});

test("reports every pending update, not only the most severe", () => {
  // Ranked into one pill, a provider update stayed hidden behind a worker
  // engine update until that was dealt with — and they are independent.
  const all = runtimeUpdates(
    health("new"),
    host("old"),
    development({ reload_available: true }),
    [{ provider: "claude_code", version: "2.1.237", installed_at: 1, worker_ids: ["a"] }],
  );

  expect(all.map((entry) => entry.kind)).toEqual(["worker_engine", "provider", "app"]);
});

test("orders them by what they cost the operator", () => {
  // The engine takes workers away; a provider update is installed and running
  // nowhere until each worker restarts; App and API leaves workers online.
  const all = runtimeUpdates(health("same"), host("same"), development({ reload_available: true }), [
    { provider: "codex", version: null, installed_at: 1, worker_ids: ["a"] },
  ]);

  expect(all.map((entry) => entry.kind)).toEqual(["provider", "app"]);
});

test("says nothing at all when every subsystem is current", () => {
  expect(runtimeUpdates(health("same"), host("same"), development({}), [])).toEqual([]);
});

test("reports one entry per subsystem, never two for the same one", () => {
  // A failed build and an available reload are the same subsystem disagreeing
  // with itself; only the current state of it is reported.
  const all = runtimeUpdates(
    health("same"),
    host("same"),
    development({ state: "failed", reload_available: true }),
    [],
  );

  expect(all.map((entry) => entry.kind)).toEqual(["failed"]);
});
