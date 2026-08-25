import { expect, test, vi, afterEach } from "vitest";

import { bundleIsStale } from "./staleBundle";

afterEach(() => vi.unstubAllEnvs());

/**
 * The case that cost a developer an afternoon: a page still running the bundle
 * it loaded before the Hive was upgraded. The fix he wanted had shipped days
 * earlier and he was still hitting the bug, and nothing could tell him why.
 */
test("a page older than the Hive serving it says so", () => {
  vi.stubEnv("VITE_SWARM_BUILD_VERSION", "0.8.12");

  expect(bundleIsStale("0.8.14")).toBe(true);
});

/**
 * SILENCE IS THE DEFAULT, and these are the cases that must stay silent. A
 * banner that appears when nothing is wrong is worse than the gap it fills:
 * it gets dismissed on reflex, and then it is decoration.
 */
test("it stays silent whenever it cannot be certain", () => {
  vi.stubEnv("VITE_SWARM_BUILD_VERSION", "0.8.14");
  // The overwhelmingly common case.
  expect(bundleIsStale("0.8.14")).toBe(false);
  // Health has not answered yet, or answered badly. Not evidence of anything.
  expect(bundleIsStale(null)).toBe(false);
  expect(bundleIsStale(undefined)).toBe(false);
  expect(bundleIsStale("")).toBe(false);
});

/**
 * Under `vite dev` and in tests there is no baked version, and no release to be
 * behind. Warning there would train every developer to ignore this.
 */
test("a development server is never stale", () => {
  vi.stubEnv("VITE_SWARM_BUILD_VERSION", "");

  expect(bundleIsStale("0.8.14")).toBe(false);
});

/**
 * It compares rather than orders.
 *
 * A page whose version merely DIFFERS from the Hive wants reloading either way
 * — including the case where the page is somehow ahead, which happens during a
 * rolling restart. Deciding which of two version strings is newer is a guess,
 * and this does not need to make it.
 */
test("a page ahead of the Hive is still worth reloading", () => {
  vi.stubEnv("VITE_SWARM_BUILD_VERSION", "0.8.14");

  expect(bundleIsStale("0.8.13")).toBe(true);
});
