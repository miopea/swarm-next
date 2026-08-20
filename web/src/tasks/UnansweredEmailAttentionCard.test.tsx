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
  draft_id: null,
  draft_body: null,
  worker_name: "Public Website",
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

test("puts the written reply where the operator already is, and sends it from there", () => {
  // The operator's ruling: "The worker should verify something is up in
  // production, not me. The only thing I verify on an email task like this is
  // the draft response that was already generated. It should be located in the
  // 'needs you' section."
  //
  // Reviewing the words is theirs. Whether the work is actually running is the
  // worker's, recorded as deployment evidence — and it is no longer asked of
  // the operator here at all.
  const onSendReply = vi.fn();
  const onOpenTask = vi.fn();
  render(<UnansweredEmailAttentionCard
    awaiting={[{ ...waiting, drafted: true, draft_id: "reply-1", draft_body: "Thank you for reporting this. The adjustment saves correctly now." }]}
    onOpenTask={onOpenTask}
    onSendReply={onSendReply}
  />);

  // The words themselves, not a promise that they exist somewhere else.
  expect(screen.getByText(/The adjustment saves correctly now/)).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Send this reply" }));
  expect(onSendReply).toHaveBeenCalledWith("reply-1");
  // Opening the task is still there for changing the wording, but it is no
  // longer the only way through.
  expect(screen.getByRole("button", { name: "Edit first" })).toBeInTheDocument();
});

test("names the worker whose work this was", () => {
  // The operator: tasks on Needs you never say which worker they are directed
  // to. A decision card names its requester; the attention cards named nobody,
  // so every one of them looked like it belonged to no one.
  render(<UnansweredEmailAttentionCard awaiting={[waiting]} onOpenTask={vi.fn()} />);

  expect(screen.getByText("Public Website · Email")).toBeInTheDocument();
});
