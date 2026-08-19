import { expect, test } from "vitest";

import type { DevelopmentRuntime, Health, TerminalHostStatus } from "../api";
import { runtimeUpdateSummary } from "./runtimeUpdates";

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
  expect(summary.label).toBe("Update ready");
  expect(summary.detail).toContain("abcdef1");
  expect(summary.busy).toBe(false);
});

test("reports a worker engine update separately, because it interrupts workers", () => {
  const summary = runtimeUpdateSummary(health("new"), host("old"), development({}));

  expect(summary.kind).toBe("worker_engine");
  expect(summary.detail).toContain("restarts loaded workers");
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
