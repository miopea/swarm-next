import { expect, test } from "vitest";

import { conversationGap } from "./conversationGap";

test("says how much newer, instead of printing two stamps to subtract", () => {
  // The operator's actual screen on 2026-09-11: two ISO stamps 32 seconds
  // apart, and no statement of what that meant.
  expect(conversationGap("2026-09-11T15:24:42.325Z", "2026-09-11T15:25:14.367Z"))
    .toBe("32 seconds after the saved conversation's last entry");
  expect(conversationGap("2026-09-11T15:24:42.325Z", "2026-09-11T15:54:42.325Z"))
    .toBe("30 minutes after the saved conversation's last entry");
  expect(conversationGap("2026-09-04T15:24:42.325Z", "2026-09-11T15:24:42.325Z"))
    .toBe("7 days after the saved conversation's last entry");
});

test("singular reads correctly", () => {
  expect(conversationGap("2026-09-11T15:24:42.000Z", "2026-09-11T15:24:43.000Z"))
    .toContain("1 second ");
  expect(conversationGap("2026-09-11T15:24:42.000Z", "2026-09-11T15:25:42.000Z"))
    .toContain("1 minute ");
});

test("an unreadable or missing stamp says so rather than guessing zero", () => {
  // A confident wrong answer is worse than admitting the stamp was unreadable,
  // because this panel exists to tell the operator whether to worry.
  expect(conversationGap(null, "2026-09-11T15:25:14.367Z"))
    .toBe("and the saved conversation has no recorded entry");
  expect(conversationGap("not-a-date", "2026-09-11T15:25:14.367Z"))
    .toBe("at an unreadable time");
  expect(conversationGap("2026-09-11T15:24:42.325Z", undefined))
    .toBe("at an unreadable time");
});

test("a transcript no newer than the pin is not reported as newer", () => {
  expect(conversationGap("2026-09-11T15:25:14.367Z", "2026-09-11T15:24:42.325Z"))
    .toBe("no later than the saved conversation");
  expect(conversationGap("2026-09-11T15:24:42.325Z", "2026-09-11T15:24:42.325Z"))
    .toBe("no later than the saved conversation");
});
