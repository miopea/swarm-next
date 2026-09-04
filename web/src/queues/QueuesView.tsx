import { useMemo } from "react";
import HeldBriefingList from "../orchestration/HeldBriefingList";
import type { BlockedEscalation, HeldBriefing, HeldDelivery } from "../api";
import DeliveryWaitList from "./DeliveryWaitList";
import type { NextMoveOwner, Task } from "../api/tasks";
import { isOpenTaskState } from "../api/tasks";
import type { Worker } from "../api/workers";

/**
 * Every queue on one screen, grouped by WHO OWES THE NEXT MOVE.
 *
 * Not grouped by mechanism — dispatch, delivery, approvals, engine
 * maintenance. That reads like the system's own diagram and answers the wrong
 * question: one stall then appears in several places, and "why is nothing
 * moving" gets no single answer. Grouped by owner, a growing pile is
 * attributable, which is the whole point.
 *
 * It also exists to keep this OUT of Needs You. That surface should hold only
 * what the operator alone can act on; a card there reading "N pieces of
 * finished work are waiting on Queen" is Queen's backlog rendered in the
 * operator's attention area, and it trains them to ignore the screen that
 * matters.
 */

type Group = {
  owner: NextMoveOwner | "unknown";
  title: string;
  /** What the operator should conclude from this pile growing. */
  meaning: string;
  tasks: Task[];
};

const GROUP_ORDER: readonly Group["owner"][] = ["operator", "queen", "worker", "blocked", "release", "unknown"];

const GROUP_TITLES: Record<Group["owner"], string> = {
  unknown: "Next owner not recorded",
  operator: "Waiting on you",
  queen: "Waiting on Queen",
  worker: "Waiting on a worker",
  blocked: "Blocked on something else",
  release: "Waiting to ship",
  nobody: "Settled",
};

const GROUP_MEANINGS: Record<Group["owner"], string> = {
  unknown: "Open work without a known next owner. This is missing evidence, not a healthy queue.",
  queen: "Work whose next recorded move belongs to Queen.",
  operator: "Reviewed work with a decision open on it. Nobody in the Hive can move these — answering the decision releases them.",
  worker: "Work a worker owns — in progress, or handed back for something missing.",
  blocked: "Nothing here can move these. A hard reason, such as waiting on another task.",
  release: "Finished and accepted. These close themselves when the work ships.",
  nobody: "Closed.",
};

/**
 * Age of the oldest item, which is the number that says whether a pile is a
 * queue or a stall. A count alone cannot: fifteen items are fine if they turn
 * over hourly and alarming if the oldest is a day old.
 */
function oldestAgeHours(tasks: Task[], now: number): number | undefined {
  const oldest = tasks.reduce<number | undefined>(
    (earliest, task) => (earliest === undefined || task.updated_at < earliest ? task.updated_at : earliest),
    undefined,
  );
  return oldest === undefined ? undefined : Math.max(0, Math.floor((now / 1000 - oldest) / 3600));
}

function ageLabel(hours: number): string {
  if (hours < 1) return "under an hour";
  if (hours < 48) return `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}

export default function QueuesView({
  tasks,
  workers,
  onOpenTask,
  heldBriefings = [],
  blockedWaits = [],
  heldDeliveries = [],
  coordinatorUnavailable = false,
  now = Date.now(),
}: {
  tasks: Task[];
  workers: Worker[];
  onOpenTask: (taskId: string) => void;
  /**
   * Briefings Swarm is holding until their worker is free.
   *
   * HERE RATHER THAN ON NEEDS YOU, which is this file's whole stated purpose.
   * The card's own text is "Nothing is wrong with these — Swarm is holding them
   * until the worker is free", and that sentence was being printed under a
   * heading promising things that need the operator. It is a queue, so it is on
   * the queues page. Not deleted: nothing else under web/src reads this, and
   * losing it would trade one defect for a blind spot.
   */
  heldBriefings?: HeldBriefing[];
  blockedWaits?: BlockedEscalation[];
  heldDeliveries?: HeldDelivery[];
  coordinatorUnavailable?: boolean;
  now?: number;
}) {
  const workerNames = useMemo(
    () => new Map(workers.map((worker) => [worker.id, worker.name])),
    [workers],
  );

  const groups = useMemo<Group[]>(() => {
    const open = tasks.filter((task) => isOpenTaskState(task.state));
    return GROUP_ORDER.map((owner) => ({
      owner,
      title: GROUP_TITLES[owner],
      meaning: GROUP_MEANINGS[owner],
      // Older servers omit the field. Nothing is invented for them: a task with
      // no stated owner is left out rather than assigned to somebody who might
      // then be blamed for a queue that is not theirs.
      tasks: open.filter((task) => owner === "unknown"
        ? !GROUP_ORDER.includes(task.next_move_owner as Group["owner"])
        : task.next_move_owner === owner),
    })).filter((group) => group.tasks.length > 0);
  }, [tasks]);

  const total = groups.reduce((sum, group) => sum + group.tasks.length, 0);
  const waits = new Map(blockedWaits.map((wait) => [wait.task_id, wait]));
  // Do not duplicate a task already in the main queue or resurrect one known
  // to be closed from an older, independently polled coordinator response.
  const known = new Set(tasks.map((task) => task.id));
  const extraWaits = blockedWaits.filter((wait) => !known.has(wait.task_id));

  if (total === 0 && extraWaits.length === 0) {
    return (
      <section className="queues" aria-label="Queues">
        {coordinatorUnavailable && <p role="status">Coordination status could not refresh. Showing last known work; it may have changed.</p>}
        {!coordinatorUnavailable && heldBriefings.length === 0 && heldDeliveries.length === 0 && <p className="queues-empty">Nothing is waiting on anyone.</p>}
        <DeliveryWaitList held={heldDeliveries} />
        <HeldBriefingList briefings={heldBriefings} onOpenTask={onOpenTask} />
      </section>
    );
  }

  return (
    <section className="queues" aria-label="Queues">
      {coordinatorUnavailable && <p role="status">Coordination status could not refresh. Showing last known work; it may have changed.</p>}
      {groups.map((group) => {
        const hours = oldestAgeHours(group.tasks, now);
        return (
          <article key={group.owner} className="queue-group" data-owner={group.owner}>
            <header>
              <h2>
                {group.title} <span className="queue-count">{group.tasks.length}</span>
              </h2>
              <p className="queue-meaning">{group.meaning}</p>
              {hours === undefined ? null : (
                <p className="queue-oldest">Oldest {ageLabel(hours)}</p>
              )}
            </header>
            <ul>
              {group.tasks.map((task) => (
                <li key={task.id}>
                  <button type="button" onClick={() => onOpenTask(task.id)}>
                    <span className="queue-task-title">{task.title}</span>
                    <span className="queue-task-meta">
                      {task.assigned_worker_id
                        ? (workerNames.get(task.assigned_worker_id) ?? "assigned")
                        : "unassigned"}
                    </span>
                    {task.state === "blocked" && waits.has(task.id) && <span className="queue-task-meta">Blocked for {ageLabel(Math.max(0, Math.floor(waits.get(task.id)!.blocked_for_seconds / 3600)))} · Queen coordinates the next move</span>}
                  </button>
                </li>
              ))}
            </ul>
          </article>
        );
      })}
      <DeliveryWaitList held={heldDeliveries} />
      {extraWaits.length > 0 && <article className="queue-group" data-owner="blocked">
        <header><h2>Blocked work awaiting reconciliation <span className="queue-count">{extraWaits.length}</span></h2>
          <p className="queue-meaning">Reported by the coordinator but absent from the current task list. Age alone does not require your approval.</p></header>
        <ul>{extraWaits.map((wait) => <li key={wait.task_id}><button type="button" onClick={() => onOpenTask(wait.task_id)}>
          <span className="queue-task-title">{wait.title}</span>
          <span className="queue-task-meta">{wait.worker_name} · blocked {ageLabel(Math.max(0, Math.floor(wait.blocked_for_seconds / 3600)))}</span>
        </button></li>)}</ul>
      </article>}
      <HeldBriefingList briefings={heldBriefings} onOpenTask={onOpenTask} />
    </section>
  );
}
