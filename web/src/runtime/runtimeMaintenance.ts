import { RuntimeRequestError } from "../api";

const HANDOFF_STATUSES = new Set([502, 503, 504]);

export function isExpectedRuntimeHandoff(error: unknown): boolean {
  return error instanceof TypeError
    || (error instanceof RuntimeRequestError && HANDOFF_STATUSES.has(error.status));
}

/**
 * A maintenance request may successfully stop or replace the API before its
 * response crosses the reverse proxy. The following health probe, rather than
 * that interrupted response, is authoritative for completion.
 */
export async function requestRuntimeHandoff(action: () => Promise<void>): Promise<void> {
  try {
    await action();
  } catch (error) {
    if (!isExpectedRuntimeHandoff(error)) throw error;
  }
}
