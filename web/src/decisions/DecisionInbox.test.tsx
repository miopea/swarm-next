import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { DecisionRequest, Task, Worker } from "../api";
import DecisionInbox from "./DecisionInbox";

afterEach(cleanup);

const pending: DecisionRequest = {
  id: "decision-1",
  hive_id: "hive-1",
  requesting_worker_id: "worker-1",
  task_id: "task-1",
  kind: "input",
  urgency: "time_sensitive",
  title: "Choose the durable route",
  reason: "Two valid paths remain",
  risk: "The wrong choice adds migration work",
  evidence: "Both prototypes pass",
  summary: "Whether to take the durable route now or the minimal one and migrate later.",
  suggested_action: "Use the durable path",
  allowed_actions: ["durable_path", "minimal_path"],
  deadline: null,
  state: "pending",
  resolution_action: null,
  resolution_note: "",
  resolved_by_operator_id: null,
  created_at: 1,
  updated_at: 1,
  resolved_at: null,
  delivery_state: null,
};

const resolved: DecisionRequest = {
  ...pending,
  id: "decision-2",
  task_id: null,
  title: "Approve release",
  urgency: "normal",
  state: "resolved",
  resolution_action: "ship",
  resolution_note: "Checks are green",
  delivery_state: "delivered",
  resolved_by_operator_id: "operator-1",
  resolved_at: 2,
};

const queued = { ...resolved, id: "decision-3", title: "Queued release", delivery_state: "queued" } as DecisionRequest;
const dispatching = { ...resolved, id: "decision-4", title: "Sending release", delivery_state: "dispatching" } as DecisionRequest;
const uncertain = { ...resolved, id: "decision-5", title: "Uncertain release", delivery_state: "uncertain" } as DecisionRequest;
const task = { id: "task-1", title: "Stabilize reloads" } as Task;
const worker = { id: "worker-1", name: "Petal" } as Worker;

test("keeps resolved history quiet until the operator asks for it", () => {
  render(
    <DecisionInbox
      decisions={[pending, resolved, queued, dispatching, uncertain]}
      tasks={[task]}
      workers={[worker]}
      busy={false}
      onResolve={vi.fn()}
    />,
  );

  expect(screen.getByText("Choose the durable route")).toBeInTheDocument();
  expect(screen.getByText("Petal · Input")).toBeInTheDocument();
  expect(screen.getByText("Stabilize reloads")).toBeInTheDocument();
  expect(screen.queryByText("Approve release")).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("checkbox", { name: "Show resolved" }));
  expect(screen.getByText("Approve release")).toBeInTheDocument();
  expect(screen.getAllByText(/Checks are green/)).toHaveLength(4);
  expect(screen.getByText("Delivered to worker")).toBeInTheDocument();
  expect(screen.getByText("Waiting for a quiet moment")).toBeInTheDocument();
  expect(screen.getByText("Sending now")).toBeInTheDocument();
  expect(screen.getByText("Delivery uncertain · worker can retrieve it")).toBeInTheDocument();
});

test("returns the selected action with the operator note", () => {
  const onResolve = vi.fn().mockResolvedValue(undefined);
  render(
    <DecisionInbox
      decisions={[pending]}
      tasks={[task]}
      workers={[worker]}
      busy={false}
      onResolve={onResolve}
    />,
  );

  fireEvent.change(screen.getByLabelText("Optional note"), {
    target: { value: "Use the migration-safe option" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Durable path" }));

  expect(onResolve).toHaveBeenCalledWith(
    pending,
    "durable_path",
    "Use the migration-safe option",
    "inbox_action",
  );
});

test("opens the task that gave a decision its context", () => {
  const onOpenTask = vi.fn();
  render(
    <DecisionInbox decisions={[pending]} tasks={[task]} workers={[worker]} busy={false} onOpenTask={onOpenTask} onResolve={vi.fn()} />,
  );

  fireEvent.click(screen.getByRole("button", { name: task.title }));
  expect(onOpenTask).toHaveBeenCalledWith(task.id);
});

test("reveals and focuses a resolved decision selected through global navigation", async () => {
  const scrollIntoView = vi.fn();
  Element.prototype.scrollIntoView = scrollIntoView;
  render(
    <DecisionInbox
      decisions={[resolved]}
      tasks={[task]}
      workers={[worker]}
      busy={false}
      focusDecisionId={resolved.id}
      focusRequest={1}
      onResolve={vi.fn()}
    />,
  );

  const card = await screen.findByRole("article", { name: "" });
  await waitFor(() => expect(card).toHaveFocus());
  expect(screen.getByRole("checkbox", { name: "Show resolved" })).toBeChecked();
  expect(scrollIntoView).toHaveBeenCalled();
});

test("requires confirmation before dismissing without a proposed action", () => {
  const onResolve = vi.fn().mockResolvedValue(undefined);
  render(
    <DecisionInbox
      decisions={[pending]}
      tasks={[task]}
      workers={[worker]}
      busy={false}
      onResolve={onResolve}
    />,
  );

  fireEvent.change(screen.getByLabelText("Optional note"), {
    target: { value: "The queue changed; review current work again." },
  });
  fireEvent.click(screen.getByRole("button", { name: "Dismiss request" }));
  expect(onResolve).not.toHaveBeenCalled();

  fireEvent.click(screen.getByRole("button", { name: "Confirm dismiss" }));
  expect(onResolve).toHaveBeenCalledWith(
    pending,
    "dismissed",
    "The queue changed; review current work again.",
    "inbox_dismiss",
  );
});

test("counts and displays first-class attention that does not originate as a worker decision", () => {
  render(
    <DecisionInbox
      decisions={[]}
      tasks={[]}
      workers={[]}
      busy={false}
      additionalPendingCount={1}
      attentionCards={<article>Queen needs you</article>}
      onResolve={vi.fn()}
    />,
  );

  expect(screen.getByRole("tab", { name: "Needs you 1" })).toBeInTheDocument();
  expect(screen.getByText("Queen needs you")).toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "Nothing needs your attention" })).not.toBeInTheDocument();
});

test("does not move the card under the operator every time the inbox refreshes", async () => {
  // The operator reported resolving a decision with an action they did not
  // choose. The card was being scrolled and refocused on every change to the
  // decision list, not only when navigation asked for it — so on a busy Hive
  // the card moves between reading an action and clicking it.
  const scrollIntoView = vi.fn();
  Element.prototype.scrollIntoView = scrollIntoView;
  const view = render(
    <DecisionInbox
      decisions={[pending]}
      tasks={[task]}
      workers={[worker]}
      busy={false}
      focusDecisionId={pending.id}
      focusRequest={1}
      onResolve={vi.fn()}
    />,
  );
  // The scroll is scheduled on an animation frame, so the assertion has to
  // outlive one or it measures nothing.
  await waitFor(() => expect(scrollIntoView).toHaveBeenCalled());
  scrollIntoView.mockClear();

  // Same focus request, new data — an ordinary live refresh.
  view.rerender(
    <DecisionInbox
      decisions={[pending, resolved]}
      tasks={[task]}
      workers={[worker]}
      busy={false}
      focusDecisionId={pending.id}
      focusRequest={1}
      onResolve={vi.fn()}
    />,
  );

  await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  expect(scrollIntoView).not.toHaveBeenCalled();
});

test("names which control the operator used, so a disputed answer can be traced", () => {
  // A decision was recorded with an action the operator says they did not
  // choose, and nothing captured where the answer arrived from.
  const onResolve = vi.fn().mockResolvedValue(undefined);
  render(
    <DecisionInbox decisions={[pending]} tasks={[task]} workers={[worker]} busy={false} onResolve={onResolve} />,
  );

  fireEvent.click(screen.getByRole("button", { name: "Durable path" }));
  expect(onResolve).toHaveBeenCalledWith(pending, "durable_path", "", "inbox_action");

  fireEvent.click(screen.getByRole("button", { name: "Dismiss request" }));
  fireEvent.click(screen.getByRole("button", { name: "Confirm dismiss" }));
  expect(onResolve).toHaveBeenCalledWith(pending, "dismissed", "", "inbox_dismiss");
});

test("offers actions as buttons that cannot submit anything", () => {
  // A button with no type is a submit button. The dismiss control beside these
  // already says so explicitly; the action buttons did not.
  render(
    <DecisionInbox decisions={[pending]} tasks={[task]} workers={[worker]} busy={false} onResolve={vi.fn()} />,
  );

  for (const label of ["Durable path", "Minimal path"]) {
    expect(screen.getByRole("button", { name: label })).toHaveAttribute("type", "button");
  }
});

test("cannot resolve a decision from the keyboard without choosing an action", async () => {
  // Asked directly: can Enter, Tab, or a focused control resolve a decision
  // without a deliberate press on that specific action? Navigation focuses the
  // card, which is not a control, so Enter there does nothing.
  const onResolve = vi.fn().mockResolvedValue(undefined);
  Element.prototype.scrollIntoView = vi.fn();
  render(
    <DecisionInbox
      decisions={[pending]}
      tasks={[task]}
      workers={[worker]}
      busy={false}
      focusDecisionId={pending.id}
      focusRequest={1}
      onResolve={onResolve}
    />,
  );

  await waitFor(() => expect(document.activeElement).toBe(
    document.querySelector(`[data-decision-id="${pending.id}"]`),
  ));
  fireEvent.keyDown(document.activeElement!, { key: "Enter" });
  fireEvent.keyUp(document.activeElement!, { key: "Enter" });
  expect(onResolve).not.toHaveBeenCalled();

  // Reaching an action still takes an explicit press on that action.
  fireEvent.click(screen.getByRole("button", { name: "Minimal path" }));
  expect(onResolve).toHaveBeenCalledWith(pending, "minimal_path", "", "inbox_action");
});

test("answers an interview instead of offering buttons the asker had to guess", () => {
  // A record carrying questions has no allowed_actions: the asker did not know
  // what to offer, which is why it asked.
  const onAnswer = vi.fn().mockResolvedValue(undefined);
  const interview: DecisionRequest = {
    ...pending,
    id: "decision-interview",
    title: "How wide should the mapping fix go?",
    allowed_actions: [],
    questions: [
      { header: "Scope", question: "How wide?", options: ["This project", "Every project"] },
    ],
  };
  render(
    <DecisionInbox decisions={[interview]} tasks={[task]} workers={[worker]} busy={false} onResolve={vi.fn()} onAnswer={onAnswer} />,
  );

  expect(screen.getByText("How wide?")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Every project" }));
  fireEvent.click(screen.getByRole("button", { name: "Send answers" }));

  expect(onAnswer).toHaveBeenCalledWith(interview, { Scope: ["Every project"] }, "");
});

test("declining an interview requires a reason the worker can act on", () => {
  // The recorded failure: dismissed with an empty note, so "hold for now" and
  // "stop asking me" were stored identically.
  const onResolve = vi.fn().mockResolvedValue(undefined);
  const interview: DecisionRequest = {
    ...pending,
    id: "decision-interview-2",
    allowed_actions: [],
    questions: [{ header: "Scope", question: "How wide?", options: ["One", "All"] }],
  };
  render(
    <DecisionInbox decisions={[interview]} tasks={[task]} workers={[worker]} busy={false} onResolve={onResolve} onAnswer={vi.fn()} />,
  );

  const decline = screen.getByRole("button", { name: "Decline with a reason" });
  expect(decline).toBeDisabled();

  fireEvent.change(screen.getByLabelText("Reason"), { target: { value: "Holding until the mapping is fixed." } });
  expect(decline).toBeEnabled();
  fireEvent.click(decline);
  expect(onResolve).toHaveBeenCalledWith(interview, "dismissed", "Holding until the mapping is fixed.", "inbox_dismiss");
});

test("lets the operator answer a ruling with something none of the buttons offered", () => {
  // Observed: a request offered three actions and the operator wanted a fourth
  // thing entirely. Pressing the closest button or dismissing were the only
  // ways out, and both lose the answer.
  const onAnswer = vi.fn().mockResolvedValue(undefined);
  render(
    <DecisionInbox decisions={[pending]} tasks={[task]} workers={[worker]} busy={false} onResolve={vi.fn()} onAnswer={onAnswer} />,
  );

  fireEvent.click(screen.getByRole("button", { name: "Say something else" }));
  fireEvent.change(screen.getByLabelText("Tell the worker what to do instead"), {
    target: { value: "Add it to the Play Store yourself, using the browser extension" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send this instead" }));

  expect(onAnswer).toHaveBeenCalledWith(
    pending,
    { Answer: ["Add it to the Play Store yourself, using the browser extension"] },
    "",
  );
});

test("will not send an empty answer in place of a button", () => {
  render(
    <DecisionInbox decisions={[pending]} tasks={[task]} workers={[worker]} busy={false} onResolve={vi.fn()} onAnswer={vi.fn()} />,
  );

  fireEvent.click(screen.getByRole("button", { name: "Say something else" }));
  expect(screen.getByRole("button", { name: "Send this instead" })).toBeDisabled();
});

test("leads with what is being decided and folds the argument behind it", () => {
  // Raised as: the assessment is way too long and gives no concise analysis of
  // what is being decided. On the live inbox one request ran to roughly five
  // thousand characters of reason, risk and evidence.
  render(
    <DecisionInbox decisions={[pending]} tasks={[task]} workers={[worker]} busy={false} onResolve={vi.fn()} />,
  );

  expect(screen.getByText(/Whether to take the durable route now/)).toBeInTheDocument();
  // The argument is present and not in the way.
  const argument = screen.getByText("Why, and what it rests on");
  expect(argument).toBeInTheDocument();
  expect(argument.closest("details")).not.toHaveAttribute("open");
});

test("does not push a card the operator is reaching for down the page", () => {
  // The remaining half of the same defect. Scroll and focus were fixed; the
  // list itself still reflowed. The server orders pending decisions newest
  // first, so a decision arriving during an ordinary refresh is inserted ABOVE
  // the ones already on screen and shoves every card below it down by a whole
  // card. An operator mid-reach for an action has that action move, and the
  // click lands on whatever slid into its place — which is exactly the report
  // that opened this task.
  const arriving = { ...pending, id: "decision-9", title: "Arrived while reading", created_at: 9, updated_at: 9 };
  const { rerender } = render(
    <DecisionInbox decisions={[pending]} tasks={[task]} workers={[worker]} busy={false} onResolve={vi.fn()} />,
  );
  expect(order()).toEqual([pending.id]);

  // The server's order, newest first, as it would arrive from a poll.
  rerender(
    <DecisionInbox decisions={[arriving, pending]} tasks={[task]} workers={[worker]} busy={false} onResolve={vi.fn()} />,
  );

  // The card that was already on screen has not moved; the new one is below it.
  expect(order()).toEqual([pending.id, arriving.id]);
});

function order() {
  return Array.from(document.querySelectorAll("[data-decision-id]")).map(
    (card) => card.getAttribute("data-decision-id"),
  );
}
