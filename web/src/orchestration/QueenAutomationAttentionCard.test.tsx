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
  expect(screen.getByText("Open Queen when you are ready to resolve her decision.")).toBeInTheDocument();
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
