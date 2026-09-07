import { expect, test } from "vitest";
import { RecentInteractions } from "./recentInteractions";

const event = (interactionId = 1, duration = 200) => ({ interactionId, duration, startTime: 100, processingStart: 120, processingEnd: 150 });

test("groups entries across callbacks and retains phases from the slowest entry", () => {
  const capture = new RecentInteractions(() => 1000);
  capture.record(event());
  capture.record({ ...event(1, 300), processingStart: 140, processingEnd: 180 });
  capture.record({ ...event(1, 150), processingStart: 200, processingEnd: 240 });
  expect(capture.snapshot()).toMatchObject({ observed_interactions: 1, slowest: {
    duration_ms: 300, input_delay_ms: 40, processing_ms: 40, presentation_estimate_ms: 220,
  } });
  capture.record(event(2));
  expect(capture.snapshot().observed_interactions).toBe(2);
});

test("unattributed native entries explain raw timing without becoming interactions", () => {
  let now = 1000;
  const capture = new RecentInteractions(() => now);
  capture.record(event(0, 1200));
  capture.record({ ...event(0, 1100), processingStart: 500, processingEnd: 600 });
  const snapshot = capture.snapshot();
  expect(snapshot).toMatchObject({ observed_interactions: 0, slowest: null,
    unattributed_event_entries: 2, slowest_unattributed: {
      duration_ms: 1200, input_delay_ms: 20, processing_ms: 30, presentation_estimate_ms: 1150,
    } });
  snapshot.slowest_unattributed!.duration_ms = 5;
  expect(capture.snapshot().slowest_unattributed!.duration_ms).toBe(1200);
  for (let i = 0; i < 500; i++) capture.record(event(0, 200));
  expect(capture.snapshot()).toMatchObject({ unattributed_event_entries: 200,
    slowest_unattributed: { duration_ms: 200 }, observed_interactions: 0 });
  capture.record(event(99, 300));
  expect(capture.snapshot().observed_interactions).toBe(1);
  now += 60_001;
  expect(capture.snapshot()).toMatchObject({ unattributed_event_entries: 0, slowest_unattributed: null });
  capture.record({ ...event(), interactionId: undefined });
  expect(capture.snapshot().unattributed_event_entries).toBe(1);
  now--;
  expect(capture.snapshot().unattributed_event_entries).toBe(0);
});

test("unidentified and malformed entries never become guessed interactions", () => {
  const capture = new RecentInteractions();
  for (const invalid of [
    { ...event(), interactionId: undefined }, event(0), event(-1), event(1.5),
    event(1, NaN), event(1, -1), event(1, 60_001), { ...event(), processingEnd: 110 },
    { ...event(), processingStart: undefined }, { ...event(), startTime: -1 },
  ]) capture.record(invalid);
  expect(capture.snapshot()).toMatchObject({ observed_interactions: 0, slowest: null });
});

test("retention is bounded by recent IDs and age, including clock reversal", () => {
  let now = 1000;
  const capture = new RecentInteractions(() => now);
  for (let id = 1; id <= 1000; id++) capture.record(event(id));
  expect(capture.snapshot().observed_interactions).toBe(200);
  now += 60_001;
  expect(capture.snapshot().observed_interactions).toBe(0);
  capture.record(event());
  now--;
  expect(capture.snapshot().observed_interactions).toBe(0);
});

test("quantized duration cannot make negative presentation and reports contain no IDs or references", () => {
  const capture = new RecentInteractions();
  capture.record(event(12345, 48));
  const snapshot = capture.snapshot();
  expect(snapshot.slowest?.presentation_estimate_ms).toBe(0);
  expect(JSON.stringify(snapshot)).not.toContain("12345");
  snapshot.slowest!.duration_ms = 999;
  expect(capture.snapshot().slowest?.duration_ms).toBe(48);
});
