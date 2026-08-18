import type { TaskActivity, TaskActivityPage, TaskState } from "../api";

const stateLabels: Record<TaskState, string> = {
  draft: "Draft",
  ready: "Ready",
  active: "In progress",
  blocked: "Blocked",
  review: "Review",
  completed: "Completed",
};

export default function TaskActivityPanel({ activity, loading, failed, onRetry }: {
  activity: TaskActivityPage | undefined;
  loading: boolean;
  failed: boolean;
  onRetry: () => void;
}) {
  return (
    <section className="task-history" aria-label="Task history" aria-live="polite">
      {loading ? <p>Loading history…</p> : failed ? (
        <p>History is unavailable. <button className="text-button" type="button" onClick={onRetry}>Retry</button></p>
      ) : activity?.events.length ? (
        <>
          {activity.truncated && <p className="task-history-note">Showing the latest activity.</p>}
          <ol>
            {activity.events.map((entry) => (
              <li key={entry.sequence}>
                <span>
                  <span>{activityLabel(entry)}</span>
                  {entry.note && <small className="task-history-handoff">{entry.note}</small>}
                </span>
                <time dateTime={new Date(entry.occurred_at * 1000).toISOString()}>{formatActivityTime(entry.occurred_at)}</time>
              </li>
            ))}
          </ol>
        </>
      ) : <p>No history recorded.</p>}
    </section>
  );
}

function activityLabel(activity: TaskActivity): string {
  if (activity.kind === "created") return "Task created";
  if (activity.kind === "details_updated") return "Details updated";
  if (activity.kind === "removed") return "Removed from Hive";
  if (activity.kind === "restored") return "Restored to Hive";
  if (activity.kind === "assigned") return "Worker assigned";
  if (activity.kind === "unassigned") return "Worker released";
  if (activity.from_state && activity.to_state) return `${stateLabels[activity.from_state]} → ${stateLabels[activity.to_state]}`;
  return "State updated";
}

function formatActivityTime(occurredAt: number): string {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(occurredAt * 1000));
}
