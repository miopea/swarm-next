import { authenticatedFetch } from "./request";
import type { Task, TaskState } from "./tasks";

export type JiraConnectionState =
  | "not_connected"
  | "ready"
  | "network_unavailable"
  | "credentials_invalid"
  | "permission_denied";
export type JiraReadiness = { configured: boolean; connection: JiraConnectionState; account_name: string | null };
export type JiraProject = { id: string; key: string; name: string };
export type JiraProjectStatus = { id: string; name: string; category_key: string; recommended_task_state: TaskState };
export type JiraProjectBinding = {
  id: string;
  project_id: string;
  project_key: string;
  project_name: string;
  scope: "hive" | "apiary";
  hive_id: string;
  apiary_id: string | null;
  access_verified: boolean;
  workflow_mapped: boolean;
  auto_sync_assigned: boolean;
};
export type JiraStatusMapping = { jira_status_id: string; jira_status_name: string; task_state: TaskState };
export type JiraIssue = {
  id: string;
  key: string;
  summary: string;
  description: string;
  status_id: string;
  status_name: string;
  assignee_account_id: string | null;
  assignee_name: string | null;
  updated_at: string;
};
export type JiraTaskLink = {
  issue_id: string;
  issue_key: string;
  issue_url: string | null;
  binding_id: string;
  project_key: string;
  project_name: string;
  task_id: string;
  jira_status_id: string;
  jira_status_name: string;
  jira_assignee_account_id: string | null;
  jira_assignee_name: string | null;
  remote_updated_at: string;
  last_synced_at: number;
  outbound_state: "queued" | "dispatching" | "conflict" | "uncertain" | null;
};
export type JiraComment = { id: string; author_name: string; body: string; created_at: string; updated_at: string };
export type JiraCommentDispatch = { state: "queued" | "dispatching" | "delivered" | "conflict" | "uncertain" };
export type JiraTaskAttachment = { id: string; filename: string; media_type: string; byte_size: number; is_image: boolean };
export type JiraTaskDetail = { summary: string; description: string; attachments: JiraTaskAttachment[] };

export async function fetchJiraTaskDetail(operatorToken: string, taskId: string): Promise<JiraTaskDetail> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/jira/task-links/${encodeURIComponent(taskId)}/detail`);
  return response.json() as Promise<JiraTaskDetail>;
}

export async function fetchJiraTaskAttachment(operatorToken: string, taskId: string, attachmentId: string): Promise<Blob> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/jira/task-links/${encodeURIComponent(taskId)}/attachments/${encodeURIComponent(attachmentId)}`);
  return response.blob();
}

export async function fetchJiraComments(operatorToken: string, taskId: string): Promise<JiraComment[]> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/jira/task-links/${encodeURIComponent(taskId)}/comments`);
  return response.json() as Promise<JiraComment[]>;
}

export async function addJiraComment(operatorToken: string, taskId: string, body: string): Promise<JiraCommentDispatch> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/jira/task-links/${encodeURIComponent(taskId)}/comments`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ body }),
  });
  return response.json() as Promise<JiraCommentDispatch>;
}

export async function fetchJiraReadiness(operatorToken: string): Promise<JiraReadiness> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/jira/readiness");
  return response.json() as Promise<JiraReadiness>;
}

export async function beginJiraAuthorization(operatorToken: string): Promise<string> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/jira/auth/start", { method: "POST" });
  return ((await response.json()) as { authorization_url: string }).authorization_url;
}

export async function disconnectJira(operatorToken: string): Promise<void> {
  await authenticatedFetch(operatorToken, "/api/v1/integrations/jira/auth", { method: "DELETE" });
}

export async function fetchJiraProjects(operatorToken: string, query = ""): Promise<JiraProject[]> {
  const params = new URLSearchParams();
  if (query.trim()) params.set("query", query.trim());
  const suffix = params.size ? `?${params}` : "";
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/jira/projects${suffix}`);
  return response.json() as Promise<JiraProject[]>;
}

export async function fetchJiraProjectStatuses(operatorToken: string, projectIdOrKey: string): Promise<JiraProjectStatus[]> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/jira/projects/${encodeURIComponent(projectIdOrKey)}/statuses`);
  return response.json() as Promise<JiraProjectStatus[]>;
}

export async function fetchJiraBindings(operatorToken: string): Promise<JiraProjectBinding[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/jira/bindings");
  return response.json() as Promise<JiraProjectBinding[]>;
}

export async function fetchJiraTaskLinks(operatorToken: string): Promise<JiraTaskLink[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/jira/task-links");
  const links = await response.json() as unknown;
  return Array.isArray(links) ? links as JiraTaskLink[] : [];
}

export async function retryJiraTaskLink(operatorToken: string, taskId: string): Promise<void> {
  await authenticatedFetch(operatorToken, `/api/v1/integrations/jira/task-links/${encodeURIComponent(taskId)}/retry`, { method: "POST" });
}

export async function createJiraBinding(operatorToken: string, project: JiraProject): Promise<JiraProjectBinding> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/jira/bindings", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ project_id: project.id, project_key: project.key, project_name: project.name }),
  });
  return response.json() as Promise<JiraProjectBinding>;
}

export async function replaceJiraMappings(operatorToken: string, bindingId: string, mappings: JiraStatusMapping[]): Promise<JiraStatusMapping[]> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/jira/bindings/${encodeURIComponent(bindingId)}/mappings`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ mappings }),
  });
  return response.json() as Promise<JiraStatusMapping[]>;
}

export async function fetchJiraMappings(operatorToken: string, bindingId: string): Promise<JiraStatusMapping[]> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/jira/bindings/${encodeURIComponent(bindingId)}/mappings`);
  return response.json() as Promise<JiraStatusMapping[]>;
}

export async function setJiraAssignedSync(operatorToken: string, bindingId: string, enabled: boolean): Promise<JiraProjectBinding> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/jira/bindings/${encodeURIComponent(bindingId)}/assigned-sync`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ enabled }),
  });
  return response.json() as Promise<JiraProjectBinding>;
}

export async function fetchJiraBindingIssues(operatorToken: string, bindingId: string): Promise<JiraIssue[]> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/jira/bindings/${encodeURIComponent(bindingId)}/issues`);
  return response.json() as Promise<JiraIssue[]>;
}

export async function syncJiraBinding(operatorToken: string, bindingId: string, issueIds: string[]): Promise<Task[]> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/jira/bindings/${encodeURIComponent(bindingId)}/sync`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ issue_ids: issueIds }),
  });
  return response.json() as Promise<Task[]>;
}

export async function reconcileJira(operatorToken: string): Promise<void> {
  await authenticatedFetch(operatorToken, "/api/v1/integrations/jira/reconcile", { method: "POST" });
}
