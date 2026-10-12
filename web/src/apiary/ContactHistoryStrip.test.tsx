import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";

import ContactHistoryStrip from "./ContactHistoryStrip";

const SLOT = 300;
const UNTIL = 1_790_900_100 - (1_790_900_100 % SLOT);
const FROM = UNTIL - 86_400 + SLOT;
const every = () => Array.from({ length: 288 }, (_, index) => FROM + index * SLOT);
const clock = (seconds: number) => new Date(seconds * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

test("a Hive that kept in touch all day says so", () => {
  render(<ContactHistoryStrip history={{ slot_seconds: SLOT, from: FROM, until: UNTIL, contacted: every() }} />);
  expect(screen.getByRole("img")).toHaveAccessibleName("Reached the Keeper throughout the last 24 hours.");
  expect(document.querySelectorAll(".contact-history-strip span")).toHaveLength(288);
});

/** ⚠️ The case it exists for: a drop that comes back is named, in words. */
test("each gap is named by when it started and ended, and the slot now falls in is not one", () => {
  const outageFrom = FROM + 100 * SLOT;
  const outageUntil = outageFrom + 37 * SLOT;
  const contacted = every().filter((at) => (at < outageFrom || at >= outageUntil) && at !== UNTIL);
  render(<ContactHistoryStrip history={{ slot_seconds: SLOT, from: FROM, until: UNTIL, contacted }} />);
  expect(screen.getByRole("img")).toHaveAccessibleName(`Unreachable ${clock(outageFrom)}–${clock(outageUntil)} in the last 24 hours.`);
});

test("past three gaps, the rest are counted rather than listed", () => {
  const dropped = new Set([10, 20, 30, 40, 50].map((index) => FROM + index * SLOT));
  render(<ContactHistoryStrip history={{ slot_seconds: SLOT, from: FROM, until: UNTIL, contacted: every().filter((at) => !dropped.has(at)) }} />);
  expect(screen.getByRole("img").getAttribute("aria-label")).toMatch(/, and 2 more in the last 24 hours\.$/);
});
