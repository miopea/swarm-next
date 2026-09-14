import { authenticatedFetch } from "./request";

/** One workspace's spend over the window, and over the window before it. */
export type WorkspaceUsage = {
  workspace: string;
  /**
   * Every worker configured against this workspace.
   *
   * PLURAL BECAUSE IT GENUINELY CAN BE. A provider transcript directory is
   * derived from the workspace PATH, so two workers in one repository land in
   * the same files and cannot be told apart. Naming one of them here would be
   * a precision the numbers do not have.
   */
  workers: string[];
  input_tokens: number;
  cache_write_tokens: number;
  cache_read_tokens: number;
  output_tokens: number;
  messages: number;
  /** Billing-weighted, in input-token equivalents. Never a price. */
  weighted: number;
  weighted_previous: number;
};

export type DayUsage = {
  day: string;
  input_tokens: number;
  cache_write_tokens: number;
  cache_read_tokens: number;
  output_tokens: number;
  weighted: number;
};

export type UsageReport = {
  days: number;
  /**
   * When the last pass over the transcripts finished, or null if none has.
   *
   * The panel prints this rather than implying the figures are live: the first
   * pass on a busy Hive reads gigabytes and takes minutes, and a number with no
   * stated age is the kind that gets trusted for longer than it deserves.
   */
  last_scan_at: number | null;
  scanning: boolean;
  by_workspace: WorkspaceUsage[];
  by_day: DayUsage[];
  total_weighted: number;
  total_weighted_previous: number;
};

/**
 * Reads what each workspace has spent with its provider.
 *
 * Returns immediately with whatever the last pass stored; asking is also what
 * schedules the next pass, so the panel never waits on one.
 */
export async function fetchUsage(
  operatorToken: string,
  days: number,
  signal?: AbortSignal,
): Promise<UsageReport> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/usage?days=${days}`, { signal });
  return normalizeUsage(await response.json(), days);
}

/**
 * ⚠️ THE PANEL MUST NOT TRUST THE SHAPE IT IS HANDED.
 *
 * Found by a test that stubs the network generically: a body without
 * `by_day` reached the component, `.reduce` was called on undefined, and the
 * WHOLE settings surface went blank -- not just this card. That is not a
 * test-only case. An API one reload behind, a proxy error page, a truncated
 * response: each produces a body that parses as JSON and is missing fields, and
 * a usage panel is not worth taking the rest of Settings down for.
 *
 * Missing lists become empty ones, which the panel already renders as "nothing
 * recorded". Wrong is better said than crashed.
 */
export function normalizeUsage(body: unknown, days: number): UsageReport {
  const source = (body ?? {}) as Partial<UsageReport>;
  return {
    days: typeof source.days === "number" ? source.days : days,
    last_scan_at: typeof source.last_scan_at === "number" ? source.last_scan_at : null,
    scanning: source.scanning === true,
    by_workspace: Array.isArray(source.by_workspace) ? source.by_workspace : [],
    by_day: Array.isArray(source.by_day) ? source.by_day : [],
    total_weighted: typeof source.total_weighted === "number" ? source.total_weighted : 0,
    total_weighted_previous:
      typeof source.total_weighted_previous === "number" ? source.total_weighted_previous : 0,
  };
}
