import type { JiraTaskLink, Task, TaskPriority, TaskState } from "../api";

const stateLabels: Record<TaskState, string> = {
  draft: "Draft",
  ready: "Ready",
  active: "In progress",
  blocked: "Blocked",
  review: "Review",
  completed: "Completed",
};

const priorityLabels: Record<TaskPriority, string> = {
  low: "Low",
  normal: "Normal",
  high: "High",
  urgent: "Urgent",
};

function taskStateLabel(task: Task): string {
  if (task.state === "ready" && task.assigned_worker_id) return "Assigned";
  return stateLabels[task.state];
}

export default function TaskMetadata({ task, jiraLink, busy, onRetryJira }: {
  task: Task;
  jiraLink?: JiraTaskLink;
  busy: boolean;
  onRetryJira: (task: Task) => Promise<void>;
}) {
  return (
    <div className="task-metadata-panel">
      <section className="task-metadata-section" aria-label="Swarm details">
        <strong className="task-section-label">Swarm</strong>
        <dl>
          <div><dt>Status</dt><dd><span className={`task-state state-${task.state}`}>{taskStateLabel(task)}</span></dd></div>
          <div><dt>Priority</dt><dd><span className={`task-priority priority-${task.priority}`}>{priorityLabels[task.priority]}</span></dd></div>
        </dl>
      </section>
      {jiraLink && (
        <section className="task-metadata-section task-jira-origin" aria-label={`Jira issue ${jiraLink.issue_key}`}>
          <strong className="task-section-label">Jira</strong>
          <dl>
            <div><dt>Issue</dt><dd>
              {jiraLink.issue_url ? (
                <a href={jiraLink.issue_url} target="_blank" rel="noreferrer" title={`Open ${jiraLink.issue_key} in Jira`}>
                  <strong>{jiraLink.issue_key}</strong><span aria-hidden="true">↗</span>
                </a>
              ) : <strong>{jiraLink.issue_key}</strong>}
            </dd></div>
            <div><dt>Project</dt><dd className="task-text-value" title={jiraLink.project_name}>{jiraLink.project_name}</dd></div>
            <div><dt>Status</dt><dd className="task-text-value">{jiraLink.jira_status_name}</dd></div>
            <div><dt>Assignee</dt><dd className="task-text-value">{jiraLink.jira_assignee_name ?? "Unassigned"}</dd></div>
          </dl>
          {jiraLink.outbound_state && (
            <span
              className={`jira-sync-state ${jiraLink.outbound_state}`}
              title={jiraLink.outbound_state === "conflict"
                ? "Jira changed or rejected a Swarm update. Its current status is shown above; retry only if Swarm should replace it."
                : jiraLink.outbound_state === "uncertain"
                  ? "Jira may already have received this update. Sync before retrying to avoid repeating it."
                  : undefined}
            >
              {jiraLink.outbound_state === "queued" || jiraLink.outbound_state === "dispatching"
                ? "Updating Jira…"
                : jiraLink.outbound_state === "conflict"
                  ? "Jira changed — review before retry"
                  : "Jira result unknown — sync before retry"}
            </span>
          )}
          {(jiraLink.outbound_state === "conflict" || jiraLink.outbound_state === "uncertain") && (
            <button className="text-button jira-sync-retry" type="button" disabled={busy} onClick={() => void onRetryJira(task)}>
              Retry Swarm update
            </button>
          )}
        </section>
      )}
    </div>
  );
}
