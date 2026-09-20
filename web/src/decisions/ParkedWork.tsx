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

type Props = {
  tasks: Task[];
  now?: number;
  onOpenTask?: (taskId: string) => void;
};

/**
 * Work that is waiting on purpose, which nothing else on this page shows.
 *
 * ⚠️ THIS EXISTS BECAUSE THE NAGGING STOPPED. Until 07c24d3d a park was kept in
 * view by being re-raised at Queen over and over; that repetition was the
 * visibility, and bounding it removed the only thing keeping these findable.
 * They are not in "Needs you" because nothing is pending on them, and Activity
 * is a log rather than a list.
 *
 * The two groups are kept apart deliberately. A park is a decision the operator
 * made and can unmake; an external wait is the world's, and reading them as one
 * list would invite them to take back a decision they never made.
 */
export default function ParkedWork({ tasks, now = Math.floor(Date.now() / 1000), onOpenTask }: Props) {
  const parked = tasks.filter((task) => task.park === "operator_deferral");
  const waiting = tasks.filter((task) => task.park === "external_condition");

  if (parked.length === 0 && waiting.length === 0) {
    return <p className="muted">Nothing is parked. Work you defer, and work waiting on something outside this Hive, will appear here.</p>;
  }

  const group = (heading: string, blurb: string, rows: Task[]) =>
    rows.length > 0 && (
      <section className="parked-group">
        <h4>{heading} <small>{rows.length}</small></h4>
        <p className="muted">{blurb}</p>
        <ul className="parked-list">
          {rows.map((task) => (
            <li key={task.id}>
              <button type="button" className="linklike" onClick={() => onOpenTask?.(task.id)}>{task.title}</button>
              <span className="muted"> · {repoName(task.workspace)} · waiting {waitedFor(task.updated_at, now)}</span>
              {task.blocked_note && <p className="muted parked-note">{task.blocked_note}</p>}
            </li>
          ))}
        </ul>
      </section>
    );

  return (
    <div className="parked-work">
      {group("Parked by you", "You decided these could wait. Nothing will raise them until you move them.", parked)}
      {group("Waiting on the world", "These wait on something outside this Hive. Nobody here can move them.", waiting)}
    </div>
  );
}
