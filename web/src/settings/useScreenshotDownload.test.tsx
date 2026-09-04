import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { DogfoodReport } from "../api";
import { useScreenshotDownload } from "./useScreenshotDownload";

const report: DogfoodReport = { id: "report-1", expectation: "", observation: "", diagnostic_bundle: "{}", attachment_name: "example.png", created_at: 1 };
afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

test("bounds downloads, prevents overlap, and recovers after timeout", async () => {
  vi.useFakeTimers();
  const signals: AbortSignal[] = [];
  vi.stubGlobal("fetch", vi.fn((_input: RequestInfo | URL, init?: RequestInit) => new Promise<Response>((_resolve, reject) => {
    const signal = init!.signal as AbortSignal;
    signals.push(signal);
    signal.addEventListener("abort", () => reject(signal.reason), { once: true });
  })));
  const { result, unmount } = renderHook(() => useScreenshotDownload("test-token"));
  act(() => { void result.current.download(report); void result.current.download({ ...report, id: "report-2" }); });
  expect(signals).toHaveLength(1);
  expect(result.current.downloadingReportId).toBe(report.id);
  await act(async () => { await vi.advanceTimersByTimeAsync(30_000); });
  expect(signals[0].aborted).toBe(true);
  expect(result.current.failure?.message).toBe("Screenshot download timed out. Try again.");
  expect(result.current.downloadingReportId).toBeUndefined();
  act(() => { void result.current.download(report); });
  expect(signals).toHaveLength(2);
  expect(result.current.failure).toBeUndefined();
  unmount();
  expect(signals[1].aborted).toBe(true);
  expect(vi.getTimerCount()).toBe(0);
});

test("departed or replaced views cannot trigger a late browser download", async () => {
  const requests: { signal: AbortSignal; resolve: (response: Response) => void }[] = [];
  vi.stubGlobal("fetch", vi.fn((_input: RequestInfo | URL, init?: RequestInit) => new Promise<Response>((resolve) => {
    requests.push({ signal: init!.signal as AbortSignal, resolve });
  })));
  const create = vi.fn(() => "blob:sample");
  const revoke = vi.fn();
  vi.stubGlobal("URL", Object.assign(class extends URL {}, { createObjectURL: create, revokeObjectURL: revoke }));
  const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
  const { result, rerender, unmount } = renderHook(({ token }) => useScreenshotDownload(token), { initialProps: { token: "first" } });
  act(() => { void result.current.download(report); });
  rerender({ token: "second" });
  expect(requests[0].signal.aborted).toBe(true);
  act(() => { void result.current.download(report); });
  await act(async () => { requests[0].resolve(new Response("old")); });
  expect(create).not.toHaveBeenCalled();
  expect(result.current.downloadingReportId).toBe(report.id);
  await act(async () => { requests[1].resolve(new Response("new")); });
  expect(click).toHaveBeenCalledTimes(1);
  expect(revoke).toHaveBeenCalledWith("blob:sample");
  act(() => { void result.current.download(report); });
  unmount();
  await act(async () => { requests[2].resolve(new Response("late")); });
  expect(click).toHaveBeenCalledTimes(1);
});

test("transfer failure does not claim the stored screenshot was deleted", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => new Response("unavailable", { status: 503 })));
  const { result } = renderHook(() => useScreenshotDownload("test-token"));
  await act(async () => { await result.current.download(report); });
  expect(result.current.failure).toEqual({ reportId: report.id, message: "Screenshot could not be downloaded. Check your connection and try again." });
  expect(result.current.downloadingReportId).toBeUndefined();
});
