import { expect, test } from "vitest";

import type { JiraTaskLink, Task, Worker } from "../api";
import { buildTaskBoardView, type TaskBoardQuery } from "./taskBoardModel";

const worker: Worker = {
  id: "worker-a", hive_id: "hive", name: "Daisy", role: "worker", provider: "claude_code",
  workspace: "/projects/a", autostart: false, position: 0, active_session_id: null,
  created_at: 1, updated_at: 1, running: false, attention_state: "sleeping",
};

const tasks: Task[] = [
  { id: "local", hive_id: "hive", title: "Local notes", workspace: "/projects/a", state: "draft", description: "", operator_instruction: "", priority: "low", assigned_worker_id: null, assigned_session_id: null, position: 0, created_at: 1, updated_at: 2 },
  { id: "jira", hive_id: "hive", title: "Repair checkout", workspace: "/projects/a", state: "ready", description: "Cart fails", operator_instruction: "", priority: "urgent", assigned_worker_id: worker.id, assigned_session_id: null, position: 1, created_at: 1, updated_at: 4 },
  { id: "blocked", hive_id: "hive", title: "Await credentials", workspace: "/projects/a", state: "blocked", description: "", operator_instruction: "", priority: "normal", assigned_worker_id: worker.id, assigned_session_id: null, position: 2, created_at: 1, updated_at: 3 },
  { id: "done", hive_id: "hive", title: "Already shipped", workspace: "/projects/a", state: "completed", closed_on_evidence: true, description: "", operator_instruction: "", priority: "high", assigned_worker_id: worker.id, assigned_session_id: null, position: 3, created_at: 1, updated_at: 5 },
];

const jiraLink: JiraTaskLink = {
  issue_id: "1", issue_key: "WWD-42", issue_url: "https://jira.example/browse/WWD-42", binding_id: "binding",
  project_key: "WWD", project_name: "Website Development", task_id: "jira", jira_status_id: "1", jira_status_name: "To Do",
  jira_assignee_account_id: "me", jira_assignee_name: "Operator", remote_updated_at: "2026-08-13T12:00:00Z", last_synced_at: 1, outbound_state: null,
};

const baseQuery: TaskBoardQuery = { text: "", filter: "all", source: "all", sort: "queue", project: "all", worker: "all" };

test("separates completed work and retains the total open count while filtering", () => {
  const view = buildTaskBoardView(tasks, [jiraLink], [worker], { ...baseQuery, filter: "attention" });
  expect(view.open.map((task) => task.id)).toEqual(["blocked"]);
  expect(view.completed.map((task) => task.id)).toEqual(["done"]);
  expect(view.allOpenCount).toBe(3);
});

test("finished work with no evidence is held apart from completed work", () => {
  // Completed is where work goes to stop being looked at. Filing unverified
  // work there put the one closed state that still needs somebody in the place
  // least likely to get it.
  const base = tasks[0];
  const shipped = { ...base, id: "shipped", state: "completed", closed_on_evidence: true } as Task;
  const exempted = { ...base, id: "exempted", state: "completed", deployment_recorded: false, closed_on_evidence: true } as Task;
  const waiting = { ...base, id: "waiting", state: "completed", closed_on_evidence: false } as Task;

  const view = buildTaskBoardView([shipped, exempted, waiting], [], [], baseQuery);

  expect(view.unverified.map((task) => task.id)).toEqual(["waiting"]);
  // An approved nothing-to-deploy claim closes a task as surely as a deployment
  // does, so it belongs with completed work and not beside it.
  expect(view.completed.map((task) => task.id).sort()).toEqual(["exempted", "shipped"]);
  // And the count of completed work does not quietly carry the unverified row.
  expect(view.completed).toHaveLength(2);
});

test("a Jira issue nobody here worked on is not Swarm work awaiting evidence", () => {
  // Twelve of these sat in the section asking somebody to chase evidence nobody
  // owed. They were imported from Jira, completed in Jira, and no Swarm worker
  // ever acted on them — the completion record exists, in the system that owns
  // the work.
  const base = tasks[0];
  const mirrored = { ...base, id: "mirrored", state: "completed", closed_on_evidence: false, worked_here: false } as Task;
  const link = { ...jiraLink, task_id: "mirrored" };

  const view = buildTaskBoardView([mirrored], [link], [], baseQuery);

  expect(view.unverified).toHaveLength(0);
  expect(view.completed.map((task) => task.id)).toEqual(["mirrored"]);
});

test("work a Swarm worker really did against a Jira issue still needs evidence", () => {
  // The boundary that matters most, and it protects a case that has never
  // occurred on this Hive — zero Jira-linked tasks have any worker activity, in
  // any state. Making the Jira link alone sufficient would turn a correct fix
  // into a blind spot, and no live row would reveal it.
  const base = tasks[0];
  const worked = { ...base, id: "worked", state: "completed", closed_on_evidence: false, worked_here: true } as Task;
  const link = { ...jiraLink, task_id: "worked" };

  const view = buildTaskBoardView([worked], [link], [], baseQuery);

  expect(view.unverified.map((task) => task.id)).toEqual(["worked"]);
  expect(view.completed).toHaveLength(0);
});

test("a local task nobody worked on is still Swarm's to account for", () => {
  // Worker involvement is not sufficient on its own either. Without a Jira
  // link there is no other system holding the record, so the row stays.
  const base = tasks[0];
  const local = { ...base, id: "local", state: "completed", closed_on_evidence: false, worked_here: false } as Task;

  const view = buildTaskBoardView([local], [], [], baseQuery);

  expect(view.unverified.map((task) => task.id)).toEqual(["local"]);
});

test("searches Jira identity and combines project, worker, and assignment filters", () => {
  const view = buildTaskBoardView(tasks, [jiraLink], [worker], {
    ...baseQuery, text: "wwd-42", project: "WWD", worker: worker.id, filter: "assigned",
  });
  expect(view.open.map((task) => task.id)).toEqual(["jira"]);
  expect(view.jiraByTask.get("jira")?.project_name).toBe("Website Development");
});

test("sorts by product meaning while preserving queue position as the tie breaker", () => {
  expect(buildTaskBoardView(tasks, [jiraLink], [worker], { ...baseQuery, sort: "priority" }).open.map((task) => task.id))
    .toEqual(["jira", "blocked", "local"]);
  expect(buildTaskBoardView(tasks, [jiraLink], [worker], { ...baseQuery, sort: "updated" }).open.map((task) => task.id))
    .toEqual(["jira", "blocked", "local"]);
  expect(buildTaskBoardView(tasks, [jiraLink], [worker], { ...baseQuery, sort: "project" }).open.map((task) => task.id))
    .toEqual(["local", "blocked", "jira"]);
});

test("separates Jira, email, and Swarm-created sources without losing Jira projects", () => {
  expect(buildTaskBoardView(tasks, [jiraLink], [worker], { ...baseQuery, source: "jira", project: "WWD" }, new Set(["blocked"])).open.map((task) => task.id)).toEqual(["jira"]);
  expect(buildTaskBoardView(tasks, [jiraLink], [worker], { ...baseQuery, source: "email" }, new Set(["blocked"])).open.map((task) => task.id)).toEqual(["blocked"]);
  expect(buildTaskBoardView(tasks, [jiraLink], [worker], { ...baseQuery, source: "local" }, new Set(["blocked"])).open.map((task) => task.id)).toEqual(["local"]);
});

test("filters and sorts finished work the same way as work in progress", () => {
  // "When I sort by source email it only sorts the active tasks, not the closed
  // tasks. It should do both." The query applied to open work only, so choosing
  // a source narrowed the top of the board and left everything below it alone.
  const base = tasks[0];
  const open = { ...base, id: "open-email", title: "Bravo", state: "active" } as Task;
  const closedEmail = { ...base, id: "closed-email", title: "Alpha", state: "completed", closed_on_evidence: true } as Task;
  const closedLocal = { ...base, id: "closed-local", title: "Zulu", state: "completed", closed_on_evidence: true } as Task;

  const view = buildTaskBoardView(
    [open, closedLocal, closedEmail],
    [],
    [],
    { text: "", filter: "all", source: "email", sort: "updated", project: "all", worker: "all" },
    new Set(["open-email", "closed-email"]),
  );

  // The source filter reaches finished work.
  expect(view.completed.map((entry) => entry.id)).toEqual(["closed-email"]);
  // And so does the sort: this list is ordered, not merely passed through.
  const newest = { ...closedLocal, id: "closed-new", title: "Newest", updated_at: 900 } as Task;
  const oldest = { ...closedEmail, id: "closed-old", title: "Oldest", updated_at: 100 } as Task;
  const ordered = buildTaskBoardView(
    [open, oldest, newest],
    [],
    [],
    { text: "", filter: "all", source: "all", sort: "updated", project: "all", worker: "all" },
    new Set(),
  );
  expect(ordered.completed.map((entry) => entry.title)).toEqual(["Newest", "Oldest"]);
});

test("orders by when work was created, oldest first, so neglect surfaces", () => {
  // updated_at moves whenever anything touches a task, including automation, so
  // it answers "what changed recently". The question that finds a five-day-old
  // draft nobody has picked up is "what has been waiting longest", and
  // newest-first would bury exactly those.
  const aged: Task[] = [
    { ...tasks[0], id: "newest", created_at: 900, updated_at: 900, position: 0 },
    { ...tasks[0], id: "oldest", created_at: 100, updated_at: 999, position: 1 },
    { ...tasks[0], id: "middle", created_at: 500, updated_at: 500, position: 2 },
  ];
  const byCreated = buildTaskBoardView(aged, [], [worker], { ...baseQuery, sort: "created" });
  expect(byCreated.open.map((task) => task.id)).toEqual(["oldest", "middle", "newest"]);

  // And it is a different answer from "recently updated", which is the whole
  // reason for adding it: the oldest task here was touched most recently.
  const byUpdated = buildTaskBoardView(aged, [], [worker], { ...baseQuery, sort: "updated" });
  expect(byUpdated.open.map((task) => task.id)).toEqual(["oldest", "newest", "middle"]);
  expect(byCreated.open.map((task) => task.id)).not.toEqual(byUpdated.open.map((task) => task.id));
});
