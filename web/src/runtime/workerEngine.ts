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
