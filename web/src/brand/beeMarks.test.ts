import { expect, test } from "vitest";

import { BEE_MARKS, BEE_MARK_LABELS, markFor, resolveMark } from "./beeMarks";

/**
 * "Randomly assign these to workers to dress them up and everyone gets the same
 * effect." The second half is the requirement that constrains this: assignment
 * is DERIVED, not drawn. A genuinely random pick would give the roster a
 * different face on every render, which is worse than everyone looking alike.
 */
test("the same worker always gets the same bee", () => {
  const first = markFor("019ff136-7a90-7631-bbc0-f95efd1df576");
  for (let attempt = 0; attempt < 50; attempt += 1) {
    expect(markFor("019ff136-7a90-7631-bbc0-f95efd1df576")).toBe(first);
  }
});

test("different workers mostly get different bees", () => {
  // Not a uniqueness guarantee — twenty-nine workers over twenty-three marks
  // cannot all differ. What matters is that the roster stops being uniform.
  const ids = Array.from({ length: 29 }, (_, index) => `worker-${index}-019ff136`);
  const assigned = new Set(ids.map(markFor));
  expect(assigned.size).toBeGreaterThan(8);
});

test("an operator's choice wins over the derived one", () => {
  const derived = markFor("worker-1");
  const chosen = BEE_MARKS.find((mark) => mark !== derived)!;
  expect(resolveMark("worker-1", chosen)).toBe(chosen);
});

/**
 * A mark that no longer exists — an older build's choice, or one removed from
 * the set — must not render nothing. One unreadable value should never cost a
 * worker its face.
 */
test("an unknown stored mark falls back to the derived one", () => {
  expect(resolveMark("worker-1", "sombrero")).toBe(markFor("worker-1"));
  expect(resolveMark("worker-1", "")).toBe(markFor("worker-1"));
  expect(resolveMark("worker-1", null)).toBe(markFor("worker-1"));
});

test("every mark has a label an operator can read", () => {
  for (const mark of BEE_MARKS) {
    expect(BEE_MARK_LABELS[mark], `${mark} has no label`).toBeTruthy();
  }
  expect(Object.keys(BEE_MARK_LABELS)).toHaveLength(BEE_MARKS.length);
});

/**
 * markFor indexes into BEE_MARKS, so reordering the list silently reassigns
 * every worker's bee. This pins the order rather than the assignment, because
 * the order is the thing a careless edit changes.
 */
test("the mark order is fixed, because assignment indexes into it", () => {
  expect(BEE_MARKS[0]).toBe("plain");
  expect(BEE_MARKS[1]).toBe("spectacles");
  expect(BEE_MARKS).toHaveLength(23);
});
