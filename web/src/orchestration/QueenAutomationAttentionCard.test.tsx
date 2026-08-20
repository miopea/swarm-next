import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { QueenAutomationStatus } from "../api";
import QueenAutomationAttentionCard from "./QueenAutomationAttentionCard";

afterEach(cleanup);

const status: QueenAutomationStatus = {
  enabled: true,
  state: "completed",
  run_id: "run-1",
  trigger: "actionable_work",
  actionable_count: 1,
  attempts: 1,
  requested_at: 1,
  delivered_at: 2,
  finished_at: 3,
  outcome: "needs_operator",
  waiting_reason: null,
};

test("routes an operator-blocked review to Queen or its settings", () => {
  const onOpenQueen = vi.fn();
  const onReviewSettings = vi.fn();
  render(<QueenAutomationAttentionCard status={status} onOpenQueen={onOpenQueen} onReviewSettings={onReviewSettings} />);

  expect(screen.getByRole("heading", { name: "Queen needs you" })).toBeInTheDocument();
  // Worded for this surface: the Needs-you card answers "what wants me and
  // what do I do", not "how does automation work".
  expect(screen.getByText("Queen filed a request and stopped. Open her to resolve it.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Open Queen" }));
  fireEvent.click(screen.getByRole("button", { name: "Review automation" }));
  expect(onOpenQueen).toHaveBeenCalledOnce();
  expect(onReviewSettings).toHaveBeenCalledOnce();
});

test("stays absent for a safe completed review", () => {
  const { container } = render(<QueenAutomationAttentionCard status={{ ...status, outcome: "completed" }} onOpenQueen={() => undefined} onReviewSettings={() => undefined} />);
  expect(container).toBeEmptyDOMElement();
});

test("stays absent when Queen already filed a concrete decision", () => {
  const { container } = render(<QueenAutomationAttentionCard status={status} coveredBySpecificDecision onOpenQueen={() => undefined} onReviewSettings={() => undefined} />);
  expect(container).toBeEmptyDOMElement();
});

const uncertain: QueenAutomationStatus = {
  ...status,
  state: "uncertain",
  outcome: null,
  finished_at: null,
};

test("offers to resume a review that could not be confirmed", () => {
  // Raised as: the same message appears in three places and none of them is
  // where you can act on it. The only control that resolved this lived in
  // settings, two screens from where the operator meets the problem.
  const onRetry = vi.fn().mockResolvedValue(undefined);
  render(<QueenAutomationAttentionCard status={uncertain} onOpenQueen={vi.fn()} onReviewSettings={vi.fn()} onRetry={onRetry} />);

  // Opening Queen stays first: the message asks the operator to check her
  // terminal before resuming.
  const actions = screen.getAllByRole("button").map((button) => button.textContent);
  expect(actions).toEqual(["Open Queen", "Resume review", "Review automation"]);

  fireEvent.click(screen.getByRole("button", { name: "Resume review" }));
  expect(onRetry).toHaveBeenCalledOnce();
});

test("does not offer to resume a review that is not waiting to be resumed", () => {
  // A review blocked on an operator decision is resolved by answering it, not
  // by running it again.
  render(<QueenAutomationAttentionCard status={status} onOpenQueen={vi.fn()} onReviewSettings={vi.fn()} onRetry={vi.fn()} />);

  expect(screen.queryByRole("button", { name: "Resume review" })).not.toBeInTheDocument();
});

test("says so when resuming fails, without claiming anything changed", async () => {
  const onRetry = vi.fn().mockRejectedValue(new Error("offline"));
  render(<QueenAutomationAttentionCard status={uncertain} onOpenQueen={vi.fn()} onReviewSettings={vi.fn()} onRetry={onRetry} />);

  fireEvent.click(screen.getByRole("button", { name: "Resume review" }));

  expect(await screen.findByRole("alert")).toHaveTextContent("Her current work was not changed");
});

test("says something different from the settings panel about the same state", () => {
  // Operator answer, 2026-08-20: keep all three surfaces, word each
  // differently. One sentence cannot answer "what is true of this terminal",
  // "how does this work", and "what do I do", so it answered none of them well.
  render(<QueenAutomationAttentionCard status={uncertain} onOpenQueen={vi.fn()} onReviewSettings={vi.fn()} onRetry={vi.fn()} />);

  expect(screen.getByText(/Check her terminal, then resume it/)).toBeInTheDocument();
  expect(screen.queryByText(/Retry resumes this same review/)).not.toBeInTheDocument();
});
