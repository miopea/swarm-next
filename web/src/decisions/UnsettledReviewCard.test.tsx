import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import type { UnsettledReview } from "../api";
import UnsettledReviewCard, { groupedByWorker } from "./UnsettledReviewCard";

const waiting = (overrides: Partial<UnsettledReview> = {}): UnsettledReview => ({
  task_id: "01a054c0-147a-7c83-ab9a-fb5e382ed066",
  title: "Ship the picker fix",
  workspace: "/workspace/petal",
  worker_name: "Field Notes",
  kind: "code_no_deployment",
  reason: "it recorded commits that touch code, and no deployment",
  created_at: 1_788_100_000,
  ...overrides,
});

const CLAIM = "a claim that nothing was deployed, which nobody has approved";

test("says how much is waiting, in the heading, without anything being opened", () => {
  render(<UnsettledReviewCard waiting={[waiting(), waiting({ task_id: "b", title: "Second" })]} />);
  // The number is the point: the operator asked to know how much exists
  // without clicking into anything.
  expect(screen.getByRole("heading", { name: /2 pieces of finished work are waiting on you/ })).toBeTruthy();
});

test("says WHY each one waits, because the reasons are not the same problem", () => {
  render(
    <UnsettledReviewCard
      waiting={[
        // Kind and reason travel together — the server picks the pair in one
        // place so a row cannot be chipped as one state and explained as
        // another, and the legend is keyed on the kind.
        waiting({ kind: "nothing_reported", reason: "nobody reported what this work produced" }),
        waiting({ task_id: "b", title: "Second", kind: "claim_unapproved", reason: CLAIM }),
      ]}
    />,
  );
  expect(screen.getByText("nobody reported what this work produced")).toBeTruthy();
  expect(screen.getByText(CLAIM)).toBeTruthy();
});

/**
 * THE LABEL AND THE POPULATION MUST AGREE.
 *
 * The count this design began from said "unverified" and meant something
 * adjacent — 49 where the answer was 31. A heading that claims to be about
 * verification, over rows that are really about settlement, would reproduce
 * exactly that. The heading is allowed to say what is true of every row and
 * nothing wider.
 */
test("the heading claims settlement, never verification", () => {
  render(<UnsettledReviewCard waiting={[waiting()]} />);
  const heading = screen.getByRole("heading", { name: /waiting on you/ }).textContent ?? "";
  expect(heading).not.toMatch(/unverified|verified/i);
  expect(screen.getByText(/Nothing has settled these/)).toBeTruthy();
});

test("renders nothing at all when nothing is waiting", () => {
  const { container } = render(<UnsettledReviewCard waiting={[]} />);
  // A card reading "0" is a card the operator learns to skip.
  expect(container.firstChild).toBeNull();
});

test("a row opens its task", () => {
  const onOpenTask = vi.fn();
  render(<UnsettledReviewCard waiting={[waiting()]} onOpenTask={onOpenTask} />);
  fireEvent.click(screen.getByRole("button", { name: "Ship the picker fix" }));
  expect(onOpenTask).toHaveBeenCalledWith("01a054c0-147a-7c83-ab9a-fb5e382ed066");
});

/**
 * THE OPERATOR'S FIRST QUESTION, and the card shipped unable to answer it.
 *
 * "no clear which worker" over eleven rows. Not a preference: whose work it is
 * is what decides whether the operator can act on a row at all.
 */
test("every row names the worker whose work it is", () => {
  render(
    <UnsettledReviewCard
      waiting={[
        waiting({ worker_name: "Orchard API" }),
        waiting({ task_id: "b", title: "Second", worker_name: "Hedgerow" }),
      ]}
    />,
  );
  expect(screen.getByText("Orchard API")).toBeTruthy();
  expect(screen.getByText("Hedgerow")).toBeTruthy();
});

/**
 * THE REPETITION IS THE DEFECT, and this is the test that would have caught it.
 *
 * Seven of the operator's eleven rows carried a byte-identical forty-eight
 * character sentence. `getAllByText` counting one occurrence over seven rows is
 * the whole fix stated as an assertion: the sentence is said once, in the
 * legend, and the rows carry a chip.
 */
test("a reason shared by seven rows is written out once, not seven times", () => {
  render(
    <UnsettledReviewCard
      waiting={Array.from({ length: 7 }, (_, index) =>
        waiting({ task_id: `t${index}`, title: `Task ${index}`, kind: "claim_unapproved", reason: CLAIM }),
      )}
    />,
  );
  expect(screen.getAllByText(CLAIM)).toHaveLength(1);
  // ...while every row still says which state it is in.
  expect(screen.getAllByText("Claim unapproved")).toHaveLength(8);
});

/**
 * WHAT MADE THE ROWS RAGGED, asserted structurally because jsdom has no layout.
 *
 * The old row was a wrapping flex of two variable-width children: title and the
 * reason SENTENCE. A long title pushed the sentence to a second line, so five of
 * eleven rows were two lines tall. jsdom cannot measure that, but it can prove
 * the sentence is no longer in the row — which is the property that caused it.
 */
test("a row holds a title and a short chip, and never the sentence", () => {
  render(<UnsettledReviewCard waiting={[waiting({ kind: "claim_unapproved", reason: CLAIM })]} />);
  const row = screen.getByRole("button", { name: "Ship the picker fix" }).closest("li");
  expect(row).not.toBeNull();
  expect(row?.textContent).not.toContain(CLAIM);
  expect(row?.textContent).toContain("Claim unapproved");
});

/**
 * The server sorts by worker and then by age. If this re-sorted, the two would
 * be free to disagree about what "oldest first" means and neither would be wrong
 * on its own.
 */
test("grouping gathers runs and does not reorder what the server sent", () => {
  const rows = [
    waiting({ task_id: "a", worker_name: "Orchard API", created_at: 200 }),
    waiting({ task_id: "b", worker_name: "Orchard API", created_at: 300 }),
    waiting({ task_id: "c", worker_name: "Hedgerow", created_at: 100 }),
  ];
  expect(groupedByWorker(rows).map((group) => [group.worker, group.rows.length])).toEqual([
    ["Orchard API", 2],
    ["Hedgerow", 1],
  ]);
});

/**
 * A state this build has no chip for is still work waiting on the operator.
 * Dropping it would make the heading's count disagree with its own list.
 */
test("a kind this build does not know still renders a row", () => {
  render(<UnsettledReviewCard waiting={[waiting({ kind: "some_new_state" })]} />);
  expect(screen.getByRole("heading", { name: /1 piece of finished work is waiting on you/ })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Ship the picker fix" })).toBeTruthy();
});
