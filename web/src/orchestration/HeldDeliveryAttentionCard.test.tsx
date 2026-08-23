import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { HeldDelivery } from "../api";
import HeldDeliveryAttentionCard from "./HeldDeliveryAttentionCard";

afterEach(cleanup);

function held(overrides: Partial<HeldDelivery> = {}): HeldDelivery {
  return {
    kind: "delivery_held_open_prompt",
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
  expect(screen.getByText(/Nothing gets routed while Queen's terminal has an open question/)).toBeInTheDocument();
  // The count is the evidence that this is stuck rather than merely slow.
  expect(screen.getByText(/retried 1503 times/)).toBeInTheDocument();
});

test("says a worker's work is waiting rather than lost", () => {
  render(<HeldDeliveryAttentionCard held={[held({ subject: "task-brief:t1", worker_name: "Poppy", observations: 20 })]} />);

  expect(screen.getByText("Poppy has work waiting behind a prompt")).toBeInTheDocument();
  expect(screen.getByText(/Swarm will not type into a terminal with an open question/)).toBeInTheDocument();
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
  expect(screen.getByText("2 things are waiting at worker prompts")).toBeInTheDocument();
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

/**
 * An unconfirmed wake is a different situation from a delivery waiting its
 * turn, and needs a different instruction. Two were sitting unseen in the
 * operator's Hive from 2026-08-21: work assigned, never started, reading as
 * routed, with nothing that would ever retry it.
 */
test("says unstarted work was never started, and what to do about it", () => {
  render(
    <HeldDeliveryAttentionCard
      held={[held({ kind: "wake_uncertain", subject: "wake:t1", worker_name: "Real Truth", observations: 1 })]}
    />,
  );

  expect(screen.getByText("Real Truth was assigned work that never started")).toBeInTheDocument();
  expect(screen.getByText(/will not try again/)).toBeInTheDocument();
  expect(screen.getByText(/waking it twice briefs it twice/)).toBeInTheDocument();
  expect(screen.getByText(/Wake it yourself and it picks up from there/)).toBeInTheDocument();
  // Not described as waiting: nothing is going to deliver it.
  expect(screen.queryByText(/waiting rather than lost/)).not.toBeInTheDocument();
});

test("counts unstarted work when several workers never started", () => {
  render(
    <HeldDeliveryAttentionCard
      held={[
        held({ kind: "wake_uncertain", subject: "wake:t1", worker_name: "Real Truth" }),
        held({ kind: "wake_uncertain", subject: "wake:t2", worker_name: "Poppy" }),
      ]}
    />,
  );
  expect(screen.getByText("2 tasks are assigned to workers that never started them")).toBeInTheDocument();
});

/**
 * The 2026-08-23 wedge. Queen's prompt held an unsent `/rc` left behind by a
 * Remote Control reconnect. The card told the operator to answer a prompt, they
 * opened Queen, found no question, and the board sat at zero active tasks for
 * three hours while the review was refused 388 times.
 *
 * Telling someone to answer a question that does not exist is worse than saying
 * nothing: it spends the one look they were going to give it.
 */
test("tells the operator to clear the line, not to answer a question", () => {
  render(
    <HeldDeliveryAttentionCard
      held={[held({ kind: "delivery_held_unsent_text", observations: 388 })]}
    />,
  );

  expect(screen.getByText("Queen cannot review until her prompt is cleared")).toBeInTheDocument();
  expect(screen.getByText(/holds unsent text/)).toBeInTheDocument();
  expect(screen.queryByText(/Answer it and the review resumes/)).not.toBeInTheDocument();
});

test("says the same for a worker that is not Queen", () => {
  render(
    <HeldDeliveryAttentionCard
      held={[
        held({
          kind: "delivery_held_unsent_text",
          subject: "task-brief:019ff",
          worker_name: "Sculpt Studio",
        }),
      ]}
    />,
  );

  expect(screen.getByText("Sculpt Studio has an unsent line at its prompt")).toBeInTheDocument();
  expect(screen.getByText(/Clear the line and this delivers itself/)).toBeInTheDocument();
});

/**
 * The card is laid out by the shared three-column attention grid: bee, content,
 * actions. This one has no bee. If it ever grows or loses a top-level child
 * without the stylesheet following, the text lands in the wrong track and wraps
 * one word per line — which is exactly how it shipped.
 */
test("renders the two top-level children its grid is sized for", () => {
  const { container } = render(
    <HeldDeliveryAttentionCard held={[held()]} onOpenWorker={vi.fn()} />,
  );

  const card = container.querySelector(".held-delivery-card");
  expect(card).not.toBeNull();
  expect(card?.children).toHaveLength(2);
});
