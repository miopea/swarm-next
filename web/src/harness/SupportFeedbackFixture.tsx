import { useState } from "react";
import SupportFeedbackDialog from "../feedback/SupportFeedbackDialog";
import type { SupportDelivery, SupportStatus, SupportSubmission } from "../api/support";
import { prepareSupportFiles, savePendingSupportFiles } from "../feedback/supportFiles";

const rows: SupportDelivery[] = [
  { submission_key: "00000000-0000-0000-0000-000000000001", created_at: 1788760000,
    delivery: { state: "confirmed", attempts: 1, receipt: { message_id: "00000000-0000-0000-0000-000000000011" } } },
  { submission_key: "00000000-0000-0000-0000-000000000002", created_at: 1788760100,
    delivery: { state: "uncertain", attempts: 5, attempt_id: "00000000-0000-0000-0000-000000000012" } },
];
const state = (): SupportStatus => ({ configured: true, attachments_supported: true, sender: "running", deliveries: [...rows] });
let failFirstUpload = true;
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status, headers: { "content-type": "application/json" } });

/** In-memory fictional responses only. Imported exclusively by the no-proxy harness. */
export function supportFixtureResponse(path: string, init?: RequestInit): Response | Promise<Response> | undefined {
  const query = new URLSearchParams(location.search);
  const identity = query.get("identity");
  if (query.get("surface") === "support-feedback" && (identity === "single" || identity === "multiple")) {
    if (path === "/api/v1/hive/public-profile") return json({ revision: 1,
      profile: { hive_name: "My Hive", operator_display_name: "Operator", contact_email: null } });
    if (path === "/api/v1/integrations/jira/readiness") return json({ configured: true,
      connection: "ready", account_name: "Bea Bee", account_address: "bea@example.test" });
    if (path === "/api/v1/integrations/email/readiness") return json(identity === "multiple"
      ? { configured: true, connection: "ready", account_name: "Cora Bee", account_address: "cora@example.test" }
      : { configured: false, connection: "not_connected", account_name: null, account_address: null });
  }
  if (!path.startsWith("/api/v1/feedback/support")) return;
  if (path.endsWith("/attachments")) {
    return (async () => {
      if (failFirstUpload) { failFirstUpload = false; throw new Error("Fictional lost upload response"); }
      const form = await new Response(init?.body, { headers: init?.headers }).formData();
      const input = JSON.parse(String(form.get("manifest"))) as { submission: SupportSubmission; attachments: { id: string }[] };
      if (!input.attachments.every((file) => form.has(`file:${file.id}`))) return json({ error: "Fixture file is missing" }, 400);
      let row = rows.find((entry) => entry.submission_key === input.submission.submission_key);
      if (!row) { row = { submission_key: input.submission.submission_key, created_at: Math.floor(Date.now()/1000), delivery: { state: "pending", attempts: 0 } }; rows.unshift(row); }
      return json(row, 202);
    })();
  }
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
  const [error, setError] = useState("");
  async function prepareFiles() {
    try {
      const files = await prepareSupportFiles([new File(["Fictional upload evidence. No customer content."],"mobile-reconnect-fictional.txt",{type:"text/plain"}),
        new File(["Fictional diagnostic notes, selected explicitly."],"selected-diagnostic-notes.txt",{type:"text/plain"})]);
      await savePendingSupportFiles({ submission: { submission_key: crypto.randomUUID(),kind:"bug_report",email:"fixture@example.invalid",name:null,
        subject:"Fictional mobile reconnect report",body:"Please review these two explicitly selected fictional files. No real diagnostics or customer data are included." },files });
      setOpen(true);
    } catch (failure) { setError(String(failure)); }
  }
  return <main style={{ padding: 24 }}><h1>Fictional support fixture</h1>
    <p>Real UI, in-memory sample reports. No Hive, credentials, email or central service.</p>
    <button className="primary-action" onClick={() => setOpen(true)}>Open support fixture</button>
    {!open && <button className="secondary-button" onClick={() => void prepareFiles()}>Prepare fictional attachment retry</button>}
    <p>The first file-upload attempt after each page load fails deliberately. Retry stays in this fixture; no external send.</p>
    {error && <p role="alert">{error}</p>}
    {open && <SupportFeedbackDialog operatorToken="fictional-harness-only" status={state()} onClose={() => setOpen(false)} />}
  </main>;
}
