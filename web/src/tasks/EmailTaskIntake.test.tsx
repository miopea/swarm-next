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
  const scrollIntoView = vi.fn();
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: scrollIntoView });
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
  await waitFor(() => expect(scrollIntoView).toHaveBeenCalledWith({ block: "start", behavior: "auto" }));
  expect(screen.getByText("screenshot.png · 2 KB")).toBeInTheDocument();
  expect(await screen.findByRole("img", { name: "screenshot.png" })).toHaveAttribute("src", "blob:private-attached-image");
  expect(screen.getByText(/source email and every attachment will stay linked/)).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Priority"), { target: { value: "high" } });
  fireEvent.click(screen.getByRole("button", { name: "Create task" }));

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

test("opens multiple messages sequentially and retries a transient gateway response", async () => {
  let activeDetails = 0;
  let highestConcurrency = 0;
  let firstMessageAttempts = 0;
  const summaries = ["message-1", "message-2"].map((id, index) => ({
    id,
    conversation_id: `thread-${index + 1}`,
    internet_message_id: null,
    subject: `Related report ${index + 1}`,
    sender_name: "Alex",
    sender_address: "alex@example.com",
    received_at: 1_786_000_000 + index,
    web_url: `https://outlook.test/${id}`,
    has_attachments: false,
    preview: `Preview ${index + 1}`,
  }));
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/readiness")) return ok({ configured: true, connection: "ready", account_name: "Bea", account_address: "bea@example.com" });
    if (url.endsWith("/inbox")) return ok(summaries);
    const summary = summaries.find((item) => url.endsWith(`/messages/${item.id}`));
    if (!summary) throw new Error(`Unexpected request: ${url}`);
    activeDetails += 1;
    highestConcurrency = Math.max(highestConcurrency, activeDetails);
    try {
      if (summary.id === "message-1" && firstMessageAttempts++ === 0) {
        return new Response("temporary gateway failure", { status: 502 });
      }
      return ok({ summary, body_text: `Full report ${summary.id}`, attachments: [] });
    } finally {
      activeDetails -= 1;
    }
  }));

  render(<EmailTaskIntake operatorToken="operator-token" onImported={vi.fn()} />);
  const checkboxes = await screen.findAllByRole("checkbox");
  fireEvent.click(checkboxes[0]);
  fireEvent.click(checkboxes[1]);
  fireEvent.click(screen.getByRole("button", { name: "Review 2 messages" }));

  expect(await screen.findByDisplayValue(/Full report message-2/)).toBeInTheDocument();
  expect(screen.getByText("2 source threads")).toBeInTheDocument();
  expect(firstMessageAttempts).toBe(2);
  expect(highestConcurrency).toBe(1);
});

test("directs the operator to integrations when Outlook is not connected", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => ok({ configured: true, connection: "not_connected", account_name: null, account_address: null })));
  render(<EmailTaskIntake operatorToken="operator-token" onImported={vi.fn()} />);
  expect(await screen.findByText("Connect Outlook first")).toBeInTheDocument();
  expect(screen.getByText(/Settings → Integrations/)).toBeInTheDocument();
});

test("distinguishes a temporary Outlook failure from a missing connection", async () => {
  let attempts = 0;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (!url.endsWith("/readiness")) throw new Error(`Unexpected request: ${url}`);
    if (attempts++ === 0) return new Response("temporary gateway failure", { status: 502 });
    return ok({ configured: true, connection: "not_connected", account_name: null, account_address: null });
  }));

  render(<EmailTaskIntake operatorToken="operator-token" onImported={vi.fn()} />);

  expect(await screen.findByRole("heading", { name: "Outlook could not be loaded" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByRole("heading", { name: "Connect Outlook first" })).toBeInTheDocument();
});

function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}

test("a long email subject produces a title the server will accept", async () => {
  // THE REPORTED BUG, 01a049b4: "Runtime request returned 400: task title must
  // contain 1 to 240 bytes. Even when I edit the this error appears."
  //
  // The title is set from the subject with setTitle(). `maxLength` constrains
  // TYPING and does nothing to a programmatic value, so the auto-filled title
  // was never bounded — and the field's limit, which counts UTF-16 units,
  // could not have matched the server's UTF-8 byte count anyway. The curly
  // apostrophes and em dashes below are three bytes each, which is exactly
  // what a mail client puts in a subject line.
  const subject = `${"The worker’s state isn’t updating — thread ".repeat(8)}end`;
  expect(new TextEncoder().encode(subject).length).toBeGreaterThan(240);

  const requests: { url: string; method: string; body?: string }[] = [];
  const summary = {
    id: "message-long", conversation_id: "thread-long", internet_message_id: "<long@example.com>", subject,
    sender_name: "Alex", sender_address: "alex@example.com", received_at: 1_786_000_000,
    web_url: "https://outlook.test/message-long", has_attachments: false, preview: "A long one.",
  };
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    const method = init?.method ?? "GET";
    requests.push({ url, method, body: typeof init?.body === "string" ? init.body : undefined });
    if (url.endsWith("/readiness")) return ok({ configured: true, connection: "ready", account_name: "Bea", account_address: "bea@example.com" });
    if (url.endsWith("/inbox")) return ok([summary]);
    if (url.endsWith("/messages/message-long") && method === "GET") return ok({ summary, body_text: "A long one.", attachments: [] });
    if (url.endsWith("/integrations/email/import") && method === "POST") return ok({ created: true, task: { id: "task-1" }, source: { id: "source-1" }, sources: [{ id: "source-1" }] });
    throw new Error(`Unexpected request: ${method} ${url}`);
  }));
  const imported = vi.fn().mockResolvedValue(undefined);

  render(<EmailTaskIntake operatorToken="operator-token" onImported={imported} />);
  fireEvent.click(await screen.findByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: "Review 1 message" }));

  const field = await screen.findByLabelText("Task title");
  // Fits the server's limit, and says it was shortened rather than silently
  // dropping the end of the subject.
  const value = (field as HTMLInputElement).value;
  expect(new TextEncoder().encode(value).length).toBeLessThanOrEqual(240);
  expect(value.endsWith("…")).toBe(true);

  fireEvent.click(screen.getByRole("button", { name: "Create task" }));
  await waitFor(() => expect(imported).toHaveBeenCalledOnce());
  const request = requests.find((item) => item.url.endsWith("/integrations/email/import"));
  const sent = JSON.parse(request?.body ?? "{}") as { title: string };
  expect(new TextEncoder().encode(sent.title).length).toBeLessThanOrEqual(240);
});

test("a title edited past the limit is refused in the field, not by a 400", async () => {
  // The second half of the report — "even when I edit". An over-long title now
  // says how much has to go and blocks the request, instead of letting the
  // operator press Create and read a sentence about UTF-8 bytes.
  const summary = {
    id: "message-1", conversation_id: "thread-1", internet_message_id: "<one@example.com>", subject: "Short subject",
    sender_name: "Alex", sender_address: "alex@example.com", received_at: 1_786_000_000,
    web_url: "https://outlook.test/message-1", has_attachments: false, preview: "Short.",
  };
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    const method = init?.method ?? "GET";
    if (url.endsWith("/readiness")) return ok({ configured: true, connection: "ready", account_name: "Bea", account_address: "bea@example.com" });
    if (url.endsWith("/inbox")) return ok([summary]);
    if (url.endsWith("/messages/message-1") && method === "GET") return ok({ summary, body_text: "Short.", attachments: [] });
    throw new Error(`Unexpected request: ${method} ${url}`);
  }));

  render(<EmailTaskIntake operatorToken="operator-token" onImported={vi.fn()} />);
  fireEvent.click(await screen.findByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: "Review 1 message" }));

  const field = await screen.findByLabelText("Task title");
  fireEvent.change(field, { target: { value: "…".repeat(100) } });

  expect(screen.getByRole("button", { name: "Create task" })).toBeDisabled();
  expect(field).toHaveAttribute("aria-invalid", "true");
  expect(screen.getByText(/too long/)).toBeInTheDocument();

  // And the way out is one press, rather than counting bytes by hand.
  fireEvent.click(screen.getByRole("button", { name: "Shorten it for me" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Create task" })).not.toBeDisabled());
});
