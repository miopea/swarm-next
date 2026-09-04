import { authenticatedFetch } from "../api/request";

/** Authorship only. Never sends PTY input or claims provider consumption. */
export async function recordOperatorSubmission(
  operatorToken: string, sessionId: string, text: string, signal: AbortSignal,
): Promise<void> {
  const id = crypto.randomUUID();
  const response = await authenticatedFetch(operatorToken,
    `/api/v1/terminal/sessions/${encodeURIComponent(sessionId)}/submissions`, {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id, text }), signal,
    });
  const result = await response.json() as Record<string, unknown>;
  if (result.id !== id || result.source !== "operator_authored" || result.provider_consumption !== "unconfirmed") {
    throw new Error("Operator source recording was not confirmed");
  }
}
