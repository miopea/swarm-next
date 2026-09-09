import type { Health, TerminalHostStatus } from "../api";

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
 */
export function workersMidCommand(workers: { name: string; attention_state: string }[]) {
  return workers
    .filter((worker) => worker.attention_state === "buzzing")
    .map((worker) => worker.name);
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
