import type { SupportSubmission } from "../api/support";

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
