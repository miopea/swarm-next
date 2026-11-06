import type { Health, TerminalHostStatus } from "../api";
import type { WorkingWorker } from "./engineAutoUpdate";

export function workerEngineUpdateRequired(
  health: Health | undefined,
  host: TerminalHostStatus | undefined,
) {
  if (!health || !host) return false;
  if (health.worker_engine_build_id && host.host_build_id) {
    return health.worker_engine_build_id !== host.host_build_id;
  }
  return health.version !== host.host_version;
}

export function workerEngineMatches(health: Health, host: TerminalHostStatus) {
  return !workerEngineUpdateRequired(health, host);
}

/**
 * The workers a worker-engine replacement would interrupt mid-command.
 *
 * The roster is an activity observation, not maintenance admission. Resting
 * cannot rule out provider background work or an accepted pending submission.
 * A resting worker with a shell it can see running is counted: the engine
 * update stops that shell.
 */
export function workersMidCommand(workers: RosterWorker[]) {
  return workersHoldingTheEngine(workers).map((worker) => worker.name);
}

type RosterWorker = { name: string; attention_state: string; background_work?: boolean };

/** The same workers, saying which only has a command running in the background. */
export function workersHoldingTheEngine(workers: RosterWorker[]): WorkingWorker[] {
  return workers
    .filter((worker) => worker.attention_state === "buzzing"
      || (worker.attention_state === "resting" && worker.background_work === true))
    .map((worker) => ({ name: worker.name, background: worker.attention_state !== "buzzing" }));
}

export const ENGINE_RESTART_CONSEQUENCE = "Loaded worker processes stop. Swarm attempts to return them to their saved conversations; interrupted commands are not resumed automatically and unsent input may be lost.";

/** How the confirmation should describe the cost of updating right now. */
export function engineUpdateCost(busyNames: string[]): string {
  if (busyNames.length === 0) {
    return `No worker currently shows as working. This is not proof that restarting is safe. ${ENGINE_RESTART_CONSEQUENCE}`;
  }
  const named = busyNames.length <= 3
    ? busyNames.join(", ")
    : `${busyNames.slice(0, 3).join(", ")} and ${busyNames.length - 3} more`;
  return `${busyNames.length} worker${busyNames.length === 1 ? " shows" : "s show"} as working: ${named}. ${ENGINE_RESTART_CONSEQUENCE}`;
}
