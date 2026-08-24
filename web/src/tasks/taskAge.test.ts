import { expect, test } from "vitest";

import { taskAge, taskAgeTitle } from "./taskAge";

const now = Date.parse("2026-08-24T20:00:00Z");
const at = (iso: string) => Math.floor(Date.parse(iso) / 1000);

test("reads as an age at a glance, not as a date to work out", () => {
  expect(taskAge(at("2026-08-24T19:59:40Z"), now)).toBe("just now");
  expect(taskAge(at("2026-08-24T19:42:00Z"), now)).toBe("18m");
  expect(taskAge(at("2026-08-24T14:00:00Z"), now)).toBe("6h");
  // The case that prompted this: drafts five days old and quietly stuck.
  expect(taskAge(at("2026-08-19T20:00:00Z"), now)).toBe("5d");
});

test("never reports a task from the future as negative", () => {
  // Clock skew between a Hive and a browser is real, and "-1m" beside a state
  // reads as a defect in the board rather than in the clock.
  expect(taskAge(at("2026-08-24T20:05:00Z"), now)).toBe("just now");
});

test("keeps the exact moment available without spending a row on it", () => {
  expect(taskAgeTitle(at("2026-08-19T20:00:00Z"))).toMatch(/^Created /);
});
