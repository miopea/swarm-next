import { afterEach, expect, test, vi } from "vitest";

import {
  measureRoutePaint,
  readRoutePaints,
  recordRoutePaint,
  routePaintSummary,
} from "./routePaint";

afterEach(() => { window.sessionStorage.clear(); vi.restoreAllMocks(); });

test("keeps a bounded window of recent route paints", () => {
  for (let index = 0; index < 25; index += 1) recordRoutePaint("tasks", index, 1000 + index);

  const samples = readRoutePaints();

  expect(samples).toHaveLength(20);
  // The oldest fall out, so a long session cannot grow this without limit.
  expect(samples[0].duration_ms).toBe(5);
  expect(samples.at(-1)?.duration_ms).toBe(24);
});

test("reports the worst alongside the middle, because the complaint is the slow one", () => {
  [40, 42, 38, 1200, 41].forEach((duration) => recordRoutePaint("workers", duration));

  const summary = routePaintSummary(readRoutePaints());

  expect(summary).toEqual({ samples: 5, slowest_ms: 1200, median_ms: 41 });
});

test("says nothing before anything has been measured", () => {
  expect(routePaintSummary([])).toBeUndefined();
  expect(readRoutePaints()).toEqual([]);
});

test("ignores stored values it cannot trust", () => {
  window.sessionStorage.setItem("swarm-next.route-paint.v1", JSON.stringify([{ surface: 7 }, "nonsense"]));

  expect(readRoutePaints()).toEqual([]);
});

test("measures to the frame after the browser painted the new surface", () => {
  const frames: Array<() => void> = [];
  const schedule = vi.fn((callback: () => void) => frames.push(callback));
  const record = vi.fn();
  let now = 100;

  measureRoutePaint("workers", schedule, vi.fn(), () => now, record);

  // One frame runs before that frame is painted, so nothing is recorded yet.
  now = 116;
  frames[0]();
  expect(record).not.toHaveBeenCalled();

  now = 132;
  frames[1]();
  expect(record).toHaveBeenCalledWith("workers", 32);
});

test("records nothing for a route abandoned before it painted", () => {
  const handles: Array<() => void> = [];
  const schedule = vi.fn((callback: () => void) => handles.push(callback));
  const cancel = vi.fn();
  const record = vi.fn();

  const abandon = measureRoutePaint("tasks", schedule, cancel, () => 0, record);
  abandon();

  expect(cancel).toHaveBeenCalled();
  expect(record).not.toHaveBeenCalled();
});

test("does not start a route sample while hidden", () => {
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
  const schedule = vi.fn();
  const record = vi.fn();
  measureRoutePaint("tasks", schedule, vi.fn(), () => 0, record)();
  expect(schedule).not.toHaveBeenCalled();
  expect(record).not.toHaveBeenCalled();
});

test.each([false, true])("a hidden interval cancels the sample, even after returning (first frame: %s)", (firstFrame) => {
  const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  const frames: Array<() => void> = [];
  const record = vi.fn();
  const cancel = vi.fn();
  const remove = vi.spyOn(document, "removeEventListener");
  let now = 100;
  const stop = measureRoutePaint("workers", (callback) => frames.push(callback), cancel, () => now, record);
  if (firstFrame) frames[0]();
  visibility.mockReturnValue("hidden");
  document.dispatchEvent(new Event("visibilitychange"));
  visibility.mockReturnValue("visible");
  now += 60_000;
  // Even a callback already queued before cancellation must not publish.
  frames.at(-1)!();
  stop();
  expect(record).not.toHaveBeenCalled();
  expect(cancel).toHaveBeenCalledTimes(firstFrame ? 2 : 1);
  expect(remove).toHaveBeenCalledTimes(1);
});

test("completion removes its visibility listener and cannot publish twice", () => {
  const frames: Array<() => void> = [];
  const record = vi.fn();
  const remove = vi.spyOn(document, "removeEventListener");
  const stop = measureRoutePaint("tasks", (callback) => frames.push(callback), vi.fn(), () => 10, record);
  frames[0]();
  frames[1]();
  frames[1]();
  stop();
  expect(record).toHaveBeenCalledTimes(1);
  expect(remove).toHaveBeenCalledTimes(1);
});
