import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

afterEach(cleanup);

import type { Task } from "../api";
import TaskBoard from "./TaskBoard";

const task: Task = {
  id: "task-1",
  hive_id: "hive-1", title: "Make reload stable", workspace: "/workspace/swarm", state: "draft",
  description: "Keep terminal history attached", priority: "high",
  assigned_session_id: null, created_at: 1, updated_at: 1,
};

function renderBoard(overrides: Partial<React.ComponentProps<typeof TaskBoard>> = {}) {
  const props: React.ComponentProps<typeof TaskBoard> = {
    tasks: [task], sessions: [], workerNames: new Map(), busy: false,
    onCreate: vi.fn(), onUpdate: vi.fn(), onTransition: vi.fn(), onAssign: vi.fn(), onStartWorker: vi.fn(), onFetchActivity: vi.fn().mockResolvedValue({ events: [], truncated: false }),
    ...overrides,
  };
  render(<TaskBoard {...props} />);
  return props;
}

test("dragging a task exposes only legal workflow targets and performs the drop", () => {
  const onTransition = vi.fn().mockResolvedValue(undefined);
  renderBoard({ onTransition });
  const dataTransfer = { effectAllowed: "none", setData: vi.fn() };

  fireEvent.dragStart(screen.getByRole("article", { name: task.title }), { dataTransfer });

  expect(screen.getByText(task.title, { selector: ".task-drop-strip strong" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Ready" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "In progress" })).not.toBeInTheDocument();

  fireEvent.drop(screen.getByRole("button", { name: "Ready" }), { dataTransfer });
  expect(onTransition).toHaveBeenCalledWith(task, "ready");
  expect(screen.queryByText(task.title, { selector: ".task-drop-strip strong" })).not.toBeInTheDocument();
});

test("creates a task with useful context and priority", () => {
  const onCreate = vi.fn().mockResolvedValue(undefined);
  renderBoard({ tasks: [], onCreate });

  fireEvent.change(screen.getByLabelText("Task title"), { target: { value: "Ship task editing" } });
  fireEvent.change(screen.getByLabelText(/Description/), { target: { value: "Keep failed forms open" } });
  fireEvent.change(screen.getByLabelText("Priority"), { target: { value: "urgent" } });
  fireEvent.change(screen.getByLabelText("Workspace"), { target: { value: "/workspace/swarm" } });
  fireEvent.click(screen.getByRole("button", { name: "Create draft" }));

  expect(onCreate).toHaveBeenCalledWith({
    title: "Ship task editing",
    description: "Keep failed forms open",
    priority: "urgent",
    workspace: "/workspace/swarm",
  });
});

test("edits task details and retains a failed form for retry", async () => {
  const onUpdate = vi.fn().mockRejectedValue(new Error("offline"));
  renderBoard({ onUpdate });

  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  const editForm = screen.getByRole("form", { name: `Edit ${task.title}` });
  fireEvent.change(within(editForm).getByLabelText("Title"), { target: { value: "Make every reload stable" } });
  fireEvent.change(within(editForm).getByLabelText("Priority"), { target: { value: "urgent" } });
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));

  await waitFor(() => expect(onUpdate).toHaveBeenCalledWith(task, {
    title: "Make every reload stable",
    description: task.description,
    priority: "urgent",
    workspace: task.workspace,
  }));
  expect(screen.getByRole("form", { name: `Edit ${task.title}` })).toBeInTheDocument();
});

test("loads task history only when the operator opens it", async () => {
  const onFetchActivity = vi.fn().mockResolvedValue({
    events: [
      { sequence: 1, task_id: task.id, kind: "created", from_state: null, to_state: "draft", occurred_at: 1_700_000_000 },
      { sequence: 2, task_id: task.id, kind: "state_changed", from_state: "draft", to_state: "ready", occurred_at: 1_700_000_060 },
    ],
    truncated: true,
  });
  renderBoard({ onFetchActivity });

  expect(onFetchActivity).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "History" }));

  await waitFor(() => expect(onFetchActivity).toHaveBeenCalledWith(task.id));
  expect(screen.getByRole("region", { name: "Task history" })).toHaveTextContent("Task created");
  expect(screen.getByRole("region", { name: "Task history" })).toHaveTextContent("Draft → Ready");
  expect(screen.getByRole("region", { name: "Task history" })).toHaveTextContent("Showing the latest activity.");
});
