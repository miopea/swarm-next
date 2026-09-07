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
  delivery: { state: "pending" | "delivering" | "uncertain" | "failed" | "confirmed"; attempts: number };
};
export type SupportStatus = { configured: boolean; sender: "configured" | "running" | "stopped" | "failed" | null; deliveries: SupportDelivery[] };

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
