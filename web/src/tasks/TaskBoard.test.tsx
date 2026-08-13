import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { Task, Worker } from "../api";
import TaskBoard from "./TaskBoard";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const task: Task = {
  id: "task-1",
  hive_id: "hive-1", title: "Make reload stable", workspace: "/workspace/swarm", state: "draft",
  description: "Keep terminal history attached", priority: "high",
  assigned_worker_id: null, assigned_session_id: null, position: 0, created_at: 1, updated_at: 1,
};
const worker: Worker = {
  id: "worker-1", hive_id: "hive-1", name: "Daisy", role: "worker", provider: "claude_code",
  workspace: "/workspace/swarm", autostart: false, position: 1, active_session_id: null,
  created_at: 1, updated_at: 1, running: false, attention_state: "sleeping",
};

function renderBoard(overrides: Partial<React.ComponentProps<typeof TaskBoard>> = {}) {
  const props: React.ComponentProps<typeof TaskBoard> = {
    tasks: [task], jiraTaskLinks: [], operatorToken: "operator-token", sessions: [], workers: [worker], busy: false,
    onCreate: vi.fn(), onUpdate: vi.fn(), onTransition: vi.fn(), onAssign: vi.fn(), onStartWorker: vi.fn(), onOpenWorker: vi.fn(), onFetchActivity: vi.fn().mockResolvedValue({ events: [], truncated: false }), onFetchJiraComments: vi.fn().mockResolvedValue([]), onAddJiraComment: vi.fn().mockResolvedValue({ state: "delivered" }), onRetryJira: vi.fn(), onJiraImported: vi.fn().mockResolvedValue(undefined), onReorder: vi.fn(),
    ...overrides,
  };
  return { props, ...render(<TaskBoard {...props} />) };
}

test("keeps active work above the fold on phones until task creation is requested", async () => {
  vi.stubGlobal("matchMedia", vi.fn().mockReturnValue({ matches: true }));
  renderBoard({ tasks: [] });

  expect(screen.queryByLabelText("Task title")).not.toBeInTheDocument();
  const toggle = screen.getByRole("button", { name: "Create a task" });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(toggle);
  const title = screen.getByLabelText("Task title");
  expect(title).toBeInTheDocument();
  await waitFor(() => expect(title).toHaveFocus());
  expect(screen.getByRole("button", { name: "Hide task form" })).toHaveAttribute("aria-expanded", "true");
});

test("opens and focuses task creation when requested from global navigation", async () => {
  vi.stubGlobal("matchMedia", vi.fn().mockReturnValue({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  const { props, rerender } = renderBoard({ tasks: [], composeRequest: 0 });

  expect(screen.queryByLabelText("Task title")).not.toBeInTheDocument();
  rerender(<TaskBoard {...props} composeRequest={1} />);

  const title = await screen.findByLabelText("Task title");
  await waitFor(() => expect(title).toHaveFocus());
  expect(screen.getByRole("button", { name: "Hide task form" })).toHaveAttribute("aria-expanded", "true");
});

test("reveals and focuses a completed task selected through global navigation", async () => {
  const completed = { ...task, state: "completed" as const };
  const scrollIntoView = vi.fn();
  Element.prototype.scrollIntoView = scrollIntoView;
  renderBoard({ tasks: [completed], focusTaskId: completed.id, focusRequest: 1 });

  const card = screen.getByRole("article", { name: completed.title });
  await waitFor(() => expect(card).toHaveFocus());
  expect(card.closest("details")).toHaveAttribute("open");
  expect(scrollIntoView).toHaveBeenCalled();
});

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
  fireEvent.change(screen.getByLabelText("Who should handle this?"), { target: { value: worker.id } });
  fireEvent.click(screen.getByRole("button", { name: "Create draft" }));

  expect(onCreate).toHaveBeenCalledWith({
    title: "Ship task editing",
    description: "Keep failed forms open",
    priority: "urgent",
    worker_id: worker.id,
  });
});

test("keeps worker ownership visible while the assigned worker is sleeping", () => {
  const sameWorkspace = { ...worker, id: "worker-2", name: "Poppy" };
  renderBoard({
    tasks: [{ ...task, state: "ready", assigned_worker_id: worker.id }],
    workers: [sameWorkspace, worker],
  });

  const card = screen.getByRole("article", { name: task.title });
  expect(within(card).getByText("Assigned", { selector: ".task-state" })).toBeInTheDocument();
  expect(within(card).queryByText("In progress", { selector: ".task-state" })).not.toBeInTheDocument();
  expect(within(card).getByText("Daisy · swarm")).toBeInTheDocument();
  expect(within(card).getByLabelText("Worker")).toHaveValue(worker.id);
});

test("distinguishes assigned work from work that has started", () => {
  const assignedReady = { ...task, state: "ready" as const, assigned_worker_id: worker.id };
  const active = { ...assignedReady, id: "task-2", title: "Actively implement reload", state: "active" as const };
  renderBoard({ tasks: [assignedReady, active] });

  expect(within(screen.getByRole("article", { name: assignedReady.title })).getByText("Assigned", { selector: ".task-state" })).toBeInTheDocument();
  expect(within(screen.getByRole("article", { name: active.title })).getByText("In progress", { selector: ".task-state" })).toBeInTheDocument();
});

test("keeps Jira identity and remote status visible while routing work", () => {
  const onRetryJira = vi.fn();
  renderBoard({
    tasks: [{ ...task, state: "ready", assigned_worker_id: worker.id }],
    jiraTaskLinks: [{
      issue_id: "20001", issue_key: "WEB-42", issue_url: "https://jira.example.test/browse/WEB-42", binding_id: "binding-1",
      project_key: "WEB", project_name: "Website Services", task_id: task.id,
      jira_status_id: "1", jira_status_name: "To Do",
      jira_assignee_account_id: "account-1", jira_assignee_name: "Bradford",
      remote_updated_at: "2026-08-13T13:00:00Z", last_synced_at: 1, outbound_state: "conflict",
    }],
    onRetryJira,
  });

  const card = screen.getByRole("article", { name: task.title });
  expect(within(card).getByLabelText("Jira issue WEB-42")).toHaveTextContent("Website Services");
  expect(within(card).getByLabelText("Jira issue WEB-42")).toHaveTextContent("To Do");
  expect(within(card).getByLabelText("Jira issue WEB-42")).toHaveTextContent("Jira: Bradford");
  expect(within(card).getByLabelText("Jira issue WEB-42")).toHaveTextContent("Jira update needs attention");
  fireEvent.click(within(card).getByRole("button", { name: "Retry Jira" }));
  expect(onRetryJira).toHaveBeenCalledWith(expect.objectContaining({ id: task.id }));
  expect(within(card).getByRole("link", { name: /WEB-42/ })).toHaveAttribute("href", "https://jira.example.test/browse/WEB-42");
  expect(within(card).getByText("Assigned", { selector: ".task-state" })).toBeInTheDocument();
});

test("reads and posts Jira discussion without leaving the task", async () => {
  const onFetchJiraComments = vi.fn().mockResolvedValue([{
    id: "comment-1", author_name: "Bea", body: "Ready for review",
    created_at: "2026-08-13T13:00:00Z", updated_at: "2026-08-13T13:00:00Z",
  }]);
  const onAddJiraComment = vi.fn().mockResolvedValue({ state: "delivered" });
  renderBoard({
    jiraTaskLinks: [{
      issue_id: "20001", issue_key: "WEB-42", issue_url: "https://jira.example.test/browse/WEB-42", binding_id: "binding-1",
      project_key: "WEB", project_name: "Website Services", task_id: task.id,
      jira_status_id: "1", jira_status_name: "To Do", jira_assignee_account_id: "account-1", jira_assignee_name: "Bradford",
      remote_updated_at: "2026-08-13T13:00:00Z", last_synced_at: 1, outbound_state: null,
    }],
    onFetchJiraComments,
    onAddJiraComment,
  });

  fireEvent.click(screen.getByRole("button", { name: "Discussion" }));
  const discussion = await screen.findByRole("region", { name: "Jira discussion for WEB-42" });
  expect(within(discussion).getByText("Ready for review")).toBeInTheDocument();
  fireEvent.change(within(discussion).getByLabelText("Add an update"), { target: { value: "Shipped cleanly" } });
  fireEvent.click(within(discussion).getByRole("button", { name: "Share to Jira" }));
  await waitFor(() => expect(onAddJiraComment).toHaveBeenCalledWith(task.id, "Shipped cleanly"));
  expect(within(discussion).getByText("Shared to Jira.")).toBeInTheDocument();
  expect(onFetchJiraComments).toHaveBeenCalledTimes(2);
});

test("opens the assigned running worker directly from her task", () => {
  const onOpenWorker = vi.fn();
  const runningWorker = { ...worker, active_session_id: "session-1", running: true };
  renderBoard({
    tasks: [{ ...task, assigned_worker_id: worker.id, assigned_session_id: "session-1" }],
    sessions: [{ session_id: "session-1", running: true }],
    workers: [runningWorker],
    onOpenWorker,
  });

  fireEvent.click(screen.getByRole("button", { name: "Daisy · swarm" }));
  expect(onOpenWorker).toHaveBeenCalledWith("session-1");
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
  }));
  expect(screen.getByRole("form", { name: `Edit ${task.title}` })).toBeInTheDocument();
});

test("loads task history only when the operator opens it", async () => {
  const onFetchActivity = vi.fn().mockResolvedValue({
    events: [
      { sequence: 1, task_id: task.id, kind: "created", from_state: null, to_state: "draft", note: "", occurred_at: 1_700_000_000 },
      { sequence: 2, task_id: task.id, kind: "state_changed", from_state: "draft", to_state: "ready", note: "Ready for Petal.", occurred_at: 1_700_000_060 },
    ],
    truncated: true,
  });
  renderBoard({ onFetchActivity });

  expect(onFetchActivity).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: `Actions for ${task.title}` }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Show history" }));

  await waitFor(() => expect(onFetchActivity).toHaveBeenCalledWith(task.id));
  expect(screen.getByRole("region", { name: "Task history" })).toHaveTextContent("Task created");
  expect(screen.getByRole("region", { name: "Task history" })).toHaveTextContent("Draft → Ready");
  expect(screen.getByRole("region", { name: "Task history" })).toHaveTextContent("Showing the latest activity.");
});

test("moves open tasks with keyboard-accessible ordering controls", () => {
  const second = { ...task, id: "task-2", title: "Second task", position: 1 };
  const onReorder = vi.fn().mockResolvedValue(undefined);
  renderBoard({ tasks: [second, task], onReorder });

  fireEvent.contextMenu(screen.getByRole("article", { name: task.title }));
  expect(screen.getByRole("menu", { name: `${task.title} actions` })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("menuitem", { name: "Move later" }));

  expect(onReorder).toHaveBeenCalledWith([second.id, task.id]);
  expect(screen.queryByRole("menu", { name: `${task.title} actions` })).not.toBeInTheDocument();

  onReorder.mockClear();
  const dataTransfer = { effectAllowed: "none", setData: vi.fn() };
  fireEvent.dragStart(screen.getByRole("article", { name: second.title }), { dataTransfer });
  fireEvent.drop(screen.getByRole("article", { name: task.title }), { dataTransfer });
  expect(onReorder).toHaveBeenCalledWith([second.id, task.id]);
});
test.each([
  ["queued", "Briefing waits for a quiet moment"],
  ["dispatching", "Briefing worker"],
  ["delivered", "Worker briefed"],
  ["uncertain", "Briefing uncertain — task remains authoritative"],
] as const)("renders the %s task briefing state", (dispatchState, label) => {
  renderBoard({
    tasks: [{ ...task, assigned_session_id: "session-1", dispatch_state: dispatchState }],
    sessions: [{ session_id: "session-1", running: true }],
  });

  expect(screen.getByRole("status")).toHaveTextContent(label);
});
test("shows Queen handoff state and its durable history note", async () => {
  const onFetchActivity = vi.fn().mockResolvedValue({
    events: [{
      sequence: 3, task_id: task.id, kind: "state_changed", from_state: "active",
      to_state: "review", note: "Android voice and shortcuts verified.", occurred_at: 1_700_000_120,
    }],
    truncated: false,
  });
  renderBoard({
    tasks: [{ ...task, state: "review", assigned_session_id: "session-1", outcome_delivery_state: "delivered" }],
    sessions: [{ session_id: "session-1", running: true }],
    onFetchActivity,
  });

  expect(screen.getByRole("status")).toHaveTextContent("Queen notified");
  fireEvent.click(screen.getByRole("button", { name: `Actions for ${task.title}` }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Show history" }));
  await waitFor(() => expect(screen.getByText("Android voice and shortcuts verified.")).toBeInTheDocument());
});
