import { useState, type ReactNode } from "react";
import type { ApiaryTask } from "../api";

export function isClosedSharedTask(task: ApiaryTask) {
  return task.state === "completed" || task.state === "abandoned";
}

/** Presentation only: retain authoritative records and keep unfinished work visible. */
export default function SharedTaskGroups({ tasks, renderTasks, emptyMessage }: {
  tasks: ApiaryTask[];
  renderTasks: (tasks: ApiaryTask[]) => ReactNode;
  emptyMessage: string;
}) {
  const [showClosed, setShowClosed] = useState(false);
  const open = tasks.filter((task) => !isClosedSharedTask(task));
  const closed = tasks.filter(isClosedSharedTask);
  return <>
    {open.length ? renderTasks(open) : <p className="keeper-empty">{emptyMessage}</p>}
    {closed.length > 0 ? <div className="shared-task-history">
      <button className="secondary-button" type="button" aria-expanded={showClosed} onClick={() => setShowClosed((value) => !value)}>
        {showClosed ? "Hide" : "Show"} closed shared work · {closed.length}
      </button>
      {showClosed ? <section aria-label="Closed shared work">{renderTasks(closed)}</section> : null}
    </div> : null}
  </>;
}
