import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { EmailTaskSource, Task } from "../api";
import EmailResolutionPanel from "./EmailResolutionPanel";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const task: Task = {
  id: "task-1", hive_id: "hive-1", title: "Fix the reported form", workspace: "email://inbox", state: "completed",
  description: "The phone field does not save.", priority: "normal", assigned_worker_id: "worker-1", assigned_session_id: null,
  position: 0, created_at: 1, updated_at: 1,
};

const source: EmailTaskSource = {
  task_id: "task-1", integration_id: "operator-outlook", message_id: "message-1", conversation_id: "thread-1",
  internet_message_id: "<one@example.com>", sender_name: "Alex", sender_address: "alex@example.com", received_at: 1,
  web_url: "https://outlook.test/message-1", imported_at: 2,
  attachments: [{ storage_name: "sha256-screen.png", display_name: "screen.png", media_type: "image/png", byte_size: 2048, inline: false, content_id: null }],
};

test("requires deployment evidence and a second explicit confirmation before replying", async () => {
  const requests: { url: string; method: string; body?: string }[] = [];
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    const method = init?.method ?? "GET";
    requests.push({ url, method, body: typeof init?.body === "string" ? init.body : undefined });
    if (url.endsWith("/deployments") && method === "GET") return ok([]);
    if (url.endsWith("/email/reply") && method === "GET") return ok(null);
    if (url.endsWith("/deployments") && method === "POST") return ok({ id: "deployment-1", task_id: task.id, environment: "production", reference: "release-42", deployed_at: 3, recorded_at: 3 });
    if (url.endsWith("/email/reply") && method === "POST") return ok({ id: "reply-1", task_id: task.id, body: "Thank you. The form now saves correctly.", state: "draft", attempts: 0, attempted_at: null, delivered_at: null, last_error: null });
    if (url.endsWith("/replies/reply-1/send") && method === "POST") return ok({ id: "reply-1", task_id: task.id, body: "Thank you. The form now saves correctly.", state: "delivered", attempts: 1, attempted_at: 4, delivered_at: 4, last_error: null });
    throw new Error(`Unexpected request: ${method} ${url}`);
  }));

  render(<EmailResolutionPanel operatorToken="operator-token" task={task} source={source} />);

  expect(await screen.findByText("Confirm the fix is live")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Send reply now" })).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Release, URL, or deployment reference"), { target: { value: "release-42" } });
  fireEvent.click(screen.getByRole("button", { name: "Record deployment" }));
  expect(await screen.findByLabelText(/Plain-language resolution/)).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText(/Plain-language resolution/), { target: { value: "Thank you. The form now saves correctly." } });
  fireEvent.click(screen.getByRole("button", { name: "Save reply for review" }));
  expect(await screen.findByText("Reply saved for review. Nothing has been sent.")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Send reply now" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Review and send" }));
  expect(screen.getByRole("group", { name: "Confirm email reply" })).toHaveTextContent("alex@example.com");
  fireEvent.click(screen.getByRole("button", { name: "Send reply now" }));
  expect(await screen.findByText("Reply delivered")).toBeInTheDocument();

  expect(JSON.parse(requests.find((request) => request.url.endsWith("/deployments") && request.method === "POST")?.body ?? "{}")).toMatchObject({ environment: "production", reference: "release-42" });
  expect(JSON.parse(requests.find((request) => request.url.endsWith("/email/reply") && request.method === "POST")?.body ?? "{}")).toEqual({ body: "Thank you. The form now saves correctly." });
});

test("never silently retries an uncertain Outlook result", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/deployments")) return ok([{ id: "deployment-1", task_id: task.id, environment: "production", reference: "release-42", deployed_at: 3, recorded_at: 3 }]);
    if (url.endsWith("/email/reply")) return ok({ id: "reply-1", task_id: task.id, body: "The issue is fixed.", state: "uncertain", attempts: 1, attempted_at: 4, delivered_at: null, last_error: "connection ended" });
    throw new Error(`Unexpected request: ${url}`);
  }));

  render(<EmailResolutionPanel operatorToken="operator-token" task={task} source={source} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("will not retry automatically");
  expect(screen.getByRole("button", { name: "I checked Outlook · retry reply" })).toBeInTheDocument();
});

function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}
