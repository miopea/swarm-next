import type { JiraConnectionState } from "./jira";
import { authenticatedFetch } from "./request";
import type { Task, TaskPriority } from "./tasks";

export type EmailConnectionState = JiraConnectionState;
export type EmailReadiness = {
  configured: boolean;
  connection: EmailConnectionState;
  account_name: string | null;
  account_address: string | null;
};
export type EmailOAuthConfiguration = {
  configured: boolean;
  managed_by: "environment" | "operator" | null;
  tenant_id: string | null;
  client_id: string | null;
  callback_url: string | null;
  secret_stored: boolean;
};
export type EmailAttachment = { id: string; name: string; media_type: string; byte_size: number; inline: boolean; content_id: string | null };
export type EmailMessageSummary = {
  id: string;
  conversation_id: string;
  internet_message_id: string | null;
  subject: string;
  sender_name: string;
  sender_address: string;
  received_at: number;
  web_url: string;
  has_attachments: boolean;
  preview: string;
};
export type EmailMessage = { summary: EmailMessageSummary; body_text: string; attachments: EmailAttachment[] };
export type EmailTaskAttachment = { storage_name: string; display_name: string; media_type: string; byte_size: number; inline: boolean; content_id: string | null };
export type EmailTaskSource = {
  id: string;
  task_id: string;
  integration_id: string;
  message_id: string;
  conversation_id: string;
  internet_message_id: string | null;
  sender_name: string;
  sender_address: string;
  received_at: number;
  web_url: string;
  imported_at: number;
  attachments: EmailTaskAttachment[];
};
export type EmailImport = { task: Task; source: EmailTaskSource; sources: EmailTaskSource[]; created: boolean };
export type EmailTaskImportInput = {
  message_ids: string[];
  title: string;
  description: string;
  priority: TaskPriority;
  worker_id: string | null;
  state: "draft" | "ready";
};
export type TaskDeployment = { id: string; task_id: string; environment: string; reference: string; deployed_at: number; recorded_at: number };
export type EmailReplyState = "draft" | "queued" | "dispatching" | "delivered" | "uncertain" | "cancelled";
export type EmailReplyTarget = {
  id: string;
  source_id: string;
  sender_name: string;
  sender_address: string;
  web_url: string;
  state: EmailReplyState;
  attempts: number;
  attempted_at: number | null;
  delivered_at: number | null;
  last_error: string | null;
};
export type EmailReply = {
  id: string;
  task_id: string;
  body: string;
  state: EmailReplyState;
  attempts: number;
  attempted_at: number | null;
  delivered_at: number | null;
  last_error: string | null;
  targets: EmailReplyTarget[];
};

export async function fetchEmailReadiness(operatorToken: string, signal?: AbortSignal): Promise<EmailReadiness> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/email/readiness", { signal });
  return response.json() as Promise<EmailReadiness>;
}

export async function fetchEmailConfiguration(operatorToken: string): Promise<EmailOAuthConfiguration> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/email/configuration");
  return response.json() as Promise<EmailOAuthConfiguration>;
}

export async function updateEmailConfiguration(operatorToken: string, tenantId: string, clientId: string, clientSecret: string): Promise<EmailOAuthConfiguration> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/email/configuration", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ tenant_id: tenantId, client_id: clientId, client_secret: clientSecret }),
  });
  return response.json() as Promise<EmailOAuthConfiguration>;
}

export async function beginEmailAuthorization(operatorToken: string): Promise<string> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/email/auth/start", { method: "POST" });
  return ((await response.json()) as { authorization_url: string }).authorization_url;
}

export async function disconnectEmail(operatorToken: string): Promise<void> {
  await authenticatedFetch(operatorToken, "/api/v1/integrations/email/auth", { method: "DELETE" });
}

export async function fetchEmailInbox(operatorToken: string, query = ""): Promise<EmailMessageSummary[]> {
  const params = new URLSearchParams();
  if (query.trim()) params.set("query", query.trim());
  const suffix = params.size ? `?${params}` : "";
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/email/inbox${suffix}`);
  return response.json() as Promise<EmailMessageSummary[]>;
}

export async function fetchEmailMessage(operatorToken: string, messageId: string): Promise<EmailMessage> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/email/messages/${encodeURIComponent(messageId)}`);
  return response.json() as Promise<EmailMessage>;
}

export async function fetchEmailAttachmentPreview(operatorToken: string, messageId: string, attachmentId: string): Promise<Blob> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/email/messages/${encodeURIComponent(messageId)}/attachments/${encodeURIComponent(attachmentId)}`);
  return response.blob();
}

export async function importEmailMessage(operatorToken: string, messageId: string, priority: TaskPriority): Promise<EmailImport> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/email/messages/${encodeURIComponent(messageId)}/import`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ priority }),
  });
  return response.json() as Promise<EmailImport>;
}

export async function importEmailTask(operatorToken: string, input: EmailTaskImportInput): Promise<EmailImport> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/email/import", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  });
  return response.json() as Promise<EmailImport>;
}

export async function fetchEmailTaskSource(operatorToken: string, taskId: string): Promise<EmailTaskSource> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/tasks/${encodeURIComponent(taskId)}/email`);
  return response.json() as Promise<EmailTaskSource>;
}

export async function fetchEmailTaskSources(operatorToken: string): Promise<EmailTaskSource[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/email/task-links");
  return response.json() as Promise<EmailTaskSource[]>;
}

export async function fetchEmailTaskAttachment(operatorToken: string, taskId: string, storageName: string): Promise<Blob> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/tasks/${encodeURIComponent(taskId)}/email/attachments/${encodeURIComponent(storageName)}`);
  return response.blob();
}

export async function fetchTaskDeployments(operatorToken: string, taskId: string): Promise<TaskDeployment[]> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/tasks/${encodeURIComponent(taskId)}/deployments`);
  return response.json() as Promise<TaskDeployment[]>;
}

export async function recordTaskDeployment(operatorToken: string, taskId: string, environment: string, reference: string): Promise<TaskDeployment> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/tasks/${encodeURIComponent(taskId)}/deployments`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ environment, reference }),
  });
  return response.json() as Promise<TaskDeployment>;
}

export async function fetchEmailReply(operatorToken: string, taskId: string): Promise<EmailReply | null> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/tasks/${encodeURIComponent(taskId)}/email/reply`);
  return response.json() as Promise<EmailReply | null>;
}

export async function prepareEmailReply(operatorToken: string, taskId: string, body: string): Promise<EmailReply> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/tasks/${encodeURIComponent(taskId)}/email/reply`, {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ body }),
  });
  return response.json() as Promise<EmailReply>;
}

/**
 * Revises a draft under a typed instruction and returns the new text WITHOUT
 * saving it, so the version it replaces is still recoverable in the editor.
 */
export async function reviseEmailReplyDraft(operatorToken: string, taskId: string, instruction: string): Promise<string> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/tasks/${encodeURIComponent(taskId)}/email/reply/revision`, {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ instruction }),
  });
  const revised = await response.json() as { body: string };
  return revised.body;
}

export async function updateEmailReplyDraft(operatorToken: string, taskId: string, body: string): Promise<EmailReply> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/tasks/${encodeURIComponent(taskId)}/email/reply`, {
    method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ body }),
  });
  return response.json() as Promise<EmailReply>;
}

export async function sendEmailReply(operatorToken: string, replyId: string): Promise<EmailReply> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/email/replies/${encodeURIComponent(replyId)}/send`, { method: "POST" });
  return response.json() as Promise<EmailReply>;
}

export async function retryEmailReply(operatorToken: string, replyId: string): Promise<EmailReply> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/email/replies/${encodeURIComponent(replyId)}/retry`, { method: "POST" });
  return response.json() as Promise<EmailReply>;
}

/** A completed email task whose requester has not been answered. */
export type UnansweredEmailTask = {
  task_id: string;
  title: string;
  sender_name: string;
  sender_address: string;
  received_at: number;
  /** A reply exists but was never sent. Writing one is not sending one. */
  drafted: boolean;
  /** This reply is on its way — queued or dispatching, waiting on nobody. */
  sending: boolean;
  /** The drafted reply, so it can be read and sent without finding the task. */
  draft_id: string | null;
  draft_body: string | null;
  /** The worker that carried this work. */
  worker_name: string | null;
  /** How many original threads one send actually answers. */
  thread_count: number;
  /** Why the last delivery attempt failed, when one did. A cancelled reply is terminal. */
  delivery_failure?: string | null;
  /** Why no reply exists, when the board knows. See UnansweredEmailAttentionCard. */
  no_reply_reason?: string | null;
};

export async function fetchEmailTasksAwaitingReply(operatorToken: string, signal?: AbortSignal): Promise<UnansweredEmailTask[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/email/awaiting-reply", { signal });
  return response.json() as Promise<UnansweredEmailTask[]>;
}
