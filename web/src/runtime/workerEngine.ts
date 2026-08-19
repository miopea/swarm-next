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
 * "Loaded" and "working" are different questions, and only the second one
 * costs the operator anything: replacing the engine while a worker is resting
 * loses nothing, while doing it mid-command kills work in progress. The count
 * of sessions comes from the host, which is what actually gets stopped; which
 * of them are busy comes from the roster, which is what knows.
 */
export function workersMidCommand(workers: { name: string; attention_state: string }[]) {
  return workers
    .filter((worker) => worker.attention_state === "buzzing")
    .map((worker) => worker.name);
}

/** How the confirmation should describe the cost of updating right now. */
export function engineUpdateCost(busyNames: string[]): string {
  if (busyNames.length === 0) {
    return "No worker is running a command right now, so nothing in progress is lost.";
  }
  const named = busyNames.length <= 3
    ? busyNames.join(", ")
    : `${busyNames.slice(0, 3).join(", ")} and ${busyNames.length - 3} more`;
  return `${busyNames.length} worker${busyNames.length === 1 ? " is" : "s are"} running a command right now: ${named}. That work is interrupted and is not resumed.`;
}
