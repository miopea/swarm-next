import { expect, test } from "vitest";

import { jiraTokenLife, jiraTokenLifeMessage } from "./jiraTokenLife";

const DAY = 86_400;
const now = new Date("2026-09-11T00:00:00Z");
const secondsAgo = (days: number) => Math.floor(now.getTime() / 1000) - days * DAY;

test("a connection Swarm has no date for says so, instead of claiming a year", () => {
  // The failure this prevents: every Hive connected before Swarm recorded
  // dates would otherwise be described as freshly connected and told it has
  // 365 days, when its token may lapse next week.
  for (const missing of [null, undefined, 0, Number.NaN]) {
    expect(jiraTokenLife(missing, now)).toEqual({ kind: "unknown" });
  }
  expect(jiraTokenLifeMessage({ kind: "unknown" })).toBeNull();
});

test("a fresh connection is quiet, and states the bound rather than a date", () => {
  const life = jiraTokenLife(secondsAgo(1), now);
  expect(life.kind).toBe("fine");
  const message = jiraTokenLifeMessage(life);
  // "on or before" is the whole point: Atlassian tells us neither the expiry
  // nor the creation date, so a precise claim would be a guess wearing a
  // date's clothing.
  expect(message).toContain("on or before");
  expect(message).not.toMatch(/expires on [A-Z]/);
});

test("the warning starts a month out and counts down", () => {
  expect(jiraTokenLife(secondsAgo(365 - 31), now).kind).toBe("fine");

  const life = jiraTokenLife(secondsAgo(365 - 30), now);
  expect(life).toMatchObject({ kind: "expiring", daysLeft: 30 });
  expect(jiraTokenLifeMessage(life)).toContain("30 days");

  const tomorrow = jiraTokenLife(secondsAgo(364), now);
  expect(tomorrow).toMatchObject({ kind: "expiring", daysLeft: 1 });
  expect(jiraTokenLifeMessage(tomorrow)).toContain("1 day");
  expect(jiraTokenLifeMessage(tomorrow)).not.toContain("1 days");
});

test("past the bound it says likely expired, not definitely", () => {
  const life = jiraTokenLife(secondsAgo(400), now);
  expect(life.kind).toBe("overdue");
  const message = jiraTokenLifeMessage(life);
  // Still hedged. The token may have been created with a shorter life and died
  // months ago, or the clock may be wrong — either way Swarm did not watch it
  // expire, so it must not claim it did.
  expect(message).toContain("very likely");
});
