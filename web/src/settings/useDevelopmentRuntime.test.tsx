import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { useDevelopmentRuntime } from "./useDevelopmentRuntime";

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test("detects a newly pulled working-copy revision while Settings remains open", async () => {
  vi.useFakeTimers();
  let reloadAvailable = false;
  const fetch = vi.fn(() => Promise.resolve(ok({
    enabled: true,
    version: "0.1.0-dev-oldrevision",
    state: "idle",
    reload_available: reloadAvailable,
    source_revision: reloadAvailable ? "newrevision123" : "oldrevision123",
    source_dirty: false,
    deployed_source_published: true,
  })));
  vi.stubGlobal("fetch", fetch);

  const { result } = renderHook(() => useDevelopmentRuntime("secret", "0.1.0-dev-oldrevision"));
  await act(async () => { await Promise.resolve(); });
  expect(result.current.runtime?.reload_available).toBe(false);

  reloadAvailable = true;
  await act(async () => { await vi.advanceTimersByTimeAsync(15_000); });
  expect(result.current.runtime?.reload_available).toBe(true);
  expect(fetch).toHaveBeenCalledTimes(2);
});

test("keeps what it knew while the API is restarting under the reload", async () => {
  // Activating a build restarts the API, so the one moment this call reliably
  // fails is the middle of the operation the operator is watching. Forgetting
  // the runtime there took the App and API card off the page with it.
  vi.useFakeTimers();
  let reachable = true;
  const fetch = vi.fn(() => reachable
    ? Promise.resolve(ok({
      enabled: true,
      version: "0.1.0-dev-oldrevision",
      state: "building",
      reload_available: false,
      source_revision: "newrevision123",
      source_dirty: false,
      deployed_source_published: true,
    }))
    : Promise.reject(new Error("Runtime request returned 502")));
  vi.stubGlobal("fetch", fetch);

  const { result } = renderHook(() => useDevelopmentRuntime("secret", "0.1.0-dev-oldrevision"));
  await act(async () => { await Promise.resolve(); });
  expect(result.current.runtime?.enabled).toBe(true);

  reachable = false;
  await act(async () => { await vi.advanceTimersByTimeAsync(15_000); });

  expect(result.current.reachable).toBe(false);
  expect(result.current.runtime?.enabled).toBe(true);
  expect(result.current.runtime?.state).toBe("building");
});

function ok(payload: unknown) {
  return { ok: true, status: 200, json: async () => payload };
}
