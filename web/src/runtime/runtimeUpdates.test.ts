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
  deployed_source_published: true,
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
    deployed_source_published: true,
    source_revision: "ed715fe3c3f3",
    deployed_source_revision: "ed715fe3c3f3",
  }));

  expect(summary.kind).toBe("none");
});

test("still reports an update when the working copy is a different commit", () => {
  const summary = runtimeUpdateSummary(health("same"), host("same"), development({
    reload_available: true,
    source_dirty: true,
    deployed_source_published: true,
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

test("only the updates that take workers away carry a consequence", () => {
  // The confirmation's weight is driven by this field rather than by a list of
  // kinds, so the classification has to be right here or the warning lands on
  // the wrong thing.
  const engine = runtimeUpdates(
    { version: "0.1.0", worker_engine_build_id: "installed" } as never,
    { protocol_version: 1, draining: false, running_sessions: 0, worker_engine_build_id: "running" } as never,
    undefined,
    [],
  ).find((update) => update.kind === "worker_engine");
  expect(engine?.consequence).toContain("stopped");
  expect(engine?.action).toBe("apply_worker_engine");

  const release = runtimeUpdates(
    undefined,
    undefined,
    { enabled: true, state: "idle", reload_available: true, source_revision: "5394d9a", deployed_source_revision: "66c26f5", source_dirty: false } as never,
    [],
  ).find((update) => update.kind === "app");
  // Workers stay online through an App and API release, so nothing is lost and
  // the dialog must not claim otherwise.
  expect(release?.consequence).toBeUndefined();
  expect(release?.action).toBe("build");
});

test("says a build stopped rather than showing it running forever", () => {
  // The operator watched a build sit at "building" while nothing was compiling:
  // the reload watcher was not enabled, so the request was never picked up.
  // Reporting progress nobody is making is worse than reporting that.
  const stalled = runtimeUpdates(undefined, undefined, { enabled: true, state: "stalled", reload_available: true, source_revision: "b9cd807", deployed_source_revision: "3c1c508", source_dirty: false } as never, []);
  const build = stalled.find((update) => update.kind === "failed");
  expect(build?.label).toBe("Build stopped responding");
  expect(build?.busy).toBe(false);
  expect(build?.action).toBe("build");
});

test("names a development mode pointing at somewhere that does not exist", () => {
  // After a migration the reload paths pointed into a directory that had moved,
  // so every request went somewhere nobody was watching while the API reported
  // calm. It offers no build action, because building is not what would fix it.
  const broken = runtimeUpdates(undefined, undefined, { enabled: true, state: "unavailable", reload_available: false, source_revision: null, deployed_source_revision: null, source_dirty: false } as never, []);
  const misconfigured = broken.find((update) => update.kind === "failed");
  expect(misconfigured?.label).toBe("Development mode is misconfigured");
  expect(misconfigured?.action).toBeUndefined();
});

/**
 * The operator, after a reload that compiled perfectly and was refused at
 * install: "it is taking a lot of work to find errors."
 *
 * Every failure reached them as "the working copy did not compile" — a
 * confident claim about a cause nobody observed, which sent them to the wrong
 * file. The real message existed in the journal and nowhere else.
 */
test("an install that was refused is not reported as a compile error", () => {
  const [status] = runtimeUpdates(undefined, undefined, {
    enabled: true,
    version: "0.1.0",
    state: "failed",
    reload_available: true,
    deployed_source_revision: "aaaaaaaaaaaa",
    source_revision: "bbbbbbbbbbbb",
    source_dirty: false,
    deployed_source_published: true,
    failure_reason: "install",
    failure_detail: "swarm-package: this release speaks terminal-host protocol 10 and the installed host speaks 9",
  });
  expect(status.detail).toContain("compiled, but could not be installed");
  expect(status.detail).toContain("protocol 10 and the installed host speaks 9");
  expect(status.detail).not.toContain("did not compile.");
});

test("a real compile failure still says so", () => {
  const [status] = runtimeUpdates(undefined, undefined, {
    enabled: true,
    version: "0.1.0",
    state: "failed",
    reload_available: true,
    deployed_source_revision: "aaaaaaaaaaaa",
    source_revision: "bbbbbbbbbbbb",
    source_dirty: false,
    deployed_source_published: true,
    failure_reason: "build",
    failure_detail: "error[E0308]: mismatched types",
  });
  expect(status.detail).toContain("did not compile");
  expect(status.detail).toContain("E0308");
});

test("a failure that recorded nothing does not invent a cause", () => {
  const [status] = runtimeUpdates(undefined, undefined, {
    enabled: true,
    version: "0.1.0",
    state: "failed",
    reload_available: true,
    deployed_source_revision: "aaaaaaaaaaaa",
    source_revision: "bbbbbbbbbbbb",
    source_dirty: false,
    deployed_source_published: true,
  });
  expect(status.detail).toContain("did not record why");
  expect(status.detail).not.toContain("did not compile");
});

/**
 * The operator, 2026-08-28: "it should allow it to be forced with the same (or
 * a similar prompt) that we get with the worker engine update."
 *
 * A prepared migration applies itself within two minutes of the workers going
 * idle. This is the card that says so and offers not to wait.
 */
test("a prepared protocol migration is offered with the worker engine's prompt", () => {
  const [status] = runtimeUpdates(
    { status: "ok", version: "0.8.20", protocol_migration_pending: 10 },
    { protocol_version: 9, host_version: "0.8.19", draining: false, running_sessions: 3, retained_sessions: 3 },
    undefined,
  );
  expect(status.actionLabel).toBe("Apply the protocol migration");
  expect(status.action).toBe("apply_worker_engine");
  // The same consequence, because it costs the same thing.
  expect(status.consequence).toContain("Every loaded worker is stopped and brought back");
  expect(status.detail).toContain("3 running workers");
  expect(status.detail).toContain("apply it now");
});

/** Nothing is offered once the host already speaks the new protocol. */
test("a completed protocol migration stops being offered", () => {
  const updates = runtimeUpdates(
    { status: "ok", version: "0.8.20", protocol_migration_pending: 10 },
    { protocol_version: 10, host_version: "0.8.20", draining: false, running_sessions: 3, retained_sessions: 3 },
    undefined,
  );
  expect(updates.map((update) => update.actionLabel)).not.toContain("Apply the protocol migration");
});

/** A build that does not report the field must not produce a phantom card. */
test("a Hive with no pending migration is unaffected", () => {
  const updates = runtimeUpdates(
    { status: "ok", version: "0.8.20" },
    { protocol_version: 9, host_version: "0.8.19", draining: false, running_sessions: 1, retained_sessions: 1 },
    undefined,
  );
  expect(updates.map((update) => update.actionLabel)).not.toContain("Apply the protocol migration");
});

/**
 * A failure that recorded its cause must not claim it did not.
 *
 * The operator hit this the moment a new failure step existed and this switch
 * had not been told about it: the card read "The development reload failed and
 * did not record why... It said: the working copy changed while this build was
 * running". Both halves in one sentence, contradicting each other.
 *
 * The specific case is now named. The general case matters more — the next step
 * added will not be in this switch either, and it should not make the interface
 * lie in the meantime.
 */
test("names a moved checkout, and never denies a cause it is about to print", () => {
  const failed = (reason: string, detail?: string) =>
    runtimeUpdates(undefined, undefined, {
      enabled: true,
      version: "0.1.0",
      state: "failed",
      reload_available: true,
      deployed_source_revision: "aaaaaaaaaaaa",
      source_revision: "bbbbbbbbbbbb",
      source_dirty: false,
      deployed_source_published: true,
      failure_reason: reason,
      failure_detail: detail,
    })[0];

  const moved = failed("source-moved", "the working copy changed while this build was running");
  expect(moved.detail).toContain("checkout changed while the build was running");
  expect(moved.detail).toContain("nothing was installed");
  expect(moved.detail).not.toContain("did not record why");

  // A step nobody has taught this about is still a step that recorded a cause.
  const unknownWithCause = failed("something-nobody-has-added-yet", "the disk filled up");
  expect(unknownWithCause.detail).toContain("the disk filled up");
  expect(unknownWithCause.detail).not.toContain("did not record why");

  // And when there genuinely is no cause, saying so is still right.
  expect(failed("something-nobody-has-added-yet").detail).toContain("did not record why");
});
