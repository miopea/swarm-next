import { expect, test } from "vitest";

import type { DevelopmentRuntime, Health, TerminalHostStatus } from "../api";
import { nextRuntimeUpdate, runtimeUpdateSummary } from "./runtimeUpdates";

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

test("work in progress outranks work waiting", () => {
  const summary = runtimeUpdateSummary(
    health("new"),
    host("old"),
    development({ state: "building", reload_available: true }),
  );

  expect(summary.kind).toBe("building");
  expect(summary.busy).toBe(true);
  expect(summary.detail).toContain("abcdef1");
});

test("a stopped build outranks everything, since nothing else will move it", () => {
  const summary = runtimeUpdateSummary(
    health("new"),
    host("old"),
    development({ state: "failed", reload_available: true }),
  );

  expect(summary.kind).toBe("failed");
  expect(summary.busy).toBe(false);
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

  expect(nextRuntimeUpdate(building, undefined, undefined, undefined)).toBe(building);
});

test("replaces the answer as soon as any subsystem reports", () => {
  const building = runtimeUpdateSummary(health("same"), host("same"), development({ state: "building" }));
  const settled = nextRuntimeUpdate(building, health("same"), host("same"), development({}));

  expect(settled?.kind).toBe("none");
});
