import { afterEach, expect, test, vi } from "vitest";
import { BrowserPerformanceRecorder, installBrowserPerformanceCapture, readBrowserPerformance, readPreviousBrowserPerformance, saveBrowserPerformance } from "./browserPerformance";

afterEach(() => { vi.unstubAllGlobals(); window.sessionStorage.clear(); });

test("high event volume is aggregated and bounded by count and age", () => {
  let now = 1_000_000;
  const recorder = new BrowserPerformanceRecorder(() => now);
  for (let i = 0; i < 1000; i++) recorder.record("interaction", 120);
  expect(recorder.snapshot().buckets).toHaveLength(1);
  expect(recorder.snapshot().buckets[0].metrics.interaction?.count).toBe(1000);
  for (let i = 0; i < 500; i++) { now += 10_000; recorder.record("route", 30); }
  expect(recorder.snapshot().buckets).toHaveLength(360);
  now += 3_600_001;
  expect(recorder.snapshot().buckets).toEqual([]);
});

test("an incident captures before and after, coalesces bursts, and expires", () => {
  let now = 1_000_000;
  const recorder = new BrowserPerformanceRecorder(() => now);
  recorder.record("route", 30);
  now += 10_000;
  recorder.record("terminal_render", 1500);
  now += 10_000;
  recorder.record("long_task", 3500);
  const incident = recorder.snapshot().incidents[0];
  expect(recorder.snapshot().incidents).toHaveLength(1);
  expect(incident.severity).toBe("critical");
  expect(incident.buckets).toHaveLength(3);
  now += 70_000;
  recorder.record("route", 20);
  expect(recorder.snapshot().incidents[0].buckets).toHaveLength(3);
  for (let i = 0; i < 10; i++) { now += 70_000; recorder.record("route", 2000); }
  expect(recorder.snapshot().incidents).toHaveLength(5);
  now += 86_400_001;
  expect(recorder.snapshot().incidents).toEqual([]);
});

test("invalid durations and metric names cannot enter evidence", () => {
  const recorder = new BrowserPerformanceRecorder();
  [NaN, Infinity, -1, 90_000_000].forEach((value) => recorder.record("route", value));
  recorder.record("secret-input" as "route", 100);
  expect(recorder.snapshot().buckets).toEqual([]);
});

test("repeated short main-thread blocks also trigger a bounded capture", () => {
  const recorder = new BrowserPerformanceRecorder(() => 100_000);
  for (let i = 0; i < 6; i++) recorder.record("long_task", 200);
  expect(recorder.snapshot().incidents).toHaveLength(1);
  expect(recorder.snapshot().incidents[0].severity).toBe("slow");
});

test("snapshot mutation cannot corrupt the recorder", () => {
  const recorder = new BrowserPerformanceRecorder();
  recorder.record("route", 1200);
  recorder.snapshot().buckets[0].metrics.route!.count = 999;
  recorder.snapshot().incidents[0].buckets.length = 0;
  expect(recorder.snapshot().buckets[0].metrics.route!.count).toBe(1);
  expect(recorder.snapshot().incidents[0].buckets).toHaveLength(1);
});

test("before-reload evidence survives but expires and strips unknown fields", () => {
  const recorder = new BrowserPerformanceRecorder(() => 100_000);
  recorder.record("route", 1200);
  saveBrowserPerformance(window.sessionStorage, recorder);
  expect(readPreviousBrowserPerformance(window.sessionStorage, 100_100)?.incidents).toHaveLength(1);
  expect(readPreviousBrowserPerformance(window.sessionStorage, 90_000_000)).toBeUndefined();
  const source = { ...recorder.snapshot(), secret: "password", buckets: [{ at: 100_000, metrics: { route: { count: 1, total_ms: 10, max_ms: 10, text: "private" }, private: "prompt" } }] };
  const parsed = readPreviousBrowserPerformance({ getItem: () => JSON.stringify(source) }, 100_100);
  expect(JSON.stringify(parsed)).not.toMatch(/password|private|prompt/);
});

test("storage denial and malformed content never stop diagnostics", () => {
  expect(() => saveBrowserPerformance({ setItem: () => { throw new Error("denied"); } })).not.toThrow();
  expect(readPreviousBrowserPerformance({ getItem: () => { throw new Error("denied"); } })).toBeUndefined();
  expect(readPreviousBrowserPerformance({ getItem: () => "{" })).toBeUndefined();
  expect(readPreviousBrowserPerformance({ getItem: () => "x".repeat(500_001) })).toBeUndefined();
});

test("capture owns and disposes supported observers without polling", () => {
  const disconnect = vi.fn();
  const observe = vi.fn();
  vi.stubGlobal("PerformanceObserver", class {
    static supportedEntryTypes = ["longtask"];
    observe = observe;
    disconnect = disconnect;
  });
  const stop = installBrowserPerformanceCapture();
  expect(readBrowserPerformance().supported_observers).toEqual(["longtask"]);
  expect(observe).toHaveBeenCalledTimes(1);
  const duplicateStop = installBrowserPerformanceCapture();
  duplicateStop();
  expect(disconnect).not.toHaveBeenCalled();
  stop();
  expect(disconnect).toHaveBeenCalledTimes(1);
  expect(readBrowserPerformance().collection).toBe("not_installed");
});

test("unsupported observation is unavailable rather than healthy by assumption", () => {
  vi.stubGlobal("PerformanceObserver", undefined);
  const stop = installBrowserPerformanceCapture();
  expect(readBrowserPerformance().supported_observers).toEqual([]);
  stop();
});
