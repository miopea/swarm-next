import { useState } from "react";
import type { JiraTaskLink, Task } from "../api";
import TaskDetailDialog from "../tasks/TaskDetailDialog";

const task: Task = {
  id: "preview-fixture", hive_id: "fictional-hive", title: "Inspect fictional attachment previews",
  workspace: "/fixture", state: "draft", description: "Eight fictional images; no Hive or external service is contacted.",
  operator_instruction: "", priority: "normal", assigned_worker_id: null, assigned_session_id: null,
  position: 0, created_at: 1, updated_at: 1,
};
const link: JiraTaskLink = {
  task_id: task.id, issue_id: "fixture-issue", issue_key: "DEMO-1", issue_url: "",
  binding_id: "fixture-binding", project_key: "DEMO", project_name: "Fictional project",
  jira_status_id: "draft", jira_status_name: "Draft", jira_assignee_account_id: null,
  jira_assignee_name: null, remote_updated_at: "2026-09-09", last_synced_at: 1, outbound_state: null,
};
const prefix = `/api/v1/integrations/jira/task-links/${task.id}`;

// A deterministic fixture, not an emulation of Jira or a live response override.
// Hold one preview until the dialog cancels it; the other previews must appear.
export function taskPreviewFixtureResponse(path: string, init?: RequestInit): Promise<Response> | undefined {
  if (path === `${prefix}/detail`) return Promise.resolve(new Response(JSON.stringify({
    summary: task.title, description: task.description,
    attachments: Array.from({ length: 8 }, (_, index) => ({
      id: String(index), filename: `Fictional preview ${index + 1}.svg`, media_type: "image/svg+xml", byte_size: 256, is_image: true,
    })),
  })));
  if (!path.startsWith(`${prefix}/attachments/`)) return undefined;
  const index = Number(path.slice(path.lastIndexOf("/") + 1));
  const response = () => new Response(
    `<svg xmlns="http://www.w3.org/2000/svg" width="480" height="240"><rect width="480" height="240" fill="#eef3df"/><circle cx="90" cy="120" r="48" fill="#e7b34d"/><text x="160" y="128" font-size="24" fill="#263724">Fictional image ${index + 1}</text></svg>`,
    { headers: { "Content-Type": "image/svg+xml" } });
  if (index !== 0) return Promise.resolve(response());
  return new Promise((_resolve, reject) => {
    const abort = () => reject(new DOMException("Fixture read cancelled", "AbortError"));
    if (init?.signal?.aborted) abort();
    else init?.signal?.addEventListener("abort", abort, { once: true });
  });
}

export default function TaskPreviewFixture() {
  const [open, setOpen] = useState(false);
  return <main style={{ padding: 24 }}>
    <h1>Fictional task preview reader</h1>
    <p>Open the reader: images 2–8 should appear while image 1 is held. Close and reopen to check cancellation. No task can be saved or removed here.</p>
    <button onClick={() => setOpen(true)}>Open fictional task</button>
    {open && <TaskDetailDialog task={task} jiraLink={link} operatorToken="fixture-only" busy={false}
      onClose={() => setOpen(false)} onSave={async () => { throw new Error("Fixture is read-only"); }}
      onRemove={async () => { throw new Error("Fixture is read-only"); }} />}
  </main>;
}
