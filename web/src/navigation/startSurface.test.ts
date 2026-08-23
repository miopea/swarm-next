import { afterEach, beforeEach, expect, test } from "vitest";

import { readSavedSurface, saveSurface, surfaceWasRequested } from "./startSurface";

beforeEach(() => {
  window.sessionStorage.clear();
  window.history.replaceState(null, "", "/");
});
afterEach(() => window.sessionStorage.clear());

/**
 * "My default page is set to workers and it always comes back to tasks."
 *
 * The remembered surface is written on every navigation, so treating it as a
 * request meant the configured opening screen applied once on a genuinely fresh
 * tab and never again. An installed PWA is one long-lived tab.
 */
test("a surface remembered from earlier in this tab is not a request", () => {
  saveSurface("tasks");

  expect(surfaceWasRequested("")).toBe(false);
});

test("a linked surface is a request and wins over the configured default", () => {
  expect(surfaceWasRequested("?surface=decisions")).toBe(true);
  expect(readSavedSurface("?surface=decisions")).toBe("decisions");
});

test("a Jira hand-off is a request", () => {
  expect(surfaceWasRequested("?jira=SWARM-1")).toBe(true);
});

test("the remembered surface still renders first, so there is no flash", () => {
  saveSurface("workers");

  expect(readSavedSurface("")).toBe("workers");
});

test("an unknown remembered value falls back to the board", () => {
  window.sessionStorage.setItem("swarm-next.surface.v1", "nonsense");

  expect(readSavedSurface("")).toBe("tasks");
});
