import { authenticatedFetch } from "./request";

export type SupportSubmission = {
  submission_key: string;
  kind: "feedback" | "bug_report" | "feature_request";
  email: string;
  name: string | null;
  subject: string;
  body: string;
};
export type SupportDelivery = {
  submission_key: string;
  created_at: number;
  delivery: { state: "pending" | "delivering" | "uncertain" | "failed" | "confirmed" | "rate_limited"; attempts: number; attempt_id?: string | null; manual_retry_pending?: boolean; retry_not_before?: number | null; refusal?: "conflict" | "rejected" | "rate_limited" | null; receipt?: { message_id: string } | null };
};
export type SupportStatus = { configured: boolean; sender: "configured" | "running" | "stopped" | "failed" | null; deliveries: SupportDelivery[] };
export type SupportRetry = { submission_key: string; retry_id: string; expected_attempt_id: string };

export async function forgetSupportCopy(token: string, row: SupportDelivery, signal?: AbortSignal): Promise<void> {
  if (row.delivery.state !== "confirmed" || !row.delivery.receipt?.message_id) throw new Error("Only a confirmed local copy can be removed");
  await authenticatedFetch(token, "/api/v1/feedback/support/local-copy", {
    method: "DELETE", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ submission_key: row.submission_key, expected_message_id: row.delivery.receipt.message_id }), signal,
  });
}

export async function retrySupport(token: string, command: SupportRetry, signal?: AbortSignal): Promise<SupportDelivery["delivery"]> {
  const response = await authenticatedFetch(token, "/api/v1/feedback/support/retry", {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(command), signal,
  });
  const result = await response.json() as { delivery?: SupportDelivery["delivery"] & { manual_retry_id: string; manual_retry_expected_attempt: string } };
  if (result.delivery?.manual_retry_id !== command.retry_id
    || result.delivery.manual_retry_expected_attempt !== command.expected_attempt_id) throw new Error("Retry receipt could not be confirmed");
  return result.delivery;
}

export async function fetchSupportStatus(token: string, signal?: AbortSignal): Promise<SupportStatus> {
  const response = await authenticatedFetch(token, "/api/v1/feedback/support", { signal });
  const value = await response.json() as SupportStatus;
  if (typeof value.configured !== "boolean" || !Array.isArray(value.deliveries)) throw new Error("Invalid support status");
  return value;
}

export async function submitSupport(token: string, submission: SupportSubmission, signal?: AbortSignal): Promise<SupportDelivery> {
  const response = await authenticatedFetch(token, "/api/v1/feedback/support", {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(submission), signal,
  });
  const value = await response.json() as SupportDelivery;
  if (value.submission_key !== submission.submission_key || !value.delivery
    || !["pending", "delivering", "uncertain", "failed", "confirmed"].includes(value.delivery.state)) {
    throw new Error("Support save could not be confirmed");
  }
  return value;
}
