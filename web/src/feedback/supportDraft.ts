import type { SupportSubmission, SupportRetry, SupportDelivery } from "../api/support";

const KEY = "swarm.support.pending.v1";
const LIMIT = 128 * 1024;

/** Keep the exact reviewed key/payload across reloads, never auto-send on recovery. */
export function savePendingSupport(input: SupportSubmission): void {
  const encoded = JSON.stringify(input);
  if (encoded.length > LIMIT) throw new Error("Report is too large to retain for safe retry.");
  sessionStorage.setItem(KEY, encoded);
}

export function loadPendingSupport(): SupportSubmission | undefined {
  const encoded = sessionStorage.getItem(KEY);
  if (!encoded) return;
  if (encoded.length > LIMIT) throw new Error("Saved report is unreadable; do not submit a replacement until its status is checked.");
  const value = JSON.parse(encoded) as SupportSubmission;
  if (!value || !/^[0-9a-f-]{36}$/i.test(value.submission_key)
    || !["feedback", "bug_report", "feature_request"].includes(value.kind)
    || typeof value.email !== "string" || !(value.name === null || typeof value.name === "string")
    || typeof value.subject !== "string" || typeof value.body !== "string"
    || Object.keys(value).some((key) => !["submission_key", "kind", "email", "name", "subject", "body"].includes(key))) {
    throw new Error("Saved report is unreadable; check its delivery before creating another.");
  }
  return value;
}

export function clearPendingSupport(): void { sessionStorage.removeItem(KEY); }

const RETRY_KEY = "swarm.support.retry.v1";
export function prepareSupportRetry(row: SupportDelivery): SupportRetry {
  const encoded = sessionStorage.getItem(RETRY_KEY);
  if (encoded) {
    if (encoded.length > 512) throw new Error("Saved retry is unreadable.");
    const pending = JSON.parse(encoded) as SupportRetry;
    if (Object.keys(pending).length !== 3 || ![pending.submission_key, pending.retry_id, pending.expected_attempt_id]
      .every((value) => typeof value === "string" && /^[0-9a-f-]{36}$/i.test(value))) throw new Error("Saved retry is unreadable.");
    if (pending.submission_key !== row.submission_key) throw new Error("Confirm the previous report's retry before requesting another.");
    return pending;
  }
  if (!row.delivery.attempt_id) throw new Error("Refresh this report before retrying.");
  const command = { submission_key: row.submission_key, retry_id: crypto.randomUUID(), expected_attempt_id: row.delivery.attempt_id };
  sessionStorage.setItem(RETRY_KEY, JSON.stringify(command));
  return command;
}
export function clearSupportRetry(): void { sessionStorage.removeItem(RETRY_KEY); }
