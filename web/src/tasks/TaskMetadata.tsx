import type { JiraTaskLink, Task, TaskPriority, TaskState } from "../api";
import { taskAgeTitle, taskCreatedOn } from "./taskAge";

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
  // Finished is not the same as shown to be live. Calling it completed claims
  // more than anyone has established, which is the distinction between
  // committed and deployed drawn everywhere else here.
  //
  // Keyed on evidence rather than on a deployment, because work closed on a
  // nothing-to-deploy claim Queen approved IS verified — somebody looked and
  // agreed there was nothing to ship. Reading deployment_recorded alone called
  // 30 of the 68 rows this touched unverified when they were properly done.
  // Recorded-unverifiable is its OWN label, never "verified". The operator
  // said nobody can now establish where this went; showing it as verified
  // would claim a check that never happened, which is the exact thing the
  // control was added to avoid rather than to enable.
  if (task.state === "completed" && task.closed_unverifiable) return "Finished · unverifiable";
  if (task.state === "completed" && !task.closed_on_evidence) return "Finished · unverified";
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
      {/* No "Swarm" label here. It named the task's source, which is the same
          word on every Swarm-native row, and it held the widest column on the
          board to say it — read by the operator as the worker's name. The Jira
          section below keeps its label, because there it distinguishes. */}
      <section className="task-metadata-section" aria-label="Swarm details">
        <dl>
          <div><dt>Status</dt><dd><span className={`task-state state-${task.state}${task.state === "completed" && task.closed_unverifiable ? " unverifiable" : task.state === "completed" && !task.closed_on_evidence ? " unverified" : ""}`} title={task.state === "completed" && task.closed_unverifiable ? "Finished, and recorded as impossible to verify now. Nobody established where this went; this is a record of that, not evidence that it shipped." : task.state === "completed" && !task.closed_on_evidence ? "The work is finished. Nothing has recorded where it is running, and no nothing-to-deploy claim has been approved, so nothing has shown it to be live." : undefined}>{taskStateLabel(task)}</span></dd></div>
          <div><dt>Priority</dt><dd><span className={`task-priority priority-${task.priority}`}>{priorityLabels[task.priority]}</span></dd></div>
          {/* The date, with the elapsed time in the tooltip. It was the other
              way round until the operator asked for the date: an age answers
              "which of these has gone wrong" but never answers "when was this
              raised", and only one of those can be read off a row. */}
          <div><dt>Created</dt><dd><span className="task-age" title={taskAgeTitle(task.created_at, Date.now())}>{taskCreatedOn(task.created_at, Date.now())}</span></dd></div>
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
