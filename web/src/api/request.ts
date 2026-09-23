export const BROWSER_SESSION_AUTH = "browser-session-cookie";

// 502/503/504 are the origin restarting or slow. 522/523/524 are the same
// family reported by a proxy in front of it — connection timed out, origin
// unreachable, origin took too long. A 524 is what an operator saw while the
// API was being replaced, on a request that was never retried because this set
// did not recognise it.
const TRANSIENT_RUNTIME_STATUSES = new Set([502, 503, 504, 522, 523, 524]);

export class RuntimeRequestError extends Error {
  /**
   * The server's machine-readable reason, when it gave one.
   *
   * Carried so a caller can act on WHICH refusal this was without matching the
   * prose, which is written for people and may change. Optional because some
   * failures (a proxy, an empty body) have none.
   */
  constructor(public readonly status: number, message: string, public readonly code?: string) {
    super(message);
    this.name = "RuntimeRequestError";
  }
}

export async function authenticatedFetch(
  operatorToken: string,
  url: string,
  init: RequestInit = {},
): Promise<Response> {
  const headers = new Headers(init.headers);
  if (operatorToken !== BROWSER_SESSION_AUTH) headers.set("Authorization", `Bearer ${operatorToken}`);
  const response = await fetch(url, { ...init, headers, cache: "no-store", credentials: "same-origin" });
  if (!response.ok) {
    let detail = "";
    let code: string | undefined;
    try {
      const body = (await response.json()) as { message?: string; code?: string };
      detail = body.message ? `: ${body.message}` : "";
      code = typeof body.code === "string" ? body.code : undefined;
    } catch {
      // Some infrastructure failures return an empty or non-JSON response.
    }
    throw new RuntimeRequestError(response.status, `Runtime request returned ${response.status}${detail}`, code);
  }
  return response;
}

export async function recoverTransientRuntime<T>(
  operation: () => Promise<T>,
  delays = [250, 500, 1_000, 2_000, 4_000, 8_000],
): Promise<T> {
  for (let attempt = 0; ; attempt += 1) {
    try {
      return await operation();
    } catch (error) {
      const retryable = error instanceof TypeError
        || (error instanceof RuntimeRequestError && TRANSIENT_RUNTIME_STATUSES.has(error.status));
      if (!retryable || attempt >= delays.length) throw error;
      await new Promise((resolve) => window.setTimeout(resolve, delays[attempt]));
    }
  }
}
