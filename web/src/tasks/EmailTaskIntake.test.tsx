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
  const createObjectURL = vi.fn(() => "blob:private-attached-image");
  const revokeObjectURL = vi.fn();
  vi.stubGlobal("URL", { createObjectURL, revokeObjectURL });
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
    if (url.endsWith("/messages/message-1/attachments/attachment-1")) return new Response(new Blob(["image"], { type: "image/png" }));
    if (url.endsWith("/integrations/email/import") && method === "POST") return ok({ created: true, task: { id: "task-1" }, source: { id: "source-1" }, sources: [{ id: "source-1" }] });
    throw new Error(`Unexpected request: ${method} ${url}`);
  }));
  const imported = vi.fn().mockResolvedValue(undefined);

  render(<EmailTaskIntake operatorToken="operator-token" onImported={imported} />);

  expect(await screen.findByText("Website form is broken")).toBeInTheDocument();
  expect(screen.getByText("The submit button does not work.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: "Review 1 message" }));
  expect(await screen.findByText("The submit button does not work on my phone.")).toBeInTheDocument();
  expect(screen.getByText("screenshot.png · 2 KB")).toBeInTheDocument();
  expect(await screen.findByRole("img", { name: "screenshot.png" })).toHaveAttribute("src", "blob:private-attached-image");
  expect(screen.getByText(/Every original thread and attachment stays linked/)).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Priority"), { target: { value: "high" } });
  fireEvent.click(screen.getByRole("button", { name: "Import 1 email as one task" }));

  await waitFor(() => expect(imported).toHaveBeenCalledOnce());
  const request = requests.find((item) => item.url.endsWith("/integrations/email/import"));
  expect(JSON.parse(request?.body ?? "{}")).toMatchObject({ message_ids: ["message-1"], priority: "high", worker_id: null, state: "draft" });
  expect(await screen.findByText("1 email added as one task.")).toBeInTheDocument();
});

test("loads inline images through the private preview endpoint and hides cid markers", async () => {
  const createObjectURL = vi.fn(() => "blob:private-inline-image");
  const revokeObjectURL = vi.fn();
  vi.stubGlobal("URL", { createObjectURL, revokeObjectURL });
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/readiness")) return ok({ configured: true, connection: "ready", account_name: "Bea", account_address: "bea@example.com" });
    if (url.endsWith("/inbox")) return ok([{
      id: "message-inline", conversation_id: "thread-inline", internet_message_id: null, subject: "Screenshot of the problem",
      sender_name: "Alex", sender_address: "alex@example.com", received_at: 1_786_000_000, web_url: "https://outlook.test/message-inline",
      has_attachments: true, preview: "Here is the screenshot.",
    }]);
    if (url.endsWith("/messages/message-inline")) return ok({
      summary: {
        id: "message-inline", conversation_id: "thread-inline", internet_message_id: null, subject: "Screenshot of the problem",
        sender_name: "Alex", sender_address: "alex@example.com", received_at: 1_786_000_000, web_url: "https://outlook.test/message-inline",
        has_attachments: true, preview: "Here is the screenshot.",
      },
      body_text: "Here is the screenshot.\n\n[cid:image-1]",
      attachments: [{ id: "inline-1", name: "screen.png", media_type: "image/png", byte_size: 2048, inline: true, content_id: "image-1" }],
    });
    if (url.endsWith("/messages/message-inline/attachments/inline-1")) return new Response(new Blob(["image"], { type: "image/png" }));
    throw new Error(`Unexpected request: ${url}`);
  }));

  render(<EmailTaskIntake operatorToken="operator-token" onImported={vi.fn()} />);
  await screen.findByRole("listitem");
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: "Review 1 message" }));

  const image = await screen.findByRole("img", { name: "screen.png" });
  expect(image).toHaveAttribute("src", "blob:private-inline-image");
  expect(screen.queryByText("[cid:image-1]")).not.toBeInTheDocument();
  expect(createObjectURL).toHaveBeenCalledOnce();
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
