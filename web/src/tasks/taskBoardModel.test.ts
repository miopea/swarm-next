import { expect, test } from "vitest";

import type { JiraTaskLink, Task, Worker } from "../api";
import { buildTaskBoardView, type TaskBoardQuery } from "./taskBoardModel";

const worker: Worker = {
  id: "worker-a", hive_id: "hive", name: "Daisy", role: "worker", provider: "claude_code",
  workspace: "/projects/a", autostart: false, position: 0, active_session_id: null,
  created_at: 1, updated_at: 1, running: false, attention_state: "sleeping",
};

const tasks: Task[] = [
  { id: "local", hive_id: "hive", title: "Local notes", workspace: "/projects/a", state: "draft", description: "", priority: "low", assigned_worker_id: null, assigned_session_id: null, position: 0, created_at: 1, updated_at: 2 },
  { id: "jira", hive_id: "hive", title: "Repair checkout", workspace: "/projects/a", state: "ready", description: "Cart fails", priority: "urgent", assigned_worker_id: worker.id, assigned_session_id: null, position: 1, created_at: 1, updated_at: 4 },
  { id: "blocked", hive_id: "hive", title: "Await credentials", workspace: "/projects/a", state: "blocked", description: "", priority: "normal", assigned_worker_id: worker.id, assigned_session_id: null, position: 2, created_at: 1, updated_at: 3 },
  { id: "done", hive_id: "hive", title: "Already shipped", workspace: "/projects/a", state: "completed", description: "", priority: "high", assigned_worker_id: worker.id, assigned_session_id: null, position: 3, created_at: 1, updated_at: 5 },
];

const jiraLink: JiraTaskLink = {
  issue_id: "1", issue_key: "WWD-42", issue_url: "https://jira.example/browse/WWD-42", binding_id: "binding",
  project_key: "WWD", project_name: "Website Development", task_id: "jira", jira_status_id: "1", jira_status_name: "To Do",
  jira_assignee_account_id: "me", jira_assignee_name: "Operator", remote_updated_at: "2026-08-13T12:00:00Z", last_synced_at: 1, outbound_state: null,
};

const baseQuery: TaskBoardQuery = { text: "", filter: "all", sort: "queue", project: "all", worker: "all" };

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
