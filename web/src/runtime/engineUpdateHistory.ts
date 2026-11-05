import type { StoppedWorker, WorkerEngineUpdateAttempt } from "../api";

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
  workers: StoppedWorker[] = [],
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
  // The automatic path that counts and names what it stopped (ADR 0156): it
  // applies an update itself once every worker rests.
  if (attempt.initiated === "automatic" && typeof attempt.stopped_sessions === "number") {
    switch (attempt.outcome) {
      case "succeeded":
        return `Swarm updated the worker engine by itself ${when}, moving ${move}, while every worker was resting. ${whoCameBack(workers, stopped)}`;
      case "timed_out":
        return `Swarm updated the worker engine by itself ${when}, moving ${move}; it ${stopped}, and the engine never reported the new release. Those workers are owed a return.`;
      case "failed":
        return `Swarm tried to update the worker engine by itself ${when} and stopped: ${attempt.detail || "no reason was recorded"}`;
      default:
        return `Swarm started updating the worker engine by itself ${when}, moving ${move}, and ${stopped}, and never recorded how it ended.`;
    }
  }
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
 * Names the workers an automatic update stopped and says whether each is back,
 * because the operator was not there to see it (ADR 0156).
 */
function whoCameBack(workers: StoppedWorker[], stopped: string): string {
  if (workers.length === 0) return `It ${stopped}.`;
  const named = (status: StoppedWorker["status"]) =>
    workers.filter((worker) => worker.status === status).map((worker) => worker.name);
  const all = listed(workers.map((worker) => worker.name));
  const returning = named("returning");
  const missing = named("needs_you");
  if (returning.length === 0 && missing.length === 0) return `It stopped ${all} and brought ${workers.length === 1 ? "it" : "them"} back.`;
  const parts = [`It stopped ${all}.`];
  if (returning.length > 0) parts.push(`${listed(returning)} ${returning.length === 1 ? "is" : "are"} still coming back.`);
  if (missing.length > 0) parts.push(`${listed(missing)} ${missing.length === 1 ? "was" : "were"} not brought back and ${missing.length === 1 ? "is" : "are"} in Needs you.`);
  return parts.join(" ");
}

function listed(names: string[]): string {
  if (names.length <= 1) return names.join("");
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
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
