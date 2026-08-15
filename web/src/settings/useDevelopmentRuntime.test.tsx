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
  })));
  vi.stubGlobal("fetch", fetch);

  const { result } = renderHook(() => useDevelopmentRuntime("secret", "0.1.0-dev-oldrevision"));
  await act(async () => { await Promise.resolve(); });
  expect(result.current?.reload_available).toBe(false);

  reloadAvailable = true;
  await act(async () => { await vi.advanceTimersByTimeAsync(15_000); });
  expect(result.current?.reload_available).toBe(true);
  expect(fetch).toHaveBeenCalledTimes(2);
});

function ok(payload: unknown) {
  return { ok: true, status: 200, json: async () => payload };
}
