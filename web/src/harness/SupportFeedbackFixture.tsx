import { useState } from "react";
import SupportFeedbackDialog from "../feedback/SupportFeedbackDialog";
import type { SupportDelivery, SupportStatus, SupportSubmission } from "../api/support";

const rows: SupportDelivery[] = [
  { submission_key: "00000000-0000-0000-0000-000000000001", created_at: 1788760000,
    delivery: { state: "confirmed", attempts: 1, receipt: { message_id: "00000000-0000-0000-0000-000000000011" } } },
  { submission_key: "00000000-0000-0000-0000-000000000002", created_at: 1788760100,
    delivery: { state: "uncertain", attempts: 5, attempt_id: "00000000-0000-0000-0000-000000000012" } },
];
const state = (): SupportStatus => ({ configured: true, sender: "running", deliveries: [...rows] });
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status, headers: { "content-type": "application/json" } });

/** In-memory fictional responses only. Imported exclusively by the no-proxy harness. */
export function supportFixtureResponse(path: string, init?: RequestInit): Response | undefined {
  if (!path.startsWith("/api/v1/feedback/support")) return;
  if (path.endsWith("/local-copy")) {
    const input = JSON.parse(String(init?.body)) as { submission_key: string };
    const index = rows.findIndex((row) => row.submission_key === input.submission_key && row.delivery.state === "confirmed");
    if (index >= 0) rows.splice(index, 1);
    return new Response(null, { status: 204 });
  }
  if (path.endsWith("/retry")) {
    const input = JSON.parse(String(init?.body)) as { submission_key: string; retry_id: string; expected_attempt_id: string };
    const row = rows.find((row) => row.submission_key === input.submission_key)!;
    row.delivery.manual_retry_pending = true;
    return json({ delivery: { ...row.delivery, manual_retry_id: input.retry_id, manual_retry_expected_attempt: input.expected_attempt_id } }, 202);
  }
  if (init?.method === "POST") {
    const input = JSON.parse(String(init.body)) as SupportSubmission;
    let row = rows.find((entry) => entry.submission_key === input.submission_key);
    if (!row) {
      row = { submission_key: input.submission_key, created_at: Math.floor(Date.now() / 1000), delivery: { state: "pending", attempts: 0 } };
      rows.unshift(row);
    }
    return json(row, 202);
  }
  return json(state());
}

export default function SupportFeedbackFixture() {
  const [open, setOpen] = useState(true);
  return <main style={{ padding: 24 }}><h1>Fictional support fixture</h1>
    <p>Real UI, in-memory sample reports. No Hive, credentials, email or central service.</p>
    <button className="primary-action" onClick={() => setOpen(true)}>Open support fixture</button>
    {open && <SupportFeedbackDialog operatorToken="fictional-harness-only" status={state()} onClose={() => setOpen(false)} />}
  </main>;
}
