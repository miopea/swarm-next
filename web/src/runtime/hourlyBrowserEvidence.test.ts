import { expect, test } from "vitest";
import { HourlyBrowserEvidence } from "./hourlyBrowserEvidence";
import { BrowserPerformanceRecorder } from "./browserPerformance";

test("hourly totals survive rolling local-window expiry and retries preserve identity", () => {
  const collector = new HourlyBrowserEvidence("1.4.1-dev-test", () => "id");
  collector.record("route", 10, 3_600_000);
  const first = collector.next(3_600_000)!;
  expect(collector.next(3_600_000)).toEqual(first);
  collector.record("route", 20, 3_601_000);
  collector.acknowledge(first);
  const next = collector.next(3_601_000)!;
  expect(next.capture_id).toBe(first.capture_id);
  expect(next.revision).toBeGreaterThan(first.revision);
  expect(next.route).toEqual({ count: 2, total_ms: 30, max_ms: 20 });
  collector.acknowledge(next);
  collector.acknowledge(first);
  expect(collector.next(3_602_000)).toBeUndefined();
});

test("hours and acknowledgement state are bounded and expired loss is visible", () => {
  let id = 0;
  const collector = new HourlyBrowserEvidence("build", () => String(++id));
  for (let hour = 1; hour <= 25; hour++) collector.record("route", 1, hour * 3_600_000);
  expect(collector.status).toEqual({ retained: 24, dropped_samples: 1 });
  expect(collector.next(50 * 3_600_000)).toBeUndefined();
  expect(collector.status).toEqual({ retained: 0, dropped_samples: 25 });
});

test("backwards clocks and invalid measurements are omitted and counted", () => {
  const collector = new HourlyBrowserEvidence("build", () => "id");
  collector.record("route", 10, 5000);
  collector.record("route", 10, 4999);
  collector.record("route", Infinity, 5000);
  expect(collector.status.dropped_samples).toBe(2);
  expect(collector.next(5000)?.route.count).toBe(1);
});

test("recorder has one detachable numeric evidence owner", () => {
  const recorder = new BrowserPerformanceRecorder(() => 3_600_000);
  const collector = new HourlyBrowserEvidence("build", () => "id");
  const detach = recorder.attachHourlySink((metric, duration, at) => collector.record(metric, duration, at));
  expect(() => recorder.attachHourlySink(() => undefined)).toThrow();
  recorder.record("interaction", 12.3);
  detach();
  recorder.record("interaction", 20);
  expect(collector.next(3_600_000)?.interaction).toEqual({ count: 1, total_ms: 12, max_ms: 12 });
});

test("unavailable capture identity does not interrupt terminal instrumentation", () => {
  const collector = new HourlyBrowserEvidence("build", () => { throw new Error("Unavailable"); });
  expect(() => collector.record("terminal_render", 10, 1000)).not.toThrow();
  expect(collector.status).toEqual({ retained: 0, dropped_samples: 1 });
});
