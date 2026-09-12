import type { WorkerEngineUpdateAttempt } from "../api";

/**
 * What happened the last time this Hive replaced its worker engine.
 *
 * ⚠️ THE MOST IMPORTANT SENTENCE THIS PRODUCES IS ABOUT NOT KNOWING. A protocol
 * migration replaces the API process mid-update, so the code that records the
 * ending can be gone before it runs. The row then carries no outcome, and an
 * operator reading a card that simply said nothing would take that for "fine".
 *
 * Before this existed the only witness to any of it was the journal, so the
 * question "did the update go through, and did it take my workers with it" had
 * no answer on the screen the operator was already looking at.
 */
export function engineUpdateHistory(
  attempt: WorkerEngineUpdateAttempt | null | undefined,
  now: number,
): string | null {
  if (!attempt) return null;
  const when = startedAgo(attempt.started_at, now);
  const move = attempt.from_version === attempt.to_version
    ? `to ${attempt.to_version}`
    : `from ${attempt.from_version} to ${attempt.to_version}`;
  const protocolNote = typeof attempt.to_protocol === "number"
    ? ` This was a protocol migration, which cannot preserve running terminals.`
    : "";
  const stopped = workersStopped(attempt.stopped_sessions);

  // ⚠️ SAID FIRST, AND SAID PLAINLY. Operator decision 01a092cd: automatic
  // replacement stays, but it must announce itself. Most engine swaps on a Hive
  // are this one — a timer takes the workers down whenever none reports
  // mid-turn — and the previous sentence read as a record of something the
  // operator had done.
  if (attempt.initiated === "automatic") {
    return `Swarm replaced the worker engine on its own ${when}, moving ${move}. Nobody was asked and nobody chose the moment; it goes ahead whenever no worker reports being mid-turn, and it ${stopped}.${protocolNote}`;
  }

  switch (attempt.outcome) {
    case "succeeded":
      return `The last worker engine update ${when} moved ${move} and ${stopped}.${protocolNote}`;
    case "timed_out":
      return `The last worker engine update ${when} moved ${move}, ${stopped}, and the engine never reported the new release. Those workers are recorded as owed a return.${protocolNote}`;
    case "failed":
      return `The last worker engine update ${when} tried to move ${move} and could not: ${attempt.detail || "no reason was recorded"}.${protocolNote}`;
    default:
      // Deliberately says what is and is not known. "Started and never reported
      // back" is usually the API being replaced by the very update it was
      // running, which is expected — and is still not the same as success.
      return `A worker engine update ${when} started moving ${move} and ${stopped}, and never recorded how it ended. That is what a protocol migration looks like when it replaces Swarm itself; it is not confirmation that it worked.${protocolNote}`;
  }
}

/**
 * ⚠️ "NOBODY COUNTED" IS NOT "IT COST NOTHING". An observed swap learns only
 * that the engine moved, so saying it stopped no workers would be the one
 * reassuring thing this sentence must never invent.
 */
function workersStopped(count: number | null | undefined): string {
  if (count === null || count === undefined) {
    return "stopped an unrecorded number of worker sessions";
  }
  if (count === 0) return "stopped no worker sessions";
  return `stopped ${count} worker session${count === 1 ? "" : "s"}`;
}

/**
 * Relative, because "is this the update I just ran" is the question being asked.
 *
 * An unreadable or future stamp says so rather than rendering "in -3 minutes" or
 * a confident zero.
 */
function startedAgo(startedAt: number, now: number): string {
  const seconds = Math.round(now - startedAt);
  if (!Number.isFinite(seconds)) return "at an unreadable time";
  if (seconds < 0) return "stamped in the future";
  if (seconds < 60) return "moments ago";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} minute${minutes === 1 ? "" : "s"} ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `${hours} hour${hours === 1 ? "" : "s"} ago`;
  return `${Math.round(hours / 24)} days ago`;
}
