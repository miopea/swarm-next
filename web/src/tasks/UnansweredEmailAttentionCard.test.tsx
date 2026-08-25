import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
  delivery_failure: null,
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

test("the draft is shown whole, however long, with its length stated", () => {
  // A correction of a correction. This was briefly cut to 45 words on the
  // reasoning that a wall of text buries the people below it — but the
  // per-card split had already fixed that, so the cut bought nothing and cost
  // sense. The operator: "we jumped to the other ditch. The view of the reply
  // gets cut off with an ellipse and doesn't make sense."
  const body = Array.from({ length: 200 }, (_, index) => `word${index}`).join(" ");
  render(<UnansweredEmailAttentionCard
    awaiting={[{ ...waiting, drafted: true, draft_id: "reply-1", draft_body: body }]}
    onOpenTask={vi.fn()}
  />);

  expect(screen.getByText(/word199/)).toBeInTheDocument();
  expect(screen.queryByText(/…/)).not.toBeInTheDocument();
  // The count stays: "how long is this" is the complaint that started all of it.
  expect(screen.getByText("200 words")).toBeInTheDocument();
});

test("the reply is edited and saved without leaving this screen", () => {
  // The operator: "This kicks me to the task page to edit, this should stay on
  // the task page." Reading and fixing the words is the only part of an email
  // task that is theirs, and it was the one part that sent them elsewhere.
  const onSaveReply = vi.fn();
  const onOpenTask = vi.fn();
  render(<UnansweredEmailAttentionCard
    awaiting={[{ ...waiting, drafted: true, draft_id: "reply-1", draft_body: "Original wording." }]}
    onOpenTask={onOpenTask}
    onSendReply={vi.fn()}
    onSaveReply={onSaveReply}
  />);

  fireEvent.click(screen.getByRole("button", { name: "Edit here" }));
  const editor = screen.getByRole("textbox");
  expect(editor).toHaveValue("Original wording.");

  fireEvent.change(editor, { target: { value: "Shorter wording." } });
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));

  expect(onSaveReply).toHaveBeenCalledWith("task-1", "Shorter wording.");
  // And nothing navigated away to do it.
  expect(onOpenTask).not.toHaveBeenCalled();
});

test("a prompted revision replaces the draft and stays undoable", async () => {
  // The operator ruled: replace in place, with the previous version
  // recoverable. The expensive failure is a prompt that overshoots and takes a
  // draft they liked with it — they said of one "I like how it's written".
  const onReviseReply = vi.fn().mockResolvedValue("Short version.");
  const onSaveReply = vi.fn();
  render(<UnansweredEmailAttentionCard
    awaiting={[{ ...waiting, drafted: true, draft_id: "reply-1", draft_body: "A long original draft." }]}
    onOpenTask={vi.fn()}
    onSaveReply={onSaveReply}
    onReviseReply={onReviseReply}
  />);

  fireEvent.click(screen.getByRole("button", { name: "Edit here" }));
  fireEvent.change(screen.getByPlaceholderText(/Halve it/), { target: { value: "halve it" } });
  fireEvent.click(screen.getByRole("button", { name: "Revise" }));

  await waitFor(() => expect(screen.getByRole("textbox", { name: /Reply to/ })).toHaveValue("Short version."));
  expect(onReviseReply).toHaveBeenCalledWith("task-1", "halve it");
  // Nothing is written until the operator says so.
  expect(onSaveReply).not.toHaveBeenCalled();

  fireEvent.click(screen.getByRole("button", { name: "Undo revision" }));
  expect(screen.getByRole("textbox", { name: /Reply to/ })).toHaveValue("A long original draft.");
});

test("a failed revision leaves the draft alone and offers no undo", async () => {
  // A revision that could not be produced must not present as a change, or the
  // operator undoes something that never happened.
  const onReviseReply = vi.fn().mockResolvedValue(null);
  render(<UnansweredEmailAttentionCard
    awaiting={[{ ...waiting, drafted: true, draft_id: "reply-1", draft_body: "Untouched draft." }]}
    onOpenTask={vi.fn()}
    onSaveReply={vi.fn()}
    onReviseReply={onReviseReply}
  />);

  fireEvent.click(screen.getByRole("button", { name: "Edit here" }));
  fireEvent.change(screen.getByPlaceholderText(/Halve it/), { target: { value: "halve it" } });
  fireEvent.click(screen.getByRole("button", { name: "Revise" }));

  await waitFor(() => expect(onReviseReply).toHaveBeenCalled());
  expect(screen.getByRole("textbox", { name: /Reply to/ })).toHaveValue("Untouched draft.");
  expect(screen.queryByRole("button", { name: "Undo revision" })).not.toBeInTheDocument();
});

test("cancelling an edit restores the draft and sends nothing", () => {
  const onSaveReply = vi.fn();
  render(<UnansweredEmailAttentionCard
    awaiting={[{ ...waiting, drafted: true, draft_id: "reply-1", draft_body: "Original wording." }]}
    onOpenTask={vi.fn()}
    onSaveReply={onSaveReply}
  />);

  fireEvent.click(screen.getByRole("button", { name: "Edit here" }));
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "Discard me." } });
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

  expect(onSaveReply).not.toHaveBeenCalled();
  expect(screen.getByText("Original wording.")).toBeInTheDocument();
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

test("a reply that was never delivered says so, loudly, with the reason", () => {
  // Seventeen replies were cancelled on 2026-08-25 and none of them said so:
  // the operator pressed Send, the item left the queue, and it looked handled.
  // They found out by opening Outlook and seeing nothing.
  render(<UnansweredEmailAttentionCard
    awaiting={[{ ...waiting, drafted: true, draft_id: "reply-1", draft_body: "Fixed.", delivery_failure: "The email message was not found" }]}
    onOpenTask={vi.fn()}
  />);

  const failure = screen.getByRole("alert");
  expect(failure).toHaveTextContent("This reply was not delivered");
  expect(failure).toHaveTextContent("The email message was not found");
});

test("a reply that has not failed carries no alarm", () => {
  render(<UnansweredEmailAttentionCard
    awaiting={[{ ...waiting, drafted: true, draft_id: "reply-1", draft_body: "Fixed." }]}
    onOpenTask={vi.fn()}
  />);

  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

test("the first reply is written here, not on the task page", () => {
  // The operator: "This is still showing on the for you page and it links to
  // this, not our new response system" — pressing Open the task landed on the
  // old two-step panel asking them to Confirm the fix is live.
  //
  // That form is wrong twice over. Verifying what is running is the worker's
  // job by their own ruling, and the task in the screenshot was closed on an
  // APPROVED NO-DEPLOYMENT EXEMPTION — a question that needed no change — so it
  // was asking for a deployment that does not exist and never will.
  const onSaveReply = vi.fn();
  const onOpenTask = vi.fn();
  render(<UnansweredEmailAttentionCard
    awaiting={[{ ...waiting, drafted: false, draft_id: null, draft_body: null }]}
    onOpenTask={onOpenTask}
    onSaveReply={onSaveReply}
  />);

  fireEvent.click(screen.getByRole("button", { name: "Write the reply" }));
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "No change was needed, and here is why." } });
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));

  expect(onSaveReply).toHaveBeenCalledWith("task-1", "No change was needed, and here is why.");
  expect(onOpenTask).not.toHaveBeenCalled();
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
    onSaveReply={vi.fn()}
  />);

  // The words themselves, not a promise that they exist somewhere else.
  expect(screen.getByText(/The adjustment saves correctly now/)).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Send this reply" }));
  expect(onSendReply).toHaveBeenCalledWith("reply-1");
  // Changing the wording happens here too, not on the task board — and the
  // task itself stays reachable for the thread, attachments and history.
  expect(screen.getByRole("button", { name: "Edit here" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Open the task" })).toBeInTheDocument();
});

test("names the worker whose work this was", () => {
  // The operator: tasks on Needs you never say which worker they are directed
  // to. A decision card names its requester; the attention cards named nobody,
  // so every one of them looked like it belonged to no one.
  render(<UnansweredEmailAttentionCard awaiting={[waiting]} onOpenTask={vi.fn()} />);

  expect(screen.getByText("Public Website · Email")).toBeInTheDocument();
});
