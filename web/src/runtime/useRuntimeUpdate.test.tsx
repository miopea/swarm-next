import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { useRuntimeUpdate } from "./useRuntimeUpdate";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test("keeps development navigation through an API interruption and resets on logout", async () => {
  stubRuntime(() => false);
  const { result, rerender } = renderHook(({ token }: { token: string | undefined }) => useRuntimeUpdate(token), { initialProps: { token: "secret" as string | undefined } });
  await act(async () => { await Promise.resolve(); });
  expect(result.current.developmentMode).toBe(true);
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("API restarting")));
  await act(async () => { await result.current.refreshRuntimeUpdate(); });
  expect(result.current.developmentMode).toBe(true);
  rerender({ token: undefined });
  expect(result.current.developmentMode).toBe(false);
});

function ok(payload: unknown) {
  return { ok: true, status: 200, json: async () => payload };
}

function stubRuntime(reloadAvailable: () => boolean) {
  vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (url === "/health") return Promise.resolve(ok({ status: "ok", version: "0.1.0", worker_engine_build_id: "same" }));
    if (url.includes("terminal-host")) return Promise.resolve(ok({ type: "host_status", status: { host_version: "0.1.0", host_build_id: "same", draining: false } }));
    return Promise.resolve(ok({
      enabled: true, version: "0.1.0", state: "idle",
      reload_available: reloadAvailable(), source_revision: "48f27e0abcdef", source_dirty: false,
    }));
  }));
}

test("reports a waiting update without being asked to refresh", async () => {
  // The indicator was set only as a side effect of refreshing the control
  // room, which nothing does on load or on a timer. An App and API update sat
  // available in Settings while the header said nothing at all.
  stubRuntime(() => true);

  const { result } = renderHook(() => useRuntimeUpdate("secret"));
  await act(async () => { await Promise.resolve(); });

  expect(result.current.runtimeUpdates.map((entry) => entry.kind)).toEqual(["app"]);
  expect(result.current.runtimeUpdates[0].label).toContain("App and API");
});

test("notices an update that appears while the operator is looking elsewhere", async () => {
  vi.useFakeTimers();
  let available = false;
  stubRuntime(() => available);

  const { result } = renderHook(() => useRuntimeUpdate("secret", 15_000));
  await act(async () => { await Promise.resolve(); });
  expect(result.current.runtimeUpdates).toEqual([]);

  available = true;
  await act(async () => { await vi.advanceTimersByTimeAsync(15_000); });

  expect(result.current.runtimeUpdates.map((entry) => entry.kind)).toEqual(["app"]);
});

test("says nothing at all before the operator is authenticated", async () => {
  stubRuntime(() => true);

  const { result } = renderHook(() => useRuntimeUpdate(undefined));
  await act(async () => { await Promise.resolve(); });

  expect(result.current.runtimeUpdates).toEqual([]);
});

test("a failed provider check preserves an honest notice until recovery confirms its state", async () => {
  let providerState: "pending" | "unavailable" | "current" = "pending";
  stubRuntime(() => true);
  const normalFetch = globalThis.fetch;
  vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    if (String(input).includes("providers")) {
      if (providerState === "unavailable") return Promise.reject(new Error("provider check failed"));
      return Promise.resolve(ok({ superseded: providerState === "pending"
        ? [{ provider: "claude_code", version: "2.1.0", installed_at: null, worker_ids: ["demo"] }]
        : [] }));
    }
    return normalFetch(input, init);
  }));
  const { result } = renderHook(() => useRuntimeUpdate("secret"));
  await act(async () => { await Promise.resolve(); });
  expect(result.current.runtimeUpdates[0].action).toBe("restart_providers");

  providerState = "unavailable";
  await act(async () => { await result.current.refreshRuntimeUpdate(); });
  expect(result.current.runtimeUpdates.map((entry) => entry.kind)).toEqual(["provider", "app"]);
  expect(result.current.runtimeUpdates[0].label).toContain("unavailable");
  expect(result.current.runtimeUpdates[0].action).toBeUndefined();
  const unavailable = result.current.runtimeUpdates;
  await act(async () => { await result.current.refreshRuntimeUpdate(); });
  expect(result.current.runtimeUpdates).toEqual(unavailable);

  providerState = "pending";
  await act(async () => { await result.current.refreshRuntimeUpdate(); });
  expect(result.current.runtimeUpdates[0].action).toBe("restart_providers");
  providerState = "current";
  await act(async () => { await result.current.refreshRuntimeUpdate(); });
  expect(result.current.runtimeUpdates.map((entry) => entry.kind)).toEqual(["app"]);
});
