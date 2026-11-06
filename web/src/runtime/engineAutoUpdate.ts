import type { DevelopmentRuntime, EngineHolder, EngineUpdateRefusal, WorkerEngineAutoUpdateView, WorkerRestart } from "../api";

/** How long an engine update waits before Needs you says so (ADR 0156). */
export const ENGINE_WAIT_ATTENTION_SECONDS = 24 * 60 * 60;

/** A worker the roster shows busy: working, or resting with a command running. */
export type WorkingWorker = { name: string; background: boolean };

/**
 * What the worker engine card says about the automatic path while an update
 * waits: one sentence on what happens and when, then each worker holding it
 * up, why, and what Swarm does about it.
 *
 * The operator, 2026-11-06: "it should be clear what is happening and if not
 * why". The card used to say the same thing twice, once from the roster and
 * once from the engine, and neither said what would happen next.
 */
export type EngineAutoUpdateStatus = {
  summary: string;
  holders: string[];
  /** What Swarm already did while this update waited (ADR 0161). */
  notes: string[];
};

const WAITING_ON = "This update applies by itself once every worker is resting. Waiting on:";

export function engineAutoUpdateStatus(
  view: WorkerEngineAutoUpdateView | null | undefined,
  working: WorkingWorker[] = [],
): EngineAutoUpdateStatus | null {
  if (!view) return null;
  if (!view.enabled) {
    return {
      summary: "Automatic engine updates are off, so this update waits for the button. Turn them back on here.",
      holders: [],
      notes: [],
    };
  }
  const notes = restartedNotes(view.restarted ?? []);
  const busy = working.map((worker) =>
    worker.background ? `${worker.name} has a command running in the background` : `${worker.name} is working`);
  // The engine names every worker it refused (ADR 0162). One the roster
  // already shows working is said once, by the roster.
  const engine = (view.holders ?? [])
    .filter((holder) => !(holder.swarm === "not_resting" && working.some((worker) => worker.name === holder.worker)))
    .map(holderLine);
  if (engine.length > 0) return { summary: WAITING_ON, holders: [...busy, ...engine], notes };
  const refusal = view.refusal;
  if (!refusal || refusal.reason === "workers_busy") {
    if (busy.length > 0) return { summary: WAITING_ON, holders: busy, notes };
    if (refusal) return { summary: WAITING_ON, holders: [capitalize(refusalSentence(refusal))], notes };
    return { summary: "Every worker is resting, so this update applies within a minute.", holders: [], notes };
  }
  if (!refusal.worker) {
    return { summary: `This update applies by itself once every worker is resting. Not now: ${refusalSentence(refusal)}.`, holders: busy, notes };
  }
  return { summary: WAITING_ON, holders: [...busy, capitalize(refusalSentence(refusal))], notes };
}

/** Why the engine refused one worker, then what Swarm does about it. */
function holderLine(holder: EngineHolder): string {
  const worker = holder.worker ?? "a worker";
  const why = capitalize(refusalSentence({ reason: holder.reason, worker, since: 0 }));
  const then = swarmDoes(holder, worker);
  return then ? `${why}. ${then}` : why;
}

function swarmDoes(holder: EngineHolder, worker: string): string {
  switch (holder.swarm) {
    case "restarts": return `Swarm restarts ${worker} at its next check, within a minute.`;
    case "in_use": return `You are using ${worker}'s terminal, so Swarm restarts it once you stop.`;
    case "not_resting": return `Swarm does not read ${worker} as resting either, so it leaves it.`;
    case "already_restarted": return `Swarm already restarted ${worker} once for this update, so use the button.`;
    default: return "";
  }
}

/** One line per moment Swarm restarted workers, naming them together. */
function restartedNotes(restarted: WorkerRestart[]): string[] {
  const byMoment = new Map<string, string[]>();
  for (const restart of restarted) {
    const at = clock(restart.at);
    byMoment.set(at, [...(byMoment.get(at) ?? []), restart.worker]);
  }
  return [...byMoment].map(([at, workers]) => workers.length === 1
    ? `Swarm restarted ${workers[0]} at ${at}, because the engine could not confirm it was resting.`
    : `Swarm restarted ${names(workers)} at ${at}, because the engine could not confirm they were resting.`);
}

function names(workers: string[]): string {
  return workers.length < 2 ? workers.join("") : `${workers.slice(0, -1).join(", ")} and ${workers[workers.length - 1]}`;
}

/** Hours and minutes in the viewer's own time. */
export function clock(at: number): string {
  return new Date(at * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function capitalize(sentence: string): string {
  return sentence.charAt(0).toUpperCase() + sentence.slice(1);
}

/** The engine's or Swarm's reason, in the operator's words. */
export function refusalSentence(refusal: EngineUpdateRefusal): string {
  const worker = refusal.worker ?? "a worker";
  switch (refusal.reason) {
    case "workers_busy": {
      const count = refusal.count ?? 0;
      return count === 1 ? "1 worker session is working" : `${count} worker sessions are working`;
    }
    case "turn_in_progress": return `the engine counts ${worker} as mid-turn`;
    case "unsent_input": return `${worker} has text typed and not sent`;
    case "operator_has_terminal": return `you have ${worker}'s terminal`;
    case "waiting_on_operator": return `${worker} is waiting for an answer`;
    case "background_work": return `${worker} has background work running`;
    case "scheduled_work": return `${worker} has a scheduled job, which a restart ends`;
    case "screen_not_resting": return `${worker} does not look at rest on screen`;
    case "turn_failed": return `${worker}'s last turn failed`;
    case "turns_unaccounted": return `the engine lost count of ${worker}'s turns`;
    case "no_startup_evidence": return `the engine never heard ${worker} start, so it cannot count its turns`;
    case "not_claude": return `${worker} is not a Claude session, and only Claude's turns are measured`;
    case "sessions_changed": return "the sessions changed while Swarm checked";
    case "engine_cannot_check_workers": return "the running engine cannot check that its workers are resting, so apply this one with the button";
    case "protocol_migration_waits": return "this update changes the engine protocol, which restarts Swarm too, so it waits for you";
    case "earlier_attempt_failed": return "the last try at this update stopped workers and did not finish, so it is not tried again by itself";
    case "stopped_partway": return `the last try stopped some workers and could not stop ${worker}, so it is not tried again by itself`;
    case "swap_not_requested": return "the last try stopped workers and could not ask for the swap, so it is not tried again by itself";
    case "engine_draining": return "the engine is already draining for an update";
    case "engine_unreachable": return "the engine did not answer";
    case "answer_lost": return "the engine's answer to the last try was lost; any worker it stopped comes back once the engine is updated";
    case "engine_not_managed": return "this installation does not replace its own worker engine";
    case "session_without_worker": return "a session no worker owns is running, and nothing could bring it back";
    case "returns_unrecorded": return "Swarm could not record who to bring back, so it stopped nobody";
    default: return "it is waiting";
  }
}

export type EngineUpdateAttention =
  | { kind: "stopped_partway"; detail: string; stopped: string[] }
  | { kind: "waiting"; hours: number; reason: string | null; enabled: boolean };

/**
 * Whether the waiting engine update belongs in Needs you.
 *
 * Two cases, and only while the update still waits: an automatic try stopped
 * some workers and could not finish, which names them; or the update has
 * waited a day. Never forced: the card stays the manual route.
 */
export function engineUpdateAttention(
  runtime: DevelopmentRuntime | null | undefined,
  now: number,
): EngineUpdateAttention | null {
  if (!runtime || runtime.worker_engine_update_required !== true) return null;
  const view = runtime.worker_engine_auto_update;
  const last = runtime.last_worker_engine_update;
  if (
    last
    && last.initiated === "automatic"
    && typeof last.stopped_sessions === "number"
    && last.outcome !== "succeeded"
  ) {
    return {
      kind: "stopped_partway",
      detail: last.detail,
      stopped: (view?.last_update_workers ?? [])
        .filter((worker) => worker.status !== "back")
        .map((worker) => worker.name),
    };
  }
  const since = view?.waiting_since;
  if (typeof since !== "number" || now - since < ENGINE_WAIT_ATTENTION_SECONDS) return null;
  return {
    kind: "waiting",
    hours: Math.floor((now - since) / 3600),
    reason: view?.refusal ? refusalSentence(view.refusal) : null,
    enabled: view?.enabled ?? false,
  };
}
