import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { useVisiblePolling } from "./useVisiblePolling";

afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); });

test("deduplicates refreshes and cancels a stalled request at its deadline", async () => {
  vi.useFakeTimers();
  const signals: AbortSignal[] = [];
  const task = vi.fn((signal: AbortSignal) => new Promise<void>((resolve) => {
    signals.push(signal);
    signal.addEventListener("abort", () => resolve(), { once: true });
  }));
  const { result, unmount } = renderHook(() => useVisiblePolling(task, true, 1_000, 3_000));
  await act(async () => { await Promise.resolve(); });
  void result.current();
  await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
  expect(task).toHaveBeenCalledTimes(1);
  await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
  expect(signals[0].aborted).toBe(true);
  expect(signals[0].reason.name).toBe("TimeoutError");
  expect(task.mock.calls.length).toBeGreaterThan(1);
  unmount();
  expect(signals.at(-1)!.aborted).toBe(true);
});

test("hidden pages do no polling and returning resumes it", async () => {
  vi.useFakeTimers();
  let visibility: DocumentVisibilityState = "hidden";
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => visibility);
  const task = vi.fn(async (_signal: AbortSignal) => undefined);
  const { result, unmount } = renderHook(() => useVisiblePolling(task, true, 1_000));
  await act(async () => { await result.current(); await vi.advanceTimersByTimeAsync(5_000); });
  expect(task).not.toHaveBeenCalled();
  visibility = "visible";
  await act(async () => { document.dispatchEvent(new Event("visibilitychange")); });
  expect(task).toHaveBeenCalledTimes(1);
  unmount();
  await act(async () => { document.dispatchEvent(new Event("visibilitychange")); await vi.advanceTimersByTimeAsync(5_000); });
  expect(task).toHaveBeenCalledTimes(1);
});

test("replaces ownership on task changes and never starts a disposed microtask", async () => {
  const signals: AbortSignal[] = [];
  const first = vi.fn(async (signal: AbortSignal) => { signals.push(signal); });
  const second = vi.fn(async (_signal: AbortSignal) => undefined);
  const { rerender, unmount } = renderHook(({ task }) => useVisiblePolling(task, true, 15_000), { initialProps: { task: first } });
  await act(async () => { await Promise.resolve(); });
  rerender({ task: second });
  unmount();
  await act(async () => { await Promise.resolve(); });
  expect(second).not.toHaveBeenCalled();
});

test("a failed request does not stop later polling", async () => {
  vi.useFakeTimers();
  const task = vi.fn().mockRejectedValueOnce(new Error("offline")).mockResolvedValue(undefined);
  renderHook(() => useVisiblePolling(task, true, 1_000));
  await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
  expect(task).toHaveBeenCalledTimes(3);
});

test("a rapid hide/show waits for cancellation then refreshes without a polling delay", async () => {
  let visibility: DocumentVisibilityState = "visible";
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => visibility);
  let finish!: () => void;
  const signals: AbortSignal[] = [];
  const task = vi.fn((signal: AbortSignal) => {
    signals.push(signal);
    return new Promise<void>((resolve) => { finish = resolve; });
  });
  renderHook(() => useVisiblePolling(task, true, 15_000));
  await act(async () => { await Promise.resolve(); });
  visibility = "hidden";
  document.dispatchEvent(new Event("visibilitychange"));
  expect(signals[0].aborted).toBe(true);
  visibility = "visible";
  document.dispatchEvent(new Event("visibilitychange"));
  expect(task).toHaveBeenCalledTimes(1);
  await act(async () => { finish(); });
  expect(task).toHaveBeenCalledTimes(2);
});
