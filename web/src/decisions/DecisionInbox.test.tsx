import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { DecisionRequest, Task, Worker } from "../api";
import DecisionInbox from "./DecisionInbox";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

test("keeps its accessible name on Activity without repeating the page introduction", async () => {
  render(<DecisionInbox decisions={[]} workers={[]} tasks={[]} busy={false} onResolve={vi.fn()} />);
  expect(screen.getByRole("region", { name: "What needs you" })).toBeInTheDocument();
  expect(screen.queryByText("One calm queue")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("tab", { name: "Activity" }));
  expect(screen.getByRole("region", { name: "What needs you" })).toBeInTheDocument();
});

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

test("interview choices and notes survive Activity but never a changed question", () => {
  const question = { header: "Scope", question: "Which scope?", options: ["One", "All"] };
  const decision = { ...pending, questions: [question] };
  const onAnswer = vi.fn();
  const props = { decisions: [decision], tasks: [], workers: [], busy: false, onResolve: vi.fn(), onAnswer };
  const view = render(<DecisionInbox {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "Something else" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Your answer" }), { target: { value: "Only the reviewed rows" } });
  fireEvent.click(screen.getByText("Add an optional note"));
  fireEvent.change(screen.getByRole("textbox", { name: "Anything else the worker should know" }), { target: { value: "Keep the rest unchanged" } });
  fireEvent.click(screen.getByRole("tab", { name: "Activity" }));
  expect(screen.queryByRole("textbox", { name: "Your answer" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("tab", { name: /Needs you/ }));
  expect(screen.getByRole("textbox", { name: "Your answer" })).toHaveValue("Only the reviewed rows");
  expect(screen.getByRole("textbox", { name: "Anything else the worker should know" })).toHaveValue("Keep the rest unchanged");
  expect(screen.getByRole("button", { name: "Send answers" })).toBeEnabled();
  expect(onAnswer).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("tab", { name: "Activity" }));
  view.rerender(<DecisionInbox {...props} decisions={[{ ...decision, questions: [{ ...question, question: "A different scope?" }] }]} />);
  fireEvent.click(screen.getByRole("tab", { name: /Needs you/ }));
  expect(screen.queryByRole("textbox", { name: "Your answer" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Send answers" })).toBeDisabled();
  expect(screen.queryByDisplayValue("Keep the rest unchanged")).not.toBeInTheDocument();
});

const queued = { ...resolved, id: "decision-3", title: "Queued release", delivery_state: "queued" } as DecisionRequest;

test("withdrawn requests leave attention and history never presents them as approval", () => {
  const request: DecisionRequest = { ...pending, state: "withdrawn", withdrawal_reason: "The defect was repaired." };
  render(<DecisionInbox decisions={[request]} workers={[]} tasks={[]} busy={false} onResolve={vi.fn()} />);
  expect(screen.queryByText(request.title)).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("checkbox", { name: "Show history" }));
  expect(screen.getByText(request.title)).toBeInTheDocument();
  expect(screen.getByText(/The defect was repaired/)).toBeInTheDocument();
  expect(screen.getByText("No operator decision or approval was recorded.")).toBeInTheDocument();
  expect(screen.queryByText("Recorded before delivery tracking")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Durable path" })).not.toBeInTheDocument();
});
const dispatching = { ...resolved, id: "decision-4", title: "Sending release", delivery_state: "dispatching" } as DecisionRequest;
const uncertain = { ...resolved, id: "decision-5", title: "Uncertain release", delivery_state: "uncertain" } as DecisionRequest;
const task = { id: "task-1", title: "Stabilize reloads" } as Task;
const worker = { id: "worker-1", name: "Petal" } as Worker;

test("clarification waits outside actionable count, returns with a reply, and never chooses an answer", async () => {
  const onResolve = vi.fn().mockResolvedValue(undefined);
  const saved = { id: "round-1", decision_id: pending.id, operator_id: "operator",
    question: "Why this route?", asked_at: 10, reply: null, replied_at: null,
    replying_worker_id: null, replying_session_id: null, delivery_state: "queued" } as import("../api").DecisionClarification;
  const read = vi.fn().mockResolvedValue([saved]);
  const ask = vi.fn().mockResolvedValue(saved);
  const props = { workers: [worker], tasks: [task], busy: false, onResolve,
    onFetchClarifications: read, onAskClarification: ask };
  const { rerender } = render(<DecisionInbox {...props} decisions={[pending]} />);
  expect(read).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Ask a question" }));
  fireEvent.change(screen.getByLabelText("What would you like clarified?"), { target: { value: saved.question } });
  fireEvent.click(screen.getByRole("button", { name: "Send question" }));
  await waitFor(() => expect(ask).toHaveBeenCalledTimes(1));
  expect(ask.mock.calls[0][0].id).toBe(pending.id);
  expect(ask.mock.calls[0][2]).toBe(saved.question);
  const waiting: DecisionRequest = { ...pending, clarification: { round_count: 1,
    waiting_clarification_id: saved.id, delivery_state: "queued", latest_reply_at: null, next_move: "requester" } };
  rerender(<DecisionInbox {...props} decisions={[waiting]} />);
  expect(screen.getByRole("tab", { name: "Needs you 0" })).toBeInTheDocument();
  expect(screen.getByText(/waiting for a reply, not an answer from you/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Durable path/ })).toBeEnabled();
  expect(onResolve).not.toHaveBeenCalled();
  const replied = { ...saved, reply: "It avoids a second migration.", replied_at: 11,
    replying_worker_id: "worker-1", replying_session_id: "session" };
  read.mockResolvedValue([replied]);
  rerender(<DecisionInbox {...props} decisions={[{ ...waiting, clarification: {
    ...waiting.clarification!, waiting_clarification_id: null, delivery_state: null,
    latest_reply_at: 11, next_move: "operator" } }]} />);
  expect(screen.getByRole("tab", { name: "Needs you 1" })).toBeInTheDocument();
  expect((await screen.findAllByText(replied.reply))[0]).toBeVisible();
  expect(screen.getByRole("button", { name: "Ask a question" })).toBeInTheDocument();
  expect(onResolve).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: /Durable path/ }));
  await waitFor(() => expect(onResolve).toHaveBeenCalledTimes(1));
});

test.each([
  "Enable FEAST_SIGNUP_ENABLED on staging now",
  "use feature_flag=false until the check passes",
  "FEAST_SIGNUP_ENABLED",
  "Run npm run verify_contract before proceeding",
])("preserves authored action identifiers in recommendation, submission and history: %s", async (action) => {
  const onResolve = vi.fn().mockResolvedValue(undefined);
  const request = { ...pending, allowed_actions: [action], suggested_action: action };
  const props = { tasks: [task], workers: [worker], busy: false, onResolve };
  const view = render(<DecisionInbox {...props} decisions={[request]} />);
  const button = screen.getByRole("button", { name: action });
  expect(button).toHaveClass("primary-action");
  expect(screen.getAllByText(action, { exact: true })).toHaveLength(1);
  expect(button).toHaveAccessibleDescription("Petal recommends");
  fireEvent.click(button);
  await waitFor(() => expect(onResolve).toHaveBeenCalledWith(request, action, "", "inbox_action"));
  view.rerender(<DecisionInbox {...props} decisions={[{ ...request, state: "resolved", resolution_action: action }]} />);
  fireEvent.click(screen.getByRole("checkbox", { name: "Show history" }));
  expect(screen.getAllByText(action, { exact: true }).some(node => node.tagName === "STRONG")).toBe(true);
});

test("answers precede supporting evidence while risk remains ahead of the action", () => {
  render(<DecisionInbox decisions={[pending]} tasks={[task]} workers={[worker]} busy={false} onResolve={vi.fn()} />);
  const action = screen.getByRole("button", { name: "Durable path" });
  const evidence = screen.getByText("Why, and what it rests on");
  expect(screen.getByText(pending.risk).compareDocumentPosition(action) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(action.compareDocumentPosition(evidence) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(evidence.closest("details")).not.toHaveAttribute("open");
});

test("matching advice is shown once on the action without an empty risk divider", () => {
  const request = { ...pending, risk: "", allowed_actions: ["durable_path"], suggested_action: "durable_path" };
  const { container } = render(<DecisionInbox decisions={[request]} tasks={[]} workers={[worker]} busy={false} onResolve={vi.fn()} />);
  expect(screen.getAllByText("Durable path", { exact: true })).toHaveLength(1);
  expect(screen.getByRole("button", { name: "Durable path" })).toHaveAccessibleDescription("Petal recommends");
  expect(container.querySelector(".decision-ask")).toBeNull();
  expect(container.querySelector(".decision-card > .decision-context")).toBeNull();
  expect(screen.getByRole("button", { name: "Say something else" })).toBeVisible();
});

test("case-sensitive advice never highlights a different authored action", () => {
  render(<DecisionInbox decisions={[{ ...pending, suggested_action: "Set MODE=ReadOnly", allowed_actions: ["Set MODE=readonly"] }]} tasks={[]} workers={[worker]} busy={false} onResolve={vi.fn()} />);
  expect(screen.getByText("Set MODE=ReadOnly")).toBeVisible();
  expect(screen.getByRole("button", { name: "Set MODE=readonly" })).not.toHaveClass("primary-action");
});

test("resolved history labels advice as historical rather than a new request", () => {
  render(<DecisionInbox decisions={[{ ...resolved, urgency: "time_sensitive" }]} tasks={[]} workers={[worker]} busy={false} onResolve={vi.fn()} />);
  fireEvent.click(screen.getByRole("checkbox", { name: "Show history" }));
  expect(screen.getByText("Answered")).toBeVisible();
  expect(screen.getByText("Petal recommended")).toBeVisible();
  expect(screen.queryByText("When ready")).not.toBeInTheDocument();
  expect(screen.queryByText("Time-sensitive")).not.toBeInTheDocument();
  expect(screen.queryByText("Petal recommends")).not.toBeInTheDocument();
});

test("one shared decision lists its other tasks without duplicating the answer or permission", () => {
  const open = vi.fn();
  const shared = { id: "task-2", title: "Verify the fictional mobile flow" } as Task;
  render(<DecisionInbox decisions={[{ ...pending, linked_tasks: [{ task_id: shared.id, decision_id: pending.id, reason: "Needs the same session", created_at: 10 }] }]} tasks={[task, shared]} workers={[worker]} busy={false} onResolve={vi.fn()} onOpenTask={open} />);
  fireEvent.click(screen.getByText("Why, and what it rests on"));
  expect(screen.getByText("Also waiting on this answer")).toBeInTheDocument();
  expect(screen.getByText("This does not extend command approval to these tasks.")).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: "Durable path" })).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: shared.title }));
  expect(open).toHaveBeenCalledWith(shared.id);
});

test("explicit no preference does not invent, recommend, or submit an answer", () => {
  const onResolve = vi.fn();
  const props = { tasks: [], workers: [worker], busy: false, onResolve };
  const view = render(<DecisionInbox {...props} decisions={[{ ...pending, suggested_action: "" }]} />);
  expect(screen.getByText("No preference")).toBeVisible();
  expect(screen.queryByText(/Petal recommends/)).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "No preference" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Durable path" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Minimal path" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Say something else" })).toBeEnabled();
  expect(onResolve).not.toHaveBeenCalled();
  view.rerender(<DecisionInbox {...props} decisions={[{ ...pending, suggested_action: "durable_path" }]} />);
  expect(screen.queryByText("No preference")).not.toBeInTheDocument();
  expect(screen.getByText("Petal recommends")).toBeVisible();
});

test("long recommendations expand without changing the custom answer", () => {
  const recommendation = "Choose the durable route after verifying the migration. ".repeat(20).trim();
  render(<DecisionInbox decisions={[{ ...pending, suggested_action: recommendation }]} tasks={[]} workers={[worker]} busy={false} onResolve={vi.fn()} />);
  const custom = screen.getByRole("button", { name: "Say something else" });
  expect(custom).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(custom);
  expect(custom).toHaveAttribute("aria-expanded", "true");
  const answer = screen.getByLabelText("Tell the worker what to do instead");
  fireEvent.change(answer, { target: { value: "Investigate another approach first" } });
  expect(screen.getByText(recommendation)).toHaveClass("clamped");
  fireEvent.click(screen.getByRole("button", { name: "Show all of the recommendation" }));
  expect(screen.getByText(recommendation)).not.toHaveClass("clamped");
  expect(answer).toHaveValue("Investigate another approach first");
  expect(document.getElementById(custom.getAttribute("aria-controls")!)).toBe(screen.getByRole("group", { name: "Answer in your own words" }));
});

test("optional notes start folded without hiding the risk or choices", () => {
  const { rerender } = render(<DecisionInbox decisions={[pending]} tasks={[task]} workers={[worker]} busy={false} onResolve={vi.fn()} />);
  const note = screen.getByLabelText("Optional note");
  expect(note.closest("details")).not.toHaveAttribute("open");
  expect(screen.getByText(pending.risk)).toBeVisible();
  expect(screen.getByRole("button", { name: "Durable path" })).toBeVisible();
  fireEvent.change(note, { target: { value: "Keep my condition" } });
  expect(screen.getByText("Edit your note")).toBeInTheDocument();
  rerender(<DecisionInbox decisions={[pending]} tasks={[task]} workers={[worker]} busy={true} onResolve={vi.fn()} />);
  expect(screen.getByLabelText("Optional note")).toBeDisabled();
  expect(screen.getByLabelText("Optional note")).toHaveValue("Keep my condition");
});

test("names the repository the decision is about, and the ask, without opening the argument", () => {
  // The reported card: Queen raising an approval about another repository. Her
  // own workspace is the Queen directory, so the requester is the wrong source
  // for "which repo is this" — the linked task is the right one.
  const queen = { id: "queen-1", name: "Queen", workspace: "/home/bee/workspaces/queen" } as Worker;
  const platformTask = { id: "task-9", title: "CI never builds packages/app-logger", workspace: "/home/bee/projects/rcg-platform" } as Task;
  const approval = {
    ...pending,
    id: "decision-9",
    requesting_worker_id: "queen-1",
    task_id: "task-9",
    kind: "approval",
    title: "Merge PR #419?",
    suggested_action: "Merge PR #419",
  } as DecisionRequest;

  render(<DecisionInbox decisions={[approval]} tasks={[platformTask]} workers={[queen]} busy={false} onResolve={vi.fn()} />);

  expect(screen.getByText("rcg-platform")).toBeInTheDocument();
  expect(screen.getByTitle("/home/bee/projects/rcg-platform")).toBeInTheDocument();
  expect(screen.queryByText("queen")).not.toBeInTheDocument();

  // The ask is on the card, not the last row of a list under a folded details.
  const ask = screen.getByText(/recommends$/).closest(".decision-ask");
  expect(ask).toHaveTextContent("Merge PR #419");
  expect(ask?.closest("details")).toBeNull();
});

test("falls back to the requesting worker's repository when no task is linked", () => {
  const petal = { id: "worker-1", name: "Petal", workspace: "/home/bee/projects/rcg-admin/" } as Worker;
  const untied = { ...pending, id: "decision-10", task_id: null } as DecisionRequest;

  render(<DecisionInbox decisions={[untied]} tasks={[]} workers={[petal]} busy={false} onResolve={vi.fn()} />);

  expect(screen.getByText("rcg-admin")).toBeInTheDocument();
});

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

  fireEvent.click(screen.getByRole("checkbox", { name: "Show history" }));
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

test.each([false, true])("reveals and focuses a resolved decision with reduced motion=%s", async (reduce) => {
  vi.stubGlobal("matchMedia", vi.fn().mockImplementation((query: string) => ({ matches: query === "(prefers-reduced-motion: reduce)" && reduce, addEventListener: vi.fn(), removeEventListener: vi.fn() })));
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
  expect(screen.getByRole("checkbox", { name: "Show history" })).toBeChecked();
  expect(scrollIntoView).toHaveBeenCalledWith({ behavior: reduce ? "instant" : "smooth", block: "center" });
});

test.each(["resolved", "withdrawn"] as const)("search navigation opens %s history without answer controls", async (state) => {
  const onResolve = vi.fn();
  render(<DecisionInbox decisions={[{ ...pending, state }]} tasks={[]} workers={[]} busy={false}
    focusDecisionId={pending.id} focusRequest={1} onResolve={onResolve} />);
  await waitFor(() => expect(screen.getByRole("article")).toHaveFocus());
  expect(screen.getByRole("checkbox", { name: "Show history" })).toBeChecked();
  expect(screen.getByText(state === "resolved" ? "Answered" : "Withdrawn", { selector: ".decision-urgency" })).toBeVisible();
  expect(screen.queryByRole("button", { name: /Something else/ })).not.toBeInTheDocument();
  expect(onResolve).not.toHaveBeenCalled();
});

test("decision navigation leaves Activity and focuses once after the request arrives", async () => {
  const scrollIntoView = vi.fn();
  Element.prototype.scrollIntoView = scrollIntoView;
  const props = { tasks: [], workers: [], busy: false, onResolve: vi.fn() };
  const { rerender } = render(<DecisionInbox {...props} decisions={[]} />);
  fireEvent.click(screen.getByRole("tab", { name: "Activity" }));
  rerender(<DecisionInbox {...props} decisions={[]} focusDecisionId={pending.id} focusRequest={1} />);
  expect(screen.getByRole("tab", { name: "Needs you 0" })).toHaveAttribute("aria-selected", "true");
  rerender(<DecisionInbox {...props} decisions={[pending]} focusDecisionId={pending.id} focusRequest={1} />);
  await waitFor(() => expect(screen.getByRole("article")).toHaveFocus());
  expect(scrollIntoView).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("tab", { name: "Activity" }));
  rerender(<DecisionInbox {...props} decisions={[{ ...pending }]} focusDecisionId={pending.id} focusRequest={1} />);
  expect(screen.getByRole("tab", { name: "Activity" })).toHaveAttribute("aria-selected", "true");
  expect(scrollIntoView).toHaveBeenCalledTimes(1);
  rerender(<DecisionInbox {...props} decisions={[pending]} focusDecisionId={pending.id} focusRequest={2} />);
  // Preserved forms can retain jsdom's activeElement while hidden; wait for the
  // new navigation frame itself, not an already-true focus assertion.
  await waitFor(() => {
    expect(scrollIntoView).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("article")).toHaveFocus();
  });
});

test("attention tabs support manual keyboard activation without starting reads on focus", () => {
  const onFetchActivity = vi.fn().mockResolvedValue({ entries: [], next_before: null });
  render(<DecisionInbox decisions={[]} tasks={[]} workers={[]} busy={false} onResolve={vi.fn()} onFetchActivity={onFetchActivity} />);
  const attention = screen.getByRole("tab", { name: "Needs you 0" });
  const activity = screen.getByRole("tab", { name: "Activity" });
  attention.focus();
  fireEvent.keyDown(attention, { key: "ArrowRight" });
  expect(activity).toHaveFocus();
  expect(attention).toHaveAttribute("aria-selected", "true");
  expect(activity).toHaveAttribute("tabindex", "-1");
  expect(onFetchActivity).not.toHaveBeenCalled();
  fireEvent.keyDown(activity, { key: "Home" });
  expect(attention).toHaveFocus();
  fireEvent.keyDown(attention, { key: "End" });
  expect(activity).toHaveFocus();
  fireEvent.click(activity);
  expect(activity).toHaveAttribute("tabindex", "0");
  expect(attention).toHaveAttribute("tabindex", "-1");
  expect(screen.getByRole("tabpanel", { name: "Activity" })).toHaveAttribute("id", activity.getAttribute("aria-controls"));
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

test("names which control the operator used, so a disputed answer can be traced", async () => {
  // A decision was recorded with an action the operator says they did not
  // choose, and nothing captured where the answer arrived from.
  const onResolve = vi.fn().mockResolvedValue(undefined);
  render(
    <DecisionInbox decisions={[pending]} tasks={[task]} workers={[worker]} busy={false} onResolve={onResolve} />,
  );

  fireEvent.click(screen.getByRole("button", { name: "Durable path" }));
  expect(onResolve).toHaveBeenCalledWith(pending, "durable_path", "", "inbox_action");

  await waitFor(() => expect(screen.getByRole("button", { name: "Dismiss request" })).toBeEnabled());
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
  expect(screen.getByText(pending.evidence).closest("details")).toBe(argument.closest("details"));
  expect(screen.getByText(pending.risk).closest("details")).toBeNull();
});

test("a failed custom answer remains visible and can be retried without choosing a quick action", async () => {
  const onAnswer = vi.fn().mockRejectedValueOnce(new Error("Connection interrupted; answer not saved")).mockResolvedValueOnce(undefined);
  const onResolve = vi.fn();
  render(<DecisionInbox decisions={[pending]} tasks={[task]} workers={[worker]} busy={false} onResolve={onResolve} onAnswer={onAnswer} />);
  fireEvent.click(screen.getByRole("button", { name: "Say something else" }));
  fireEvent.change(screen.getByLabelText("Tell the worker what to do instead"), { target: { value: "Do neither; investigate the third route first." } });
  fireEvent.click(screen.getByRole("button", { name: "Send this instead" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Connection interrupted");
  expect(screen.getByLabelText("Tell the worker what to do instead")).toHaveValue("Do neither; investigate the third route first.");
  fireEvent.click(screen.getByRole("button", { name: "Send this instead" }));
  await waitFor(() => expect(screen.queryByLabelText("Tell the worker what to do instead")).not.toBeInTheDocument());
  expect(onAnswer).toHaveBeenCalledTimes(2);
  expect(onResolve).not.toHaveBeenCalled();
});

test("pending custom answers disable competing decisions and never grant an exact command", async () => {
  let finish!: () => void;
  const onAnswer = vi.fn(() => new Promise<void>(resolve => { finish = resolve; }));
  const onResolve = vi.fn();
  render(<DecisionInbox decisions={[{ ...pending, requested_command: "demo-only-command" }]} tasks={[task]} workers={[worker]} busy={false} onResolve={onResolve} onAnswer={onAnswer} />);
  fireEvent.click(screen.getByRole("button", { name: "Say something else" }));
  fireEvent.change(screen.getByLabelText("Tell the worker what to do instead"), { target: { value: "Do not run this command. Explain the alternative." } });
  fireEvent.click(screen.getByRole("button", { name: "Send this instead" }));
  expect(screen.getByRole("button", { name: "Send this instead" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Durable path" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Dismiss request" })).toBeDisabled();
  expect(screen.getByLabelText("Tell the worker what to do instead")).toBeDisabled();
  expect(onResolve).not.toHaveBeenCalled();
  finish();
  await waitFor(() => expect(screen.queryByLabelText("Tell the worker what to do instead")).not.toBeInTheDocument());
  expect(onAnswer).toHaveBeenCalledOnce();
});

test("a view without answer transport reports a failure instead of silently consuming custom text", async () => {
  render(<DecisionInbox decisions={[pending]} tasks={[task]} workers={[worker]} busy={false} onResolve={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Say something else" }));
  fireEvent.change(screen.getByLabelText("Tell the worker what to do instead"), { target: { value: "Keep my answer" } });
  fireEvent.click(screen.getByRole("button", { name: "Send this instead" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Answers cannot be sent from this view");
  expect(screen.getByLabelText("Tell the worker what to do instead")).toHaveValue("Keep my answer");
});

test("a late failed send cannot resurrect an answer resolved by a newer snapshot", async () => {
  let rejectSend!: (reason: Error) => void;
  const onAnswer = vi.fn(() => new Promise<void>((_resolve, reject) => { rejectSend = reject; }));
  const props = { tasks: [task], workers: [worker], busy: false, onResolve: vi.fn(), onAnswer };
  const view = render(<DecisionInbox {...props} decisions={[pending]} />);
  fireEvent.click(screen.getByRole("button", { name: "Say something else" }));
  fireEvent.change(screen.getByLabelText("Tell the worker what to do instead"), { target: { value: "Prepare a preview first" } });
  fireEvent.click(screen.getByRole("button", { name: "Send this instead" }));
  view.rerender(<DecisionInbox {...props} decisions={[{ ...pending, state: "resolved", resolution_action: "answered", resolution_note: "Prepare a preview first", delivery_state: "queued" }]} />);
  rejectSend(new Error("The old response was lost"));
  await waitFor(() => expect(screen.queryByRole("textbox", { name: "Tell the worker what to do instead" })).not.toBeInTheDocument());
  fireEvent.click(screen.getByRole("checkbox", { name: "Show history" }));
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(screen.getByText("Waiting for a quiet moment")).toBeVisible();
  expect(screen.queryByRole("button", { name: "Say something else" })).not.toBeInTheDocument();
  expect(onAnswer).toHaveBeenCalledOnce();
});

test("long summaries expand without hiding the ask or losing source text", () => {
  const summary = "A long explanation of the decision. ".repeat(20);
  render(<DecisionInbox decisions={[{ ...pending, summary }]} tasks={[task]} workers={[worker]} busy={false} onResolve={vi.fn()} />);
  expect(screen.getByText(pending.suggested_action, { exact: false })).toBeInTheDocument();
  expect(screen.getByText(summary.trim())).toHaveClass("clamped");
  fireEvent.click(screen.getByRole("button", { name: "Show all of the summary" }));
  expect(screen.getByText(summary.trim())).not.toHaveClass("clamped");
});

test("risk preview stays visible and expands without losing the decision or draft", () => {
  const risk = "Changing this field affects existing records. ".repeat(12).trim();
  render(<DecisionInbox decisions={[{ ...pending, risk }]} tasks={[task]} workers={[worker]} busy={false} onResolve={vi.fn()} />);
  const preview = screen.getByText(risk);
  expect(preview.closest(".decision-risk")).not.toBeNull();
  expect(preview.closest("details")).toBeNull();
  expect(preview).toHaveClass("clamped");
  fireEvent.change(screen.getByLabelText("Optional note"), { target: { value: "Preserve existing values" } });
  fireEvent.click(screen.getByRole("button", { name: "Show all of the risk" }));
  expect(preview).not.toHaveClass("clamped");
  expect(screen.getByRole("button", { name: "Durable path" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Show less of the risk" }));
  expect(preview).toHaveClass("clamped");
  expect(screen.getByLabelText("Optional note")).toHaveValue("Preserve existing values");
});

test("action emphasis follows the stated suggestion rather than array order", () => {
  const props = { tasks: [task], workers: [worker], busy: false, onResolve: vi.fn() };
  const view = render(<DecisionInbox {...props} decisions={[{ ...pending, suggested_action: "minimal_path" }]} />);
  expect(screen.getByRole("button", { name: "Minimal path" })).toHaveClass("primary-action");
  expect(screen.getByRole("button", { name: "Durable path" })).not.toHaveClass("primary-action");
  view.rerender(<DecisionInbox {...props} decisions={[{ ...pending, suggested_action: "Ask a follow-up first" }]} />);
  expect(screen.getByRole("button", { name: "Minimal path" })).not.toHaveClass("primary-action");
  expect(screen.getByRole("button", { name: "Durable path" })).not.toHaveClass("primary-action");
});

test("pending notes survive refresh but a resolved request releases its draft", () => {
  const props = { tasks: [task], workers: [worker], busy: false, onResolve: vi.fn() };
  const view = render(<DecisionInbox {...props} decisions={[pending]} />);
  fireEvent.change(screen.getByLabelText("Optional note"), { target: { value: "Keep my context" } });
  view.rerender(<DecisionInbox {...props} decisions={[{ ...pending, updated_at: 5 }]} />);
  expect(screen.getByLabelText("Optional note")).toHaveValue("Keep my context");
  view.rerender(<DecisionInbox {...props} decisions={[{ ...pending, state: "resolved" }]} />);
  expect(screen.queryByLabelText("Optional note")).not.toBeInTheDocument();
  view.rerender(<DecisionInbox {...props} decisions={[pending]} />);
  expect(screen.getByLabelText("Optional note")).toHaveValue("");
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

/**
 * Item 48's second door, on the operator's ruling. Three summary cards sit
 * above the list and each appears on live state, so any of them mounting shoved
 * every decision down by a card — between the operator reading an action and
 * pressing it, which is the same harm the inserted-decision fix addressed.
 */
test("a card appearing does not move the decision list", () => {
  const decisions = [pending, { ...pending, id: "decision-9" }];
  const { rerender, container } = render(
    <DecisionInbox
      decisions={decisions}
      tasks={[]}
      workers={[]}
      busy={false}
      attentionCards={null}
      onResolve={vi.fn()}
    />,
  );
  const region = container.querySelector(".decision-attention-cards");
  expect(region).toHaveClass("reserved");

  // The same region, now holding a card. Its height is fixed, so the list
  // below it has not moved.
  rerender(
    <DecisionInbox
      decisions={decisions}
      tasks={[]}
      workers={[]}
      busy={false}
      attentionCards={<div className="queen-attention-card">Queen needs a look</div>}
      onResolve={vi.fn()}
    />,
  );
  expect(container.querySelector(".decision-attention-cards")).toHaveClass("reserved");
  expect(screen.getByText("Queen needs a look")).toBeInTheDocument();
});

/** With nothing waiting there is nothing below to shove, and blank space above
 *  "Nothing needs your attention" would be the oddity the other layout was
 *  rejected for. */
test("reserves nothing when the queue is empty", () => {
  const { container } = render(
    <DecisionInbox decisions={[]} tasks={[]} workers={[]} busy={false} attentionCards={null} onResolve={vi.fn()} />,
  );
  expect(container.querySelector(".decision-attention-cards")).not.toHaveClass("reserved");
  expect(screen.getByText("Nothing needs your attention")).toBeInTheDocument();
});

test("a decision that would grant a command shows the operator that command", () => {
  // THE SAFETY PROPERTY OF THE WHOLE GRANT MECHANISM. One of these buttons
  // turns the approval into a permission rule the classifier obeys. Approving
  // "Allow the command shown in this request" with no command shown is
  // approving something you cannot read, which is worse than the block it
  // removes — so the command renders, in full, beside the button.
  const command =
    "curl -sS -X POST 'https://example.crm.dynamics.com/api/data/v9.2/EntityDefinitions(LogicalName='\"'\"'contact'\"'\"')/Attributes'";
  const granting: DecisionRequest = {
    ...pending,
    id: "decision-grant",
    kind: "approval",
    questions: [],
    requested_command: command,
    allowed_actions: ["Do not run it", "Allow the command shown in this request"],
  };
  render(<DecisionInbox decisions={[granting]} tasks={[]} workers={[]} busy={false} onResolve={vi.fn()} />);

  expect(screen.getByText("Command this would allow")).toBeInTheDocument();
  // The WHOLE command, not a truncation: an operator approving the first half
  // of a command has not read what they are allowing.
  expect(screen.getByText(command)).toBeInTheDocument();
  expect(
    screen.getByText(/Grants permission for this exact command, once, for this worker only/),
  ).toBeInTheDocument();
});

test("an ordinary decision shows no command block", () => {
  // A grant panel on a decision that grants nothing is noise, and noise is how
  // an operator learns to skip past the one card where it mattered.
  render(<DecisionInbox decisions={[pending]} tasks={[]} workers={[]} busy={false} onResolve={vi.fn()} />);
  expect(screen.queryByText("Command this would allow")).not.toBeInTheDocument();
});

/**
 * ORDER IS THE RANKING. The operator's screenshot, 2026-08-28, showed the
 * "Briefings waiting their turn" panel — whose own copy says "Nothing is wrong
 * with these" — sitting ABOVE a Queen request asking four questions that block
 * work. Everything in attentionCards renders above the list and therefore
 * claims to outrank it; a panel that is not asking anything must not.
 */
test("panels that are not asking anything render below the requests", () => {
  const { container } = render(
    <DecisionInbox
      decisions={[pending]}
      tasks={[]}
      workers={[]}
      busy={false}
      attentionCards={<p data-testid="urgent">Someone is waiting</p>}
      trailingCards={<p data-testid="benign">Nothing is wrong with these</p>}
      onResolve={vi.fn()}
    />,
  );

  const urgent = container.querySelector('[data-testid="urgent"]');
  const list = container.querySelector(".decision-list");
  const benign = container.querySelector('[data-testid="benign"]');
  expect(urgent).not.toBeNull();
  expect(list).not.toBeNull();
  expect(benign).not.toBeNull();

  // DOCUMENT_POSITION_FOLLOWING: the node passed in comes later in the document.
  if (!urgent || !list || !benign) throw new Error("the three regions must all render");
  expect(urgent.compareDocumentPosition(list) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(list.compareDocumentPosition(benign) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});

/**
 * A refused terminal answer says so on the card the operator is looking at.
 *
 * ⚠️ THE WHOLE COMPLAINT IS "I ANSWERED AND NOTHING HAPPENED". Someone reaching
 * this card believes the question is already settled; if the refusal is not on
 * it, their experience is identical to the bug — and worse, the code now
 * believes it is working.
 */
test("tells the operator when an answer typed in a terminal could not be used", () => {
  render(<DecisionInbox decisions={[{
    ...pending,
    refused_native_answer: { reason: "ambiguous", seen_at: 100 },
  }]} tasks={[]} workers={[]} busy={false} onResolve={vi.fn()} />);

  expect(screen.getByText(/An answer was typed in this worker's terminal/)).toBeInTheDocument();
  expect(screen.getByText(/cannot tell which one you answered/)).toBeInTheDocument();
});

test("says nothing about refused answers when none was refused", () => {
  render(<DecisionInbox decisions={[pending]} tasks={[]} workers={[]} busy={false} onResolve={vi.fn()} />);

  expect(screen.queryByText(/An answer was typed in this worker's terminal/)).not.toBeInTheDocument();
});

/**
 * A resolved question keeps its notice to itself.
 *
 * The notice is an instruction to act. On a question that is already settled it
 * would be an alarm about finished work, which is the failure mode on the other
 * side of this feature.
 */
test("does not carry a refusal notice onto a question that is already resolved", () => {
  // A refusal can be recorded while the question is pending and outlive it:
  // the operator answers here instead, and the row stays. On the Activity tab
  // that notice would tell them to act on something already settled.
  render(<DecisionInbox decisions={[{
    ...resolved,
    refused_native_answer: { reason: "unverified", seen_at: 100 },
  }]} tasks={[]} workers={[]} busy={false} onResolve={vi.fn()} />);
  // Resolved work is behind "Show history", so the card has to be ON SCREEN for
  // this to be asserting anything at all. Without this the test passed against a
  // component that renders the notice unconditionally.
  fireEvent.click(screen.getByRole("checkbox", { name: "Show history" }));
  expect(screen.getByText("Approve release")).toBeInTheDocument();

  expect(screen.queryByText(/An answer was typed in this worker's terminal/)).not.toBeInTheDocument();
});
