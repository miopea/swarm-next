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

type Props = {
  tasks: Task[];
  now?: number;
  onOpenTask?: (taskId: string) => void;
  /** Present only for work the operator parked themselves. */
  onLiftPark?: (taskId: string) => void;
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
export default function ParkedWork({ tasks, now = Math.floor(Date.now() / 1000), onOpenTask, onLiftPark }: Props) {
  const parked = tasks.filter((task) => task.park === "operator_deferral");
  const waiting = tasks.filter((task) => task.park === "external_condition");

  if (parked.length === 0 && waiting.length === 0) {
    return <p className="muted">Nothing is parked. Work you defer, and work waiting on something outside this Hive, will appear here.</p>;
  }

  // ⚠️ LIFTABLE ONLY IN THE FIRST GROUP, and the split is the whole reason the
  // groups exist. A park is the operator's own decision and theirs to unmake; an
  // external wait belongs to the world, and offering to clear one would invite
  // taking back a decision they never made.
  const group = (heading: string, blurb: string, rows: Task[], liftable = false) =>
    rows.length > 0 && (
      <section className="parked-group">
        <h4>{heading} <small>{rows.length}</small></h4>
        <p className="muted parked-blurb">{blurb}</p>
        <ul className="parked-list">
          {rows.map((task) => {
            const summary = gist(task.blocked_note);
            return (
              <li key={task.id}>
                <div className="parked-row">
                  <span className="parked-title">{task.title}</span>
                  {liftable && onLiftPark && (
                    <button
                      type="button"
                      className="secondary-button parked-lift"
                      onClick={() => onLiftPark(task.id)}
                    >
                      I've done this
                    </button>
                  )}
                  <button type="button" className="secondary-button parked-open" onClick={() => onOpenTask?.(task.id)}>Open</button>
                </div>
                <p className="muted parked-meta">{repoName(task.workspace)} · waiting {waitedFor(task.updated_at, now)}</p>
                {summary && <p className="parked-gist">{summary}</p>}
              </li>
            );
          })}
        </ul>
      </section>
    );

  return (
    <div className="parked-work">
      {group("Parked by you", "You decided these could wait, or answered that you would do them yourself. Nothing will raise them until you move them.", parked, true)}
      {group("Waiting on the world", "These wait on something outside this Hive. Nobody here can move them.", waiting)}
    </div>
  );
}
