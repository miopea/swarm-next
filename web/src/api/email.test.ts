import { afterEach, expect, test, vi } from "vitest";

import {
  beginEmailAuthorization,
  disconnectEmail,
  fetchEmailAttachmentPreview,
  fetchEmailConfiguration,
  fetchEmailInbox,
  fetchEmailMessage,
  fetchEmailReadiness,
  fetchEmailReply,
  fetchEmailTaskSource,
  fetchEmailTaskSources,
  fetchTaskDeployments,
  importEmailMessage,
  importEmailTask,
  prepareEmailReply,
  recordTaskDeployment,
  retryEmailReply,
  sendEmailReply,
  updateEmailConfiguration,
  updateEmailReplyDraft,
} from "../api";

const summary = {
  id: "message/one", conversation_id: "conversation-1", internet_message_id: null,
  subject: "Issue report", sender_name: "Ari", sender_address: "ari@example.test",
  received_at: 1, web_url: "https://outlook.example.test/message", has_attachments: true, preview: "Please fix this",
};
const source = {
  id: "source-1", task_id: "task/one", integration_id: "email-1", message_id: summary.id,
  conversation_id: summary.conversation_id, internet_message_id: null, sender_name: summary.sender_name,
  sender_address: summary.sender_address, received_at: 1, web_url: summary.web_url, imported_at: 2, attachments: [],
};
const task = {
  id: "task/one", hive_id: "hive-1", title: "Issue report", description: "Please fix this",
  priority: "normal", workspace: "/projects/swarm-next", state: "ready",
  assigned_worker_id: null, assigned_session_id: null, position: 1, created_at: 1, updated_at: 1,
};
const emailImport = { task, source, sources: [source], created: true };
const deployment = { id: "deployment-1", task_id: task.id, environment: "staging", reference: "build-1", deployed_at: 3, recorded_at: 3 };
const reply = { id: "reply/one", task_id: task.id, body: "The issue is fixed.", state: "draft", attempts: 0, attempted_at: null, delivered_at: null, last_error: null, targets: [] };

afterEach(() => vi.unstubAllGlobals());

function response(payload: unknown): Response {
  return payload === null
    ? new Response(null, { status: 204 })
    : new Response(JSON.stringify(payload), { status: 200, headers: { "content-type": "application/json" } });
}

test("owns bounded email configuration, inbox, preview, source, deployment, and reply reads", async () => {
  const payloads = [
    { configured: true, connection: "ready", account_name: "Bea", account_address: "bea@example.test" },
    { configured: true, managed_by: "operator", tenant_id: "tenant", client_id: "client", callback_url: "https://swarm.test/callback", secret_stored: true },
    [summary],
    { summary, body_text: "Please fix this", attachments: [{ id: "attachment/one", name: "screen.png", media_type: "image/png", byte_size: 100, inline: true, content_id: "image-1" }] },
    { attachment: "bytes" },
    source,
    [source],
    [deployment],
    reply,
  ];
  const fetch = vi.fn().mockImplementation(() => Promise.resolve(response(payloads.shift())));
  vi.stubGlobal("fetch", fetch);

  await expect(fetchEmailReadiness("operator")).resolves.toMatchObject({ connection: "ready" });
  await expect(fetchEmailConfiguration("operator")).resolves.toMatchObject({ secret_stored: true });
  await expect(fetchEmailInbox("operator", "  issue report  ")).resolves.toHaveLength(1);
  await expect(fetchEmailMessage("operator", "message/one")).resolves.toMatchObject({ body_text: "Please fix this" });
  await expect(fetchEmailAttachmentPreview("operator", "message/one", "attachment/one")).resolves.toBeInstanceOf(Blob);
  await expect(fetchEmailTaskSource("operator", "task/one")).resolves.toEqual(source);
  await expect(fetchEmailTaskSources("operator")).resolves.toEqual([source]);
  await expect(fetchTaskDeployments("operator", "task/one")).resolves.toEqual([deployment]);
  await expect(fetchEmailReply("operator", "task/one")).resolves.toEqual(reply);

  expect(fetch).toHaveBeenNthCalledWith(3, "/api/v1/integrations/email/inbox?query=issue+report", expect.any(Object));
  expect(fetch).toHaveBeenNthCalledWith(4, "/api/v1/integrations/email/messages/message%2Fone", expect.any(Object));
  expect(fetch).toHaveBeenNthCalledWith(5, "/api/v1/integrations/email/messages/message%2Fone/attachments/attachment%2Fone", expect.any(Object));
  expect(fetch).toHaveBeenNthCalledWith(6, "/api/v1/tasks/task%2Fone/email", expect.any(Object));
});

test("serializes email setup, import, deployment evidence, and reviewed reply commands", async () => {
  const configuration = { configured: true, managed_by: "operator", tenant_id: "tenant", client_id: "client", callback_url: "https://swarm.test/callback", secret_stored: true };
  const payloads = [configuration, { authorization_url: "https://login.example.test/authorize" }, null, emailImport, emailImport, deployment, reply, reply, reply, reply];
  const fetch = vi.fn().mockImplementation(() => Promise.resolve(response(payloads.shift())));
  vi.stubGlobal("fetch", fetch);

  await updateEmailConfiguration("operator", "tenant", "client", "one-time-secret");
  await expect(beginEmailAuthorization("operator")).resolves.toContain("authorize");
  await disconnectEmail("operator");
  await expect(importEmailMessage("operator", "message/one", "high")).resolves.toEqual(emailImport);
  const importInput = { message_ids: ["message/one", "message-two"], title: "Merged issue", description: "Two reports", priority: "urgent" as const, worker_id: null, state: "ready" as const };
  await expect(importEmailTask("operator", importInput)).resolves.toEqual(emailImport);
  await recordTaskDeployment("operator", "task/one", "production", "release-42");
  await prepareEmailReply("operator", "task/one", "The issue is fixed.");
  await updateEmailReplyDraft("operator", "task/one", "The issue is fixed and verified.");
  await sendEmailReply("operator", "reply/one");
  await retryEmailReply("operator", "reply/one");

  expect(fetch).toHaveBeenNthCalledWith(1, "/api/v1/integrations/email/configuration", expect.objectContaining({
    method: "PUT", body: JSON.stringify({ tenant_id: "tenant", client_id: "client", client_secret: "one-time-secret" }),
  }));
  expect(fetch).toHaveBeenNthCalledWith(4, "/api/v1/integrations/email/messages/message%2Fone/import", expect.objectContaining({ body: JSON.stringify({ priority: "high" }) }));
  expect(fetch).toHaveBeenNthCalledWith(5, "/api/v1/integrations/email/import", expect.objectContaining({ body: JSON.stringify(importInput) }));
  expect(fetch).toHaveBeenNthCalledWith(6, "/api/v1/tasks/task%2Fone/deployments", expect.objectContaining({ body: JSON.stringify({ environment: "production", reference: "release-42" }) }));
  expect(fetch).toHaveBeenNthCalledWith(9, "/api/v1/integrations/email/replies/reply%2Fone/send", expect.objectContaining({ method: "POST" }));
  expect(fetch).toHaveBeenNthCalledWith(10, "/api/v1/integrations/email/replies/reply%2Fone/retry", expect.objectContaining({ method: "POST" }));
});
