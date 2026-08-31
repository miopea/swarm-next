import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import type { UnsettledReview } from "../api";
import UnsettledReviewCard from "./UnsettledReviewCard";

const waiting = (overrides: Partial<UnsettledReview> = {}): UnsettledReview => ({
  task_id: "01a054c0-147a-7c83-ab9a-fb5e382ed066",
  title: "Ship the picker fix",
  workspace: "/workspace/petal",
  reason: "it recorded commits that touch code, and no deployment",
  ...overrides,
});

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
        waiting({ reason: "nobody reported what this work produced" }),
        waiting({ task_id: "b", title: "Second", reason: "a claim that nothing was deployed, which nobody has approved" }),
      ]}
    />,
  );
  expect(screen.getByText("nobody reported what this work produced")).toBeTruthy();
  expect(screen.getByText("a claim that nothing was deployed, which nobody has approved")).toBeTruthy();
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
