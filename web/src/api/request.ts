export const BROWSER_SESSION_AUTH = "browser-session-cookie";

const TRANSIENT_RUNTIME_STATUSES = new Set([502, 503, 504]);

export class RuntimeRequestError extends Error {
  constructor(public readonly status: number, message: string) {
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
    try {
      const body = (await response.json()) as { message?: string };
      detail = body.message ? `: ${body.message}` : "";
    } catch {
      // Some infrastructure failures return an empty or non-JSON response.
    }
    throw new RuntimeRequestError(response.status, `Runtime request returned ${response.status}${detail}`);
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
