import type { Task } from "../api";

/// The repository a person recognises, out of an absolute workspace path.
function repoName(workspace: string) {
  const trimmed = workspace.replace(/\/+$/, "");
  return trimmed.slice(trimmed.lastIndexOf("/") + 1) || trimmed;
}

function waitedFor(since: number, now: number) {
  const hours = Math.floor((now - since) / 3600);
  if (hours < 1) return "under an hour";
  if (hours < 24) return `${hours} ${hours === 1 ? "hour" : "hours"}`;
  const days = Math.floor(hours / 24);
  return `${days} ${days === 1 ? "day" : "days"}`;
}

/**
 * One scannable line out of a worker's handoff note.
 *
 * ⚠️ THESE NOTES ARE NOT WRITTEN FOR THIS SCREEN. They are worker-to-Queen
 * handoffs — markdown, capitals, several hundred words — and rendering one
 * whole made this list a wall of text the operator could not scan, which is
 * the opposite of what a Parked tab is for. The first sentence is a summary;
 * the rest belongs on the task, one click away.
 */
function gist(note: string | null | undefined) {
  if (!note) return "";
  const flattened = note
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/[*_`#>|]+/g, "")
    .replace(/\s+/g, " ")
    .trim();
  if (!flattened) return "";
  const stop = flattened.search(/[.!?](\s|$)/);
  const sentence = stop > 0 ? flattened.slice(0, stop + 1) : flattened;
  return sentence.length > 150 ? `${sentence.slice(0, 149).trimEnd()}…` : sentence;
}

/**
 * Waiting on something outside this Hive: an outside party, a date, or an
 * external-condition park recorded before ADR 0119. Nobody here can move it,
 * and it comes back to its worker by itself when its time arrives.
 *
 * An operator-owed park is NOT here any more. It is something only the
 * operator can do, so it is a Needs you item with Done (ADR 0119).
 */
export function isWaitingOnTheWorld(task: Task) {
  if (task.wait) return (task.wait.kind === "outside" || task.wait.kind === "date") && !task.wait.cleared;
  return task.park === "external_condition";
}

function comesBack(resumeAt: number) {
  return new Date(resumeAt * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

type Props = {
  tasks: Task[];
  now?: number;
  onOpenTask?: (taskId: string) => void;
};

/**
 * Work that is waiting on the world, which nothing else on this page shows.
 *
 * ⚠️ THIS EXISTS BECAUSE THE NAGGING STOPPED. Until 07c24d3d a wait was kept in
 * view by being re-raised at Queen over and over; that repetition was the
 * visibility, and bounding it removed the only thing keeping these findable.
 * They are not in "Needs you" because nothing is pending on the operator, and
 * Activity is a log rather than a list.
 */
export default function ParkedWork({ tasks, now = Math.floor(Date.now() / 1000), onOpenTask }: Props) {
  const waiting = tasks.filter(isWaitingOnTheWorld);

  if (waiting.length === 0) {
    return <p className="muted">Nothing is waiting on the world. Work waiting on someone outside this Hive, or on a date, will appear here.</p>;
  }

  return (
    <div className="parked-work">
      <section className="parked-group">
        <h4>Waiting on the world <small>{waiting.length}</small></h4>
        <p className="muted parked-blurb">These wait on something outside this Hive. Nobody here can move them, and each comes back to its worker by itself.</p>
        <ul className="parked-list">
          {waiting.map((task) => {
            const summary = task.wait?.needs ?? gist(task.blocked_note);
            return (
              <li key={task.id}>
                <div className="parked-row">
                  <span className="parked-title">{task.title}</span>
                  <button type="button" className="secondary-button parked-open" onClick={() => onOpenTask?.(task.id)}>Open</button>
                </div>
                <p className="muted parked-meta">
                  {repoName(task.workspace)} · waiting {waitedFor(task.updated_at, now)}
                  {task.wait?.party ? ` on ${task.wait.party}` : ""}
                  {task.wait?.resume_at ? ` · comes back ${comesBack(task.wait.resume_at)}` : ""}
                </p>
                {summary && <p className="parked-gist">{summary}</p>}
              </li>
            );
          })}
        </ul>
      </section>
    </div>
  );
}
