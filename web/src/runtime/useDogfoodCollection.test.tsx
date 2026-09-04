import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { recordBrowserEvidence } from "../api";
import { browserPerformance } from "./browserPerformance";
import { useDogfoodCollection } from "./useDogfoodCollection";

vi.mock("../api", () => ({ recordBrowserEvidence: vi.fn() }));
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); vi.clearAllMocks(); });

test("only stamped development builds collect and unmount releases ownership", async () => {
  vi.useFakeTimers();
  vi.mocked(recordBrowserEvidence).mockResolvedValue({ updated: true, pruned: 0 });
  const { rerender, unmount } = renderHook(({ enabled }) => useDogfoodCollection("token", enabled, "build"), { initialProps: { enabled: false } });
  await act(async () => { browserPerformance.record("route", 10); await vi.advanceTimersByTimeAsync(60_000); });
  expect(recordBrowserEvidence).not.toHaveBeenCalled();
  rerender({ enabled: true });
  await act(async () => { browserPerformance.record("route", 10); await vi.advanceTimersByTimeAsync(60_000); });
  expect(recordBrowserEvidence).toHaveBeenCalledTimes(1);
  expect(vi.mocked(recordBrowserEvidence).mock.calls[0][1].build).toBe("build");
  unmount();
  const release = browserPerformance.attachHourlySink(() => undefined);
  release();
});

test("failed upload retries the same capture and reports recovery", async () => {
  vi.useFakeTimers();
  vi.mocked(recordBrowserEvidence).mockRejectedValueOnce(new Error("offline"))
    .mockResolvedValue({ updated: true, pruned: 2 });
  const { result, unmount } = renderHook(() => useDogfoodCollection("token", true, "build"));
  await act(async () => { browserPerformance.record("route", 10); await vi.advanceTimersByTimeAsync(100); });
  expect(result.current.state).toBe("unavailable");
  await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
  expect(result.current.state).toBe("collecting");
  expect(result.current.pruned_captures).toBe(2);
  const calls = vi.mocked(recordBrowserEvidence).mock.calls;
  expect(calls[1][1]).toEqual(calls[0][1]);
  unmount();
});

test("unmount aborts upload and prevents late acknowledgement", async () => {
  vi.useFakeTimers();
  let resolve!: (value: { updated: boolean; pruned: number }) => void;
  vi.mocked(recordBrowserEvidence).mockReturnValue(new Promise((done) => { resolve = done; }));
  const { unmount } = renderHook(() => useDogfoodCollection("token", true, "build"));
  await act(async () => { browserPerformance.record("route", 10); await vi.advanceTimersByTimeAsync(100); });
  const signal = vi.mocked(recordBrowserEvidence).mock.calls[0][2];
  unmount();
  expect(signal.aborted).toBe(true);
  await act(async () => { resolve({ updated: true, pruned: 0 }); });
});
