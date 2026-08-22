import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { HeldDelivery } from "../api";
import HeldDeliveryAttentionCard from "./HeldDeliveryAttentionCard";

afterEach(cleanup);

function held(overrides: Partial<HeldDelivery> = {}): HeldDelivery {
  return {
    subject: "queen-review",
    worker_name: null,
    reason: "Queen cannot review while her terminal has an unanswered prompt",
    first_observed_at: 1_755_800_000,
    observations: 1503,
    ...overrides,
  };
}

/**
 * The measured incident: a Queen review held 1503 times over twelve hours
 * behind one unanswered prompt. Nothing reached Needs you, nothing was routed,
 * and the operator reasonably concluded the coordination design was wrong.
 */
test("names Queen's stalled review as the reason nothing is moving", () => {
  render(<HeldDeliveryAttentionCard held={[held()]} />);

  expect(screen.getByText("Queen cannot review until a prompt is answered")).toBeInTheDocument();
  expect(screen.getByText(/Nothing reaches this queue and nothing gets routed/)).toBeInTheDocument();
  // The count is the evidence that this is stuck rather than merely slow.
  expect(screen.getByText(/retried 1503 times/)).toBeInTheDocument();
});

test("says a worker's work is waiting rather than lost", () => {
  render(<HeldDeliveryAttentionCard held={[held({ subject: "task-brief:t1", worker_name: "Poppy", observations: 20 })]} />);

  expect(screen.getByText("Poppy has work waiting behind a prompt")).toBeInTheDocument();
  expect(screen.getByText(/waiting rather than lost/)).toBeInTheDocument();
});

test("counts them when several are held", () => {
  render(
    <HeldDeliveryAttentionCard
      held={[
        held({ subject: "task-brief:t1", worker_name: "Poppy", first_observed_at: 1_755_800_500 }),
        held({ subject: "task-brief:t2", worker_name: "Daisy", first_observed_at: 1_755_800_100 }),
      ]}
      onOpenWorker={vi.fn()}
    />,
  );
  expect(screen.getByText("2 things are waiting behind unanswered prompts")).toBeInTheDocument();
  // The oldest is the one worth naming, not whichever arrived last.
  expect(screen.getByRole("button", { name: "Open Daisy" })).toBeInTheDocument();
});

/** Nothing held is the normal state and must not occupy the queue. */
test("renders nothing when the coordinator is holding nothing", () => {
  const { container } = render(<HeldDeliveryAttentionCard held={[]} />);
  expect(container).toBeEmptyDOMElement();
});

test("opens the worker whose terminal needs answering", () => {
  const onOpenWorker = vi.fn();
  render(<HeldDeliveryAttentionCard held={[held()]} onOpenWorker={onOpenWorker} />);
  fireEvent.click(screen.getByRole("button", { name: "Open Queen" }));
  expect(onOpenWorker).toHaveBeenCalledWith("Queen");
});
