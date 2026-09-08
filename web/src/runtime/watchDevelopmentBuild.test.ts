import { afterEach, expect, test, vi } from "vitest";
import { watchDevelopmentBuild } from "./watchDevelopmentBuild";

afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });
const ok = (body: unknown) => ({ ok: true, status: 200, json: async () => body });
function fixture() {
  vi.useFakeTimers();
  const owner = new AbortController();
  const fetch = vi.fn((input: RequestInfo | URL) => Promise.resolve(ok(String(input) === "/health"
    ? { version: "new" } : { state: "ready" })));
  vi.stubGlobal("fetch", fetch);
  return { owner, fetch };
}

test("a healthy different version completes one watcher and clears its timers", async () => {
  const { owner, fetch } = fixture();
  const result = watchDevelopmentBuild("token", "old", owner.signal);
  await vi.advanceTimersByTimeAsync(2_000);
  expect(await result).toEqual({ kind: "changed", version: "new" });
  expect(fetch).toHaveBeenCalledTimes(2);
  expect(vi.getTimerCount()).toBe(0);
});

test("cancellation during the delay never issues a request", async () => {
  const { owner, fetch } = fixture();
  const result = watchDevelopmentBuild("token", "old", owner.signal);
  owner.abort();
  expect(await result).toEqual({ kind: "cancelled" });
  expect(fetch).not.toHaveBeenCalled();
  expect(vi.getTimerCount()).toBe(0);
});

test("late responses after cancellation cannot report a version change", async () => {
  const { owner, fetch } = fixture();
  const replies: ((value: ReturnType<typeof ok>) => void)[] = [];
  fetch.mockImplementation(() => new Promise((resolve) => { replies.push(resolve); }));
  const result = watchDevelopmentBuild("token", "old", owner.signal);
  await vi.advanceTimersByTimeAsync(2_000);
  owner.abort();
  for (const reply of replies) reply(ok({ version: "new", state: "ready" }));
  expect(replies).toHaveLength(2);
  expect(await result).toEqual({ kind: "cancelled" });
  expect(vi.getTimerCount()).toBe(0);
});

test("request deadlines recover without parallel polling", async () => {
  const { owner, fetch } = fixture();
  fetch.mockImplementationOnce((_input, init?: RequestInit) => new Promise((_resolve, reject) => {
    init?.signal?.addEventListener("abort", () => reject(new DOMException("Timeout", "AbortError")));
  }));
  const result = watchDevelopmentBuild("token", "old", owner.signal);
  await vi.advanceTimersByTimeAsync(9_999);
  expect(fetch).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(2_001);
  expect(await result).toEqual({ kind: "changed", version: "new" });
  expect(fetch).toHaveBeenCalledTimes(4);
  expect(vi.getTimerCount()).toBe(0);
});

test("a recorded install failure is returned without claiming a compilation failure", async () => {
  const { owner, fetch } = fixture();
  fetch.mockImplementation((input) => Promise.resolve(ok(String(input) === "/health" ? { version: "old" }
    : { state: "failed", failure_reason: "install", failure_detail: "not enough space" })));
  const result = watchDevelopmentBuild("token", "old", owner.signal);
  await vi.advanceTimersByTimeAsync(2_000);
  expect(await result).toMatchObject({ kind: "failed", runtime: { failure_reason: "install" } });
});

test("an unchanged build is observed for at most twenty minutes", async () => {
  const { owner, fetch } = fixture();
  fetch.mockImplementation((input) => Promise.resolve(ok(String(input) === "/health" ? { version: "old" } : { state: "building" })));
  const result = watchDevelopmentBuild("token", "old", owner.signal);
  await vi.advanceTimersByTimeAsync(20 * 60_000);
  expect(await result).toEqual({ kind: "timeout" });
  expect(vi.getTimerCount()).toBe(0);
});
