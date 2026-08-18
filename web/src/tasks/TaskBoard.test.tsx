import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { HiveIdentity, Task, Worker } from "../api";
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
const jiraTaskLink = {
  issue_id: "20001", issue_key: "WEB-42", issue_url: "https://jira.example.test/browse/WEB-42", binding_id: "binding-1",
  project_key: "WEB", project_name: "Website Services", task_id: task.id,
  jira_status_id: "1", jira_status_name: "To Do", jira_assignee_account_id: "account-1", jira_assignee_name: "Bradford",
  remote_updated_at: "2026-08-13T13:00:00Z", last_synced_at: 1, outbound_state: null,
} as const;

function renderBoard(overrides: Partial<React.ComponentProps<typeof TaskBoard>> = {}) {
  const props: React.ComponentProps<typeof TaskBoard> = {
    tasks: [task], jiraTaskLinks: [], operatorToken: "operator-token", sessions: [], workers: [worker], busy: false,
    onCreate: vi.fn(), onUpdate: vi.fn(), onRemove: vi.fn(), onRestore: vi.fn(), onTransition: vi.fn(), onAssign: vi.fn(), onStartWorker: vi.fn(), onOpenWorker: vi.fn(), onFetchActivity: vi.fn().mockResolvedValue({ events: [], truncated: false }), onFetchJiraComments: vi.fn().mockResolvedValue([]), onAddJiraComment: vi.fn().mockResolvedValue({ state: "delivered" }), onRetryJira: vi.fn(), onJiraImported: vi.fn().mockResolvedValue(undefined), onReorder: vi.fn(),
    ...overrides,
  };
  return { props, ...render(<TaskBoard {...props} />) };
}

test("keeps active work above the fold on phones until task creation is requested", async () => {
  vi.stubGlobal("matchMedia", vi.fn().mockReturnValue({ matches: true }));
  renderBoard({ tasks: [] });

  expect(screen.queryByLabelText("Task title")).not.toBeInTheDocument();
  const toggle = screen.getByRole("button", { name: "Write task" });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(toggle);
  const title = screen.getByLabelText("Task title");
  expect(title).toBeInTheDocument();
  await waitFor(() => expect(title).toHaveFocus());
  expect(screen.getByRole("button", { name: "Close task form" })).toHaveAttribute("aria-expanded", "true");
});

test("opens and focuses task creation when requested from global navigation", async () => {
  vi.stubGlobal("matchMedia", vi.fn().mockReturnValue({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  const { props, rerender } = renderBoard({ tasks: [], composeRequest: 0 });

  expect(screen.queryByLabelText("Task title")).not.toBeInTheDocument();
  rerender(<TaskBoard {...props} composeRequest={1} />);

  const title = await screen.findByLabelText("Task title");
  await waitFor(() => expect(title).toHaveFocus());
  expect(screen.getByRole("button", { name: "Close task form" })).toHaveAttribute("aria-expanded", "true");
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

test("keeps task creation focused after navigating from a completed task", async () => {
  const completed = { ...task, state: "completed" as const };
  Element.prototype.scrollIntoView = vi.fn();
  const { props, rerender } = renderBoard({ tasks: [completed], focusTaskId: completed.id, focusRequest: 1, composeRequest: 0 });
  await waitFor(() => expect(screen.getByRole("article", { name: completed.title })).toHaveFocus());

  rerender(<TaskBoard {...props} tasks={[completed]} focusTaskId={completed.id} focusRequest={1} composeRequest={1} />);

  await waitFor(() => expect(screen.getByLabelText("Task title")).toHaveFocus());
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

  fireEvent.click(screen.getByRole("button", { name: "Write task" }));
  expect(screen.getByLabelText("Who should handle this?")).toHaveValue("");
  expect(screen.getByRole("button", { name: "Create draft" })).toBeDisabled();
  fireEvent.change(screen.getByLabelText("Task title"), { target: { value: "Ship task editing" } });
  expect(screen.getByRole("button", { name: "Create draft" })).toBeDisabled();
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

test("offers one clear recovery action when filters hide every open task", () => {
  const onQueryChange = vi.fn();
  const onFilterChange = vi.fn();
  const onSourceChange = vi.fn();
  const onProjectChange = vi.fn();
  const onWorkerChange = vi.fn();
  renderBoard({
    query: "missing",
    filter: "assigned",
    source: "jira",
    project: "WEB",
    worker: worker.id,
    onQueryChange,
    onFilterChange,
    onSourceChange,
    onProjectChange,
    onWorkerChange,
  });

  expect(screen.getByText("One or more board filters are hiding open work.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Show all open work" }));

  expect(onQueryChange).toHaveBeenCalledWith("");
  expect(onFilterChange).toHaveBeenCalledWith("all");
  expect(onSourceChange).toHaveBeenCalledWith("all");
  expect(onProjectChange).toHaveBeenCalledWith("all");
  expect(onWorkerChange).toHaveBeenCalledWith("all");
});

test("preserves selected email work while the operator switches task entry paths", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/integrations/email/readiness")) return ok({ configured: true, connection: "ready", account_name: "Bea", account_address: "bea@example.com" });
    if (url.includes("/integrations/email/inbox")) return ok([{
      id: "message-1", conversation_id: "thread-1", internet_message_id: null, subject: "Broken form",
      sender_name: "Alex", sender_address: "alex@example.com", received_at: 1_786_000_000,
      web_url: "https://outlook.test/message-1", has_attachments: false, preview: "The form does not submit.",
    }]);
    return ok([]);
  }));
  renderBoard();

  fireEvent.click(screen.getByRole("button", { name: "Use email" }));
  const message = await screen.findByRole("checkbox");
  fireEvent.click(message);
  expect(message).toBeChecked();

  fireEvent.click(screen.getByRole("button", { name: "Write task" }));
  expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Use email" }));

  expect(screen.getByRole("checkbox")).toBeChecked();
});

test("lets the operator recover removed local work without mixing in Jira work", async () => {
  const removed = { ...task, id: "removed-1", title: "Recover this idea", removed_at: 2 };
  vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/v1/tasks/removed")) return Promise.resolve(ok([removed]));
    return Promise.resolve(ok([]));
  }));
  const onRestore = vi.fn().mockResolvedValue(undefined);
  renderBoard({ tasks: [], onRestore });

  const disclosure = await screen.findByText("Removed local work");
  fireEvent.click(disclosure);
  expect(screen.getByText("Recover a task removed from this Hive. Jira work stays under Jira and never appears here.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Restore to board" }));

  await waitFor(() => expect(onRestore).toHaveBeenCalledWith(removed));
  await waitFor(() => expect(screen.queryByText(removed.title)).not.toBeInTheDocument());
});

test("keeps removed work recoverable when restoration fails", async () => {
  const removed = { ...task, id: "removed-2", title: "Try recovery again", removed_at: 2 };
  vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL) => Promise.resolve(ok(
    String(input).endsWith("/api/v1/tasks/removed") ? [removed] : [],
  ))));
  const onRestore = vi.fn().mockRejectedValue(new Error("recovery unavailable"));
  renderBoard({ tasks: [], onRestore });

  fireEvent.click(await screen.findByText("Removed local work"));
  fireEvent.click(screen.getByRole("button", { name: "Restore to board" }));

  await waitFor(() => expect(onRestore).toHaveBeenCalledWith(removed));
  await waitFor(() => expect(screen.getByRole("button", { name: "Restore to board" })).toBeEnabled());
  expect(screen.getByText(removed.title)).toBeInTheDocument();
});

test("creates Apiary work from Tasks instead of the supervisory Apiary view", async () => {
  const requests: Array<{ url: string; init?: RequestInit }> = [];
  vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    requests.push({ url, init });
    if (url.endsWith("/api/v1/tasks/email-sources")) return Promise.resolve(ok([]));
    if (url.endsWith("/api/v1/apiary/members")) return Promise.resolve(ok([
      { hive_id: "hive-1", hive_name: "Lead Hive", operator_id: "operator-1", operator_display_name: "Bea", role: "keeper", is_local: true },
      { hive_id: "hive-2", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", role: "member", is_local: false },
    ]));
    if (url.endsWith("/api/v1/apiary/tasks") && init?.method === "POST") {
      return Promise.resolve({ ...ok({ id: "apiary-task-1", apiary_id: "apiary-1", source: "swarm", title: "Coordinate release", description: "Across both Hives", priority: "high", state: "ready", home_node_id: null, home_hive_id: "hive-2", revision: 1, created_at: 1, updated_at: 1 }), status: 201 });
    }
    if (url.endsWith("/api/v1/apiary/tasks")) return Promise.resolve(ok([]));
    throw new Error(`Unexpected request: ${url}`);
  }));
  renderBoard({ tasks: [], hiveIdentity: keeperIdentity() });

  fireEvent.click(screen.getByRole("button", { name: "Write task" }));
  fireEvent.click(screen.getByRole("radio", { name: /Apiary/ }));
  fireEvent.change(screen.getByLabelText("Task title"), { target: { value: "  Coordinate release  " } });
  fireEvent.change(screen.getByLabelText(/Description/), { target: { value: "  Across both Hives  " } });
  fireEvent.change(screen.getByLabelText("Priority"), { target: { value: "high" } });
  await waitFor(() => expect(screen.getByLabelText(/Route to a Hive/)).toHaveTextContent("Clover Hive"));
  fireEvent.change(screen.getByLabelText(/Route to a Hive/), { target: { value: "hive-2" } });
  fireEvent.click(screen.getByRole("button", { name: "Create for Apiary" }));

  await waitFor(() => expect(requests.some(({ url, init }) => url.endsWith("/api/v1/apiary/tasks") && init?.method === "POST")).toBe(true));
  const request = requests.find(({ url, init }) => url.endsWith("/api/v1/apiary/tasks") && init?.method === "POST");
  expect(JSON.parse(String(request?.init?.body))).toEqual({ title: "Coordinate release", description: "Across both Hives", priority: "high", home_hive_id: "hive-2" });
});

test("lets a Member claim shared work from Tasks", async () => {
  const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/api/v1/tasks/email-sources")) return Promise.resolve(ok([]));
    if (url.endsWith("/api/v1/apiary/members")) return Promise.resolve(ok([{ hive_id: "hive-2", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", role: "member", is_local: true }]));
    if (url.endsWith("/api/v1/apiary/tasks/task-shared/claim") && init?.method === "POST") return Promise.resolve(ok({ command: { task_id: "task-shared" }, state: "queued" }));
    if (url.endsWith("/api/v1/apiary/tasks")) return Promise.resolve(ok([apiaryTask()]));
    if (url.endsWith("/api/v1/apiary/tasks/local-executions") || url.endsWith("/api/v1/apiary/task-outbox")) return Promise.resolve(ok([]));
    if (url.endsWith("/api/v1/apiary/my-stewardship")) return Promise.resolve(ok(null));
    throw new Error(`Unexpected request: ${url}`);
  });
  vi.stubGlobal("fetch", fetchMock);
  renderBoard({ tasks: [], hiveIdentity: memberIdentity() });

  fireEvent.click(await screen.findByRole("button", { name: "Claim for this Hive" }));

  await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining("/tasks/task-shared/claim"), expect.objectContaining({ method: "POST" })));
});

test("routes owned Apiary work to a private worker from Tasks", async () => {
  const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/api/v1/tasks/email-sources")) return Promise.resolve(ok([]));
    if (url.endsWith("/api/v1/apiary/members")) return Promise.resolve(ok([{ hive_id: "hive-2", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", role: "member", is_local: true }]));
    if (url.endsWith("/api/v1/apiary/tasks/task-shared/local-execution") && init?.method === "POST") return Promise.resolve(ok({ apiary_task_id: "task-shared", local_task_id: "local-1", worker_id: worker.id, state: "ready", created_at: 2 }));
    if (url.endsWith("/api/v1/apiary/tasks")) return Promise.resolve(ok([{ ...apiaryTask(), home_hive_id: "hive-2", home_node_id: "node-2" }]));
    if (url.endsWith("/api/v1/apiary/tasks/local-executions") || url.endsWith("/api/v1/apiary/task-outbox")) return Promise.resolve(ok([]));
    if (url.endsWith("/api/v1/apiary/my-stewardship")) return Promise.resolve(ok(null));
    throw new Error(`Unexpected request: ${url}`);
  });
  vi.stubGlobal("fetch", fetchMock);
  const onOpenTask = vi.fn();
  renderBoard({ tasks: [], hiveIdentity: memberIdentity(), onOpenTask });

  fireEvent.change(await screen.findByRole("combobox", { name: "Worker for Prepare shared brief" }), { target: { value: worker.id } });
  fireEvent.click(screen.getByRole("button", { name: "Send to worker" }));

  await waitFor(() => expect(onOpenTask).toHaveBeenCalledWith("local-1"));
});

test("lets a Steward route work from Tasks only to Hives in her scope", async () => {
  const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/api/v1/tasks/email-sources")) return Promise.resolve(ok([]));
    if (url.endsWith("/api/v1/apiary/members")) return Promise.resolve(ok([
      { hive_id: "hive-3", hive_name: "Fern Hive", operator_id: "operator-3", operator_display_name: "Faye", role: "member", is_local: false },
      { hive_id: "hive-4", hive_name: "Poppy Hive", operator_id: "operator-4", operator_display_name: "Pia", role: "member", is_local: false },
    ]));
    if (url.endsWith("/api/v1/apiary/my-stewardship")) return Promise.resolve(ok({ stewardship: { managed_hive_ids: ["hive-3"], capabilities: ["assign"] } }));
    if (url.endsWith("/api/v1/apiary/steward/tasks") && init?.method === "POST") return Promise.resolve(ok({ command: { id: "command-1" }, state: "queued" }));
    if (url.endsWith("/api/v1/apiary/tasks") || url.endsWith("/api/v1/apiary/tasks/local-executions") || url.endsWith("/api/v1/apiary/task-outbox")) return Promise.resolve(ok([]));
    throw new Error(`Unexpected request: ${url}`);
  });
  vi.stubGlobal("fetch", fetchMock);
  renderBoard({ tasks: [], hiveIdentity: memberIdentity() });

  fireEvent.click(screen.getByRole("button", { name: "Write task" }));
  await waitFor(() => expect(screen.getByRole("radio", { name: /Apiary/ })).toBeInTheDocument());
  fireEvent.click(screen.getByRole("radio", { name: /Apiary/ }));
  fireEvent.change(screen.getByLabelText("Task title"), { target: { value: "Restore shared service" } });
  expect(screen.getByLabelText(/Route to a Hive/)).toHaveTextContent("Fern Hive");
  expect(screen.getByLabelText(/Route to a Hive/)).not.toHaveTextContent("Poppy Hive");
  fireEvent.change(screen.getByLabelText(/Route to a Hive/), { target: { value: "hive-3" } });
  fireEvent.click(screen.getByRole("button", { name: "Route through Keeper" }));

  await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining("/apiary/steward/tasks"), expect.objectContaining({ method: "POST" })));
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
  expect(within(card).getByText("Daisy")).toBeInTheDocument();
  expect(within(card).getByRole("combobox", { name: /Assign Swarm worker/ })).toHaveValue(worker.id);
});

test("lets the operator return assigned work to the unassigned queue", () => {
  const assigned = { ...task, state: "ready" as const, assigned_worker_id: worker.id };
  const onAssign = vi.fn().mockResolvedValue(undefined);
  renderBoard({ tasks: [assigned], onAssign });

  fireEvent.change(screen.getByRole("combobox", { name: /Assign Swarm worker/ }), { target: { value: "" } });

  expect(onAssign).toHaveBeenCalledWith(assigned, "");
});

test("does not infer assignment merely because a worker owns the repository", () => {
  renderBoard({ tasks: [{ ...task, state: "ready", assigned_worker_id: null }] });

  const card = screen.getByRole("article", { name: task.title });
  expect(within(card).getByText("Unassigned", { selector: ".task-owner" })).toBeInTheDocument();
  expect(within(card).getByRole("combobox", { name: /Assign Swarm worker/ })).toHaveValue("");
});

test("shares Jira project navigation and manual sync with the board controls", () => {
  const onJiraSync = vi.fn();
  renderBoard({
    projects: [{ key: "WWD", name: "Website Development", url: "https://jira.example.test/issues/?jql=project" }],
    onJiraSync,
  });

  expect(screen.getByRole("link", { name: "Open Website Development in Jira" })).toHaveAttribute("href", "https://jira.example.test/issues/?jql=project");
  expect(screen.getByText(/Jira refreshes every minute/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Sync now" }));
  expect(onJiraSync).toHaveBeenCalledOnce();
});

test("distinguishes assigned work from work that has started", () => {
  const assignedReady = { ...task, state: "ready" as const, assigned_worker_id: worker.id };
  const active = { ...assignedReady, id: "task-2", title: "Actively implement reload", state: "active" as const };
  renderBoard({ tasks: [assignedReady, active] });

  expect(within(screen.getByRole("article", { name: assignedReady.title })).getByText("Assigned", { selector: ".task-state" })).toBeInTheDocument();
  expect(within(screen.getByRole("article", { name: active.title })).getByText("In progress", { selector: ".task-state" })).toBeInTheDocument();
});

test("records verification evidence before review work can be completed", async () => {
  const reviewTask = { ...task, state: "review" as const, assigned_worker_id: worker.id };
  const onTransition = vi.fn().mockResolvedValue(undefined);
  renderBoard({ tasks: [reviewTask], onTransition });

  fireEvent.click(screen.getByRole("button", { name: "Finish review" }));
  const form = screen.getByRole("form", { name: `Complete ${reviewTask.title}` });
  const complete = within(form).getByRole("button", { name: "Complete task" });
  expect(complete).toBeDisabled();
  expect(within(form).getByText("This becomes part of the durable task history.")).toBeInTheDocument();

  fireEvent.change(within(form).getByLabelText("Completion evidence"), {
    target: { value: "Desktop and Android recovery verified; release 42 is live." },
  });
  fireEvent.click(complete);

  await waitFor(() => expect(onTransition).toHaveBeenCalledWith(
    reviewTask,
    "completed",
    "Desktop and Android recovery verified; release 42 is live.",
  ));
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
  expect(within(card).getByLabelText("Jira issue WEB-42")).toHaveTextContent("Bradford");
  expect(within(card).getByLabelText("Jira issue WEB-42")).toHaveTextContent("Jira changed — review before retry");
  expect(within(card).getByText("Jira changed — review before retry")).toHaveAttribute(
    "title",
    "Jira changed or rejected a Swarm update. Its current status is shown above; retry only if Swarm should replace it.",
  );
  fireEvent.click(within(card).getByRole("button", { name: "Retry Swarm update" }));
  expect(onRetryJira).toHaveBeenCalledWith(expect.objectContaining({ id: task.id }));
  expect(within(card).getByRole("link", { name: /WEB-42/ })).toHaveAttribute("href", "https://jira.example.test/browse/WEB-42");
  const swarmDetails = within(card).getByRole("region", { name: "Swarm details" });
  expect(within(swarmDetails).getByText("Status")).toBeInTheDocument();
  expect(within(swarmDetails).getByText("Priority")).toBeInTheDocument();
  expect(within(card).getByText("Swarm worker")).toBeInTheDocument();
  const jiraDetails = within(card).getByRole("region", { name: "Jira issue WEB-42" });
  expect(within(jiraDetails).getByText("Issue")).toBeInTheDocument();
  expect(within(jiraDetails).getByText("Project")).toBeInTheDocument();
  expect(within(jiraDetails).getByText("Status")).toBeInTheDocument();
  expect(within(jiraDetails).getByText("Assignee")).toBeInTheDocument();
  expect(within(card).getByText("Assigned", { selector: ".task-state" })).toBeInTheDocument();
});

test("opens an editable task detail with Jira description and image on double click", async () => {
  const createObjectURL = vi.fn().mockReturnValue("blob:jira-image");
  const revokeObjectURL = vi.fn();
  Object.defineProperty(URL, "createObjectURL", { configurable: true, value: createObjectURL });
  Object.defineProperty(URL, "revokeObjectURL", { configurable: true, value: revokeObjectURL });
  vi.stubGlobal("fetch", vi.fn().mockImplementation((url: string) => {
    if (url.endsWith("/detail")) {
      return Promise.resolve(new Response(JSON.stringify({
        summary: task.title,
        description: "Full Jira description\nwith the missing screenshot.",
        attachments: [{ id: "attachment-1", filename: "evidence.png", media_type: "image/png", byte_size: 128, is_image: true }],
      }), { status: 200, headers: { "Content-Type": "application/json" } }));
    }
    return Promise.resolve(new Response(new Blob(["image"], { type: "image/png" }), { status: 200 }));
  }));
  renderBoard({ jiraTaskLinks: [jiraTaskLink] });

  fireEvent.doubleClick(screen.getByRole("article", { name: task.title }));

  const dialog = await screen.findByRole("dialog", { name: "Review and edit task" });
  expect(within(dialog).getByLabelText("Title")).toHaveValue(task.title);
  expect(within(dialog).getByLabelText("Work brief")).toHaveValue(task.description);
  expect(within(dialog).getByText(/Full Jira description/)).toBeInTheDocument();
  const image = await within(dialog).findByRole("img", { name: "evidence.png" });
  expect(image).toHaveAttribute("src", "blob:jira-image");
  expect(within(dialog).getByRole("link", { name: "Open in Jira" })).toHaveAttribute("href", jiraTaskLink.issue_url);
  const save = within(dialog).getByRole("button", { name: "Save changes" });
  expect(save.closest("footer")).toBe(dialog.querySelector("footer"));
  fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
  await waitFor(() => expect(revokeObjectURL).toHaveBeenCalledWith("blob:jira-image"));
});

test("keeps edit explicit and ignores double clicks on interactive task controls", () => {
  renderBoard({ jiraTaskLinks: [jiraTaskLink] });
  const card = screen.getByRole("article", { name: task.title });

  const edit = within(card).getByRole("button", { name: "Edit" });
  fireEvent.doubleClick(edit);
  expect(screen.queryByRole("dialog", { name: "Review and edit task" })).not.toBeInTheDocument();
  fireEvent.click(edit);
  expect(screen.getByRole("dialog", { name: "Review and edit task" })).toBeInTheDocument();
});

test("confirms safe Jira removal without implying that Jira will be deleted", async () => {
  const onRemove = vi.fn().mockResolvedValue(undefined);
  renderBoard({ jiraTaskLinks: [jiraTaskLink], onRemove });

  fireEvent.doubleClick(screen.getByRole("article", { name: task.title }));
  const dialog = screen.getByRole("dialog", { name: "Review and edit task" });
  const remove = within(dialog).getByRole("button", { name: "Remove from Hive" });
  expect(remove).toHaveClass("secondary-button", "danger-text");
  fireEvent.click(remove);

  expect(within(dialog).getByText("The Jira issue will not be deleted or changed. Its source link and Swarm audit history stay retained.")).toBeInTheDocument();
  fireEvent.click(within(dialog).getByRole("button", { name: "Remove from Hive" }));
  await waitFor(() => expect(onRemove).toHaveBeenCalledWith(task));
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
  expect(await within(discussion).findByText("Ready for review")).toBeInTheDocument();
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

  fireEvent.click(screen.getByRole("button", { name: "Daisy" }));
  expect(onOpenWorker).toHaveBeenCalledWith("session-1");
});

test("edits task details and retains a failed form for retry", async () => {
  const onUpdate = vi.fn().mockRejectedValue(new Error("offline"));
  renderBoard({ onUpdate });

  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  const dialog = screen.getByRole("dialog", { name: "Review and edit task" });
  fireEvent.change(within(dialog).getByLabelText("Title"), { target: { value: "Make every reload stable" } });
  fireEvent.change(within(dialog).getByLabelText("Priority"), { target: { value: "urgent" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Save changes" }));

  await waitFor(() => expect(onUpdate).toHaveBeenCalledWith(task, {
    title: "Make every reload stable",
    description: task.description,
    priority: "urgent",
  }));
  expect(screen.getByRole("dialog", { name: "Review and edit task" })).toBeInTheDocument();
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
  fireEvent.dragOver(screen.getByRole("article", { name: task.title }), { dataTransfer });
  expect(screen.getByRole("article", { name: task.title })).toHaveClass("drop-target-before");
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

test("keeps an unsent Jira update when discussion is hidden and reopened", async () => {
  renderBoard({
    jiraTaskLinks: [jiraTaskLink],
    onFetchJiraComments: vi.fn().mockResolvedValue([]),
  });

  fireEvent.click(screen.getByRole("button", { name: "Discussion" }));
  const update = await screen.findByLabelText("Add an update");
  fireEvent.change(update, { target: { value: "Waiting for the reporter to confirm." } });
  fireEvent.click(screen.getByRole("button", { name: "Hide discussion" }));
  expect(screen.queryByRole("textbox", { name: "Add an update" })).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Discussion" }));
  expect(screen.getByRole("textbox", { name: "Add an update" })).toHaveValue("Waiting for the reporter to confirm.");
});

function keeperIdentity(): HiveIdentity {
  return {
    operator: { id: "operator-1", display_name: "Bea" },
    hive: { id: "hive-1", name: "Lead Hive", operator_id: "operator-1", apiary_id: "apiary-1" },
    apiary_context: {
      mode: "federated",
      apiary: { id: "apiary-1", name: "Grand Garden", keeper_operator_id: "operator-1", shared_work_backend: "jira" },
      local_role: "keeper",
    },
  };
}

function memberIdentity(): HiveIdentity {
  return {
    operator: { id: "operator-2", display_name: "Cora" },
    hive: { id: "hive-2", name: "Clover Hive", operator_id: "operator-2", apiary_id: "apiary-1" },
    apiary_context: {
      mode: "federated",
      apiary: { id: "apiary-1", name: "Grand Garden", keeper_operator_id: "operator-1", shared_work_backend: "jira" },
      local_role: "member",
    },
  };
}

function apiaryTask() {
  return { id: "task-shared", apiary_id: "apiary-1", source: "swarm", title: "Prepare shared brief", description: "Coordinate the release", priority: "high", state: "ready", home_node_id: null, home_hive_id: null, revision: 1, created_at: 1, updated_at: 1 };
}

function ok(payload: unknown) {
  return { ok: true, status: 200, json: async () => payload };
}
