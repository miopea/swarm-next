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
  thread_count: 1,
};

test("names the person still waiting and how to reach the task", () => {
  // A worker closed an email task without anyone replying, and nothing said so.
  const onOpenTask = vi.fn();
  render(<UnansweredEmailAttentionCard awaiting={[waiting]} onOpenTask={onOpenTask} />);

  expect(screen.getByRole("heading", { name: "Lynn Kuczyra is waiting on a reply" })).toBeInTheDocument();
  expect(screen.getByText(/No reply has been written/)).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Open the task" }));
  expect(onOpenTask).toHaveBeenCalledWith("task-1");
});

test("separates a reply that was written from one that was sent", () => {
  // Drafting is not answering; the requester has still heard nothing.
  render(<UnansweredEmailAttentionCard awaiting={[{ ...waiting, drafted: true }]} onOpenTask={vi.fn()} />);

  expect(screen.getByText(/A reply is written but was never sent/)).toBeInTheDocument();
});

test("gives every waiting person their own item, approvable on its own", () => {
  // The operator: "we would separate each email into a separate for you item
  // that I could quickly scan through and approve or edit for sending."
  //
  // This used to render only the first and reduce the rest to "and 2 others
  // like it" — unreadable, unsendable, and reachable only by sending the first
  // and waiting for the list to change.
  const onSendReply = vi.fn();
  render(<UnansweredEmailAttentionCard
    awaiting={[
      { ...waiting, drafted: true, draft_id: "reply-1", draft_body: "First reply." },
      { ...waiting, task_id: "task-2", sender_name: "Sharon Echelbarger", drafted: true, draft_id: "reply-2", draft_body: "Second reply." },
      { ...waiting, task_id: "task-3", sender_name: "Larissa Oxley", drafted: true, draft_id: "reply-3", draft_body: "Third reply." },
    ]}
    onOpenTask={vi.fn()}
    onSendReply={onSendReply}
  />);

  expect(screen.getAllByRole("heading", { name: /is waiting on a reply/ })).toHaveLength(3);
  // Named, not counted. The second and third person were previously invisible.
  expect(screen.getByRole("heading", { name: "Sharon Echelbarger is waiting on a reply" })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Larissa Oxley is waiting on a reply" })).toBeInTheDocument();
  expect(screen.getByText("Third reply.")).toBeInTheDocument();

  // And each one sends on its own, rather than the queue draining in order.
  fireEvent.click(screen.getAllByRole("button", { name: "Send this reply" })[2]);
  expect(onSendReply).toHaveBeenCalledWith("reply-3");
});

test("a long draft is cut for scanning and says how long it really is", () => {
  // The drafts being written measured 273 to 627 words on the operator's own
  // Hive the day they called one "way too long". Printing that inline is what
  // buried everyone below it.
  const body = Array.from({ length: 200 }, (_, index) => `word${index}`).join(" ");
  render(<UnansweredEmailAttentionCard
    awaiting={[{ ...waiting, drafted: true, draft_id: "reply-1", draft_body: body }]}
    onOpenTask={vi.fn()}
  />);

  expect(screen.getByText(/200 words · shown in part/)).toBeInTheDocument();
  expect(screen.getByText(/word0 word1/)).toBeInTheDocument();
  expect(screen.queryByText(/word199/)).not.toBeInTheDocument();
});

test("a short draft is shown whole, and still says its length", () => {
  render(<UnansweredEmailAttentionCard
    awaiting={[{ ...waiting, drafted: true, draft_id: "reply-1", draft_body: "The adjustment saves correctly now." }]}
    onOpenTask={vi.fn()}
  />);

  expect(screen.getByText("The adjustment saves correctly now.")).toBeInTheDocument();
  expect(screen.getByText("5 words")).toBeInTheDocument();
});

test("says how many people one Send actually reaches", () => {
  // The card named only the earliest sender, so a reply fanning out to seven
  // original threads read exactly like a reply to one. This Hive has sent to
  // seven. Deciding whether to press Send without knowing who hears about it
  // is not a decision.
  render(<UnansweredEmailAttentionCard
    awaiting={[{ ...waiting, thread_count: 7, drafted: true, draft_id: "reply-1", draft_body: "Fixed." }]}
    onOpenTask={vi.fn()}
  />);

  expect(screen.getByRole("heading", { name: "Lynn Kuczyra and 6 others are waiting on a reply" })).toBeInTheDocument();
  expect(screen.getByText(/Sending answers all 7 original threads at once/)).toBeInTheDocument();
});

test("a single thread is not described as a group", () => {
  render(<UnansweredEmailAttentionCard awaiting={[waiting]} onOpenTask={vi.fn()} />);

  expect(screen.getByRole("heading", { name: "Lynn Kuczyra is waiting on a reply" })).toBeInTheDocument();
  expect(screen.queryByText(/original threads at once/)).not.toBeInTheDocument();
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
