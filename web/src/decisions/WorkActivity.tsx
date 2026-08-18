import { useMemo, useState } from "react";

import type { Task, TaskActivity, TaskActivityActorKind, TaskActivityPage, TaskState, Worker } from "../api";
import BeeMascot from "../brand/BeeMascot";

type ActivityFilter = "all" | "progress" | "assignments" | "changes";
type ActorFilter = "all" | TaskActivityActorKind;

const stateLabels: Record<TaskState, string> = {
  draft: "Draft", ready: "Ready", active: "In progress", blocked: "Blocked", review: "Review", completed: "Completed",
};

export default function WorkActivity({ activity, tasks, workers, loading, failed, onRetry, onOpenTask }: {
  activity: TaskActivityPage | undefined;
  tasks: Task[];
  workers: Worker[];
  loading: boolean;
  failed: boolean;
  onRetry: () => void;
  onOpenTask?: (taskId: string) => void;
}) {
  const [filter, setFilter] = useState<ActivityFilter>("all");
  const [actorFilter, setActorFilter] = useState<ActorFilter>("all");
  const [query, setQuery] = useState("");
  const taskNames = useMemo(() => new Map(tasks.map((task) => [task.id, task.title])), [tasks]);
  const workerNames = useMemo(() => new Map(workers.map((worker) => [worker.id, worker.name])), [workers]);
  const visible = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    return [...(activity?.events ?? [])].reverse().filter((entry) => {
      if (filter === "progress" && entry.kind !== "state_changed") return false;
      if (filter === "assignments" && entry.kind !== "assigned" && entry.kind !== "unassigned") return false;
      if (filter === "changes" && entry.kind !== "created" && entry.kind !== "details_updated") return false;
      if (actorFilter !== "all" && entry.actor_kind !== actorFilter) return false;
      return !normalized || (taskNames.get(entry.task_id) ?? "Unknown task").toLocaleLowerCase().includes(normalized);
    });
  }, [activity, actorFilter, filter, query, taskNames]);

  if (loading) return <div className="activity-state" role="status">Loading recent work…</div>;
  if (failed) return <div className="activity-state">Activity is unavailable. <button className="text-button" type="button" onClick={onRetry}>Retry</button></div>;

  return (
    <section className="work-activity" aria-labelledby="work-activity-heading">
      <div className="work-activity-intro">
        <div><p className="eyebrow">Durable work record</p><h3 id="work-activity-heading">What changed recently</h3><p>Task progress, assignments, and edits stay here without terminal output or background system noise.</p></div>
        <div className="work-activity-controls">
          <label><span>Find work</span><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Task title" /></label>
          <label><span>Show</span><select value={filter} onChange={(event) => setFilter(event.target.value as ActivityFilter)}><option value="all">All activity</option><option value="progress">Progress</option><option value="assignments">Assignments</option><option value="changes">Created and edited</option></select></label>
          <label><span>Source</span><select value={actorFilter} onChange={(event) => setActorFilter(event.target.value as ActorFilter)}><option value="all">Everyone</option><option value="operator">You</option><option value="worker">Workers</option><option value="jira">Jira</option><option value="email">Email</option><option value="system">Swarm</option></select></label>
          <button className="secondary-button" type="button" onClick={onRetry}>Refresh</button>
        </div>
      </div>
      {visible.length ? (
        <ol className="work-activity-list">
          {visible.map((entry) => (
            <li key={entry.sequence}>
              <span className={`activity-kind kind-${entry.kind}`} aria-hidden="true" />
              <div className="activity-copy">
                <button type="button" onClick={() => onOpenTask?.(entry.task_id)} disabled={!onOpenTask}>{taskNames.get(entry.task_id) ?? "Unavailable task"}</button>
                <span className="activity-summary"><span className={`activity-actor actor-${entry.actor_kind}`}>{actorLabel(entry, workerNames)}</span><span>{activityLabel(entry)}</span></span>
                {entry.note ? <small>{entry.note}</small> : null}
              </div>
              <time dateTime={new Date(entry.occurred_at * 1000).toISOString()}>{formatTime(entry.occurred_at)}</time>
            </li>
          ))}
        </ol>
      ) : (
        <div className="decision-empty compact"><BeeMascot className="empty-bee" expression="available" /><h3>No matching activity</h3><p>Recent task changes will appear here.</p></div>
      )}
      {activity?.truncated ? <p className="work-activity-footnote">Showing the latest {activity.events.length} events.</p> : null}
    </section>
  );
}

function actorLabel(activity: TaskActivity, workerNames: Map<string, string>): string {
  if (activity.actor_kind === "operator") return "You";
  if (activity.actor_kind === "worker") return activity.actor_id ? workerNames.get(activity.actor_id) ?? "Worker" : "Worker";
  if (activity.actor_kind === "jira") return "Jira";
  if (activity.actor_kind === "email") return "Email";
  return "Swarm";
}

function activityLabel(activity: TaskActivity): string {
  if (activity.kind === "created") return "Task created";
  if (activity.kind === "details_updated") return "Details updated";
  if (activity.kind === "removed") return "Removed from Hive";
  if (activity.kind === "restored") return "Restored to Hive";
  if (activity.kind === "assigned") return "Worker assigned";
  if (activity.kind === "unassigned") return "Worker released";
  if (activity.from_state && activity.to_state) return `${stateLabels[activity.from_state]} → ${stateLabels[activity.to_state]}`;
  return "Progress updated";
}

function formatTime(occurredAt: number): string {
  return new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" }).format(new Date(occurredAt * 1000));
}
