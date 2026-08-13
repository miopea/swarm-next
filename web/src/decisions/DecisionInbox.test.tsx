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
