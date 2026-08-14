import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import EmailTaskIntake from "./EmailTaskIntake";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("previews an Inbox message and explicitly imports its body and attachments as a task", async () => {
  const requests: { url: string; method: string; body?: string }[] = [];
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    const method = init?.method ?? "GET";
    requests.push({ url, method, body: typeof init?.body === "string" ? init.body : undefined });
    if (url.endsWith("/readiness")) return ok({ configured: true, connection: "ready", account_name: "Bea", account_address: "bea@example.com" });
    if (url.endsWith("/inbox")) return ok([{
      id: "message-1", conversation_id: "thread-1", internet_message_id: "<one@example.com>", subject: "Website form is broken",
      sender_name: "Alex", sender_address: "alex@example.com", received_at: 1_786_000_000, web_url: "https://outlook.test/message-1",
      has_attachments: true, preview: "The submit button does not work.",
    }]);
    if (url.endsWith("/messages/message-1") && method === "GET") return ok({
      summary: {
        id: "message-1", conversation_id: "thread-1", internet_message_id: "<one@example.com>", subject: "Website form is broken",
        sender_name: "Alex", sender_address: "alex@example.com", received_at: 1_786_000_000, web_url: "https://outlook.test/message-1",
        has_attachments: true, preview: "The submit button does not work.",
      },
      body_text: "The submit button does not work on my phone.",
      attachments: [{ id: "attachment-1", name: "screenshot.png", media_type: "image/png", byte_size: 2048, inline: false, content_id: null }],
    });
    if (url.endsWith("/messages/message-1/import") && method === "POST") return ok({ created: true, task_id: "task-1" });
    throw new Error(`Unexpected request: ${method} ${url}`);
  }));
  const imported = vi.fn().mockResolvedValue(undefined);

  render(<EmailTaskIntake operatorToken="operator-token" onImported={imported} />);

  expect(await screen.findByText("Website form is broken")).toBeInTheDocument();
  expect(screen.getByText("The submit button does not work.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("listitem"));
  expect(await screen.findByText("The submit button does not work on my phone.")).toBeInTheDocument();
  expect(screen.getByText("screenshot.png · 2 KB")).toBeInTheDocument();
  expect(screen.getByText(/original Outlook thread remains linked/)).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Task priority"), { target: { value: "high" } });
  fireEvent.click(screen.getByRole("button", { name: "Import as task" }));

  await waitFor(() => expect(imported).toHaveBeenCalledOnce());
  const request = requests.find((item) => item.url.endsWith("/messages/message-1/import"));
  expect(JSON.parse(request?.body ?? "{}")).toEqual({ priority: "high" });
  expect(await screen.findByText("Email added as a draft task.")).toBeInTheDocument();
});

test("directs the operator to integrations when Outlook is not connected", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => ok({ configured: true, connection: "not_connected", account_name: null, account_address: null })));
  render(<EmailTaskIntake operatorToken="operator-token" onImported={vi.fn()} />);
  expect(await screen.findByText("Connect Outlook first")).toBeInTheDocument();
  expect(screen.getByText(/Settings → Integrations/)).toBeInTheDocument();
});

function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}
