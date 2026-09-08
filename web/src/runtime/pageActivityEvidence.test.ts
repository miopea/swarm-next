import { expect, test } from "vitest";
import { PageActivityEvidence } from "./pageActivityEvidence";

test("classifies the measured interval rather than callback-time focus", () => {
  let now = 0;
  const activity = new PageActivityEvidence(() => now);
  activity.record("foreground");
  now = 100;
  activity.record("unfocused");
  now = 200;
  activity.record("foreground");
  now = 300;
  expect(activity.during(10, 50)).toBe("foreground");
  expect(activity.during(110, 50)).toBe("unfocused");
  expect(activity.during(10, 250)).toBe("changed");
  expect(activity.during(210, 50)).toBe("foreground");
  expect(activity.during(210, 100)).toBe("unknown");
});

test("evicted, missing, invalid and expired context never establishes foreground", () => {
  let now = 10;
  const activity = new PageActivityEvidence(() => now);
  expect(activity.during(0, 1)).toBe("unknown");
  activity.record("foreground");
  for (let i = 1; i <= 300; i++) {
    now++;
    activity.record(i % 2 ? "hidden" : "foreground");
  }
  expect(activity.during(10, 1)).toBe("unknown");
  expect(activity.during(NaN, 1)).toBe("unknown");
  expect(activity.during(10, -1)).toBe("unknown");
  now += 60_001;
  expect(activity.during(300, 1)).toBe("unknown");
  // A known stable state still covers recent observations after older changes expire.
  expect(activity.during(now - 10, 5)).toBe("foreground");
  now = 1;
  activity.record("unknown");
  expect(activity.during(0, 1)).toBe("unknown");
});

test("repeated identical observations do not discard the original boundary", () => {
  let now = 0;
  const activity = new PageActivityEvidence(() => now);
  activity.record("foreground");
  for (now = 1; now < 500; now++) activity.record("foreground");
  expect(activity.during(1, 400)).toBe("foreground");
});
