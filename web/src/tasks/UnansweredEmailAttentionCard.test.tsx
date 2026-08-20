import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import UnansweredEmailAttentionCard from "./UnansweredEmailAttentionCard";

afterEach(cleanup);

const waiting = {
  task_id: "task-1",
  title: "Re: Adjustment Request",
  sender_name: "Lynn Kuczyra",
  sender_address: "l.kuczyra@rcg.org",
  received_at: 1_786_730_000,
  drafted: false,
};

test("names the person still waiting and how to reach the task", () => {
  // A worker closed an email task without anyone replying, and nothing said so.
  const onOpenTask = vi.fn();
  render(<UnansweredEmailAttentionCard awaiting={[waiting]} onOpenTask={onOpenTask} />);

  expect(screen.getByRole("heading", { name: /has not been answered/ })).toBeInTheDocument();
  expect(screen.getByText(/Lynn Kuczyra is still waiting/)).toBeInTheDocument();
  expect(screen.getByText(/No reply has been written/)).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Open the task" }));
  expect(onOpenTask).toHaveBeenCalledWith("task-1");
});

test("separates a reply that was written from one that was sent", () => {
  // Drafting is not answering; the requester has still heard nothing.
  render(<UnansweredEmailAttentionCard awaiting={[{ ...waiting, drafted: true }]} onOpenTask={vi.fn()} />);

  expect(screen.getByText(/A reply is written but was never sent/)).toBeInTheDocument();
});

test("counts the rest without listing them", () => {
  render(<UnansweredEmailAttentionCard
    awaiting={[waiting, { ...waiting, task_id: "task-2" }, { ...waiting, task_id: "task-3" }]}
    onOpenTask={vi.fn()}
  />);

  expect(screen.getByRole("heading", { name: "3 finished tasks have not been answered" })).toBeInTheDocument();
  expect(screen.getByText(/and 2 others like it/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Open the oldest" })).toBeInTheDocument();
});

test("says nothing when every finished email task has been answered", () => {
  const { container } = render(<UnansweredEmailAttentionCard awaiting={[]} onOpenTask={vi.fn()} />);

  expect(container).toBeEmptyDOMElement();
});
