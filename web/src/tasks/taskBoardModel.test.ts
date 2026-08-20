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
  { id: "done", hive_id: "hive", title: "Already shipped", workspace: "/projects/a", state: "completed", description: "", operator_instruction: "", priority: "high", assigned_worker_id: worker.id, assigned_session_id: null, position: 3, created_at: 1, updated_at: 5 },
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
  const closedEmail = { ...base, id: "closed-email", title: "Alpha", state: "completed" } as Task;
  const closedLocal = { ...base, id: "closed-local", title: "Zulu", state: "completed" } as Task;

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
