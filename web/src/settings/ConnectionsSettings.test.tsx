import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import ConnectionsSettings from "./ConnectionsSettings";

afterEach(cleanup);

function ok(body: unknown) {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}

const tool = {
  id: "01a04a00-0000-7000-8000-000000000001",
  name: "Claude Desktop",
  connected_at: 1_787_900_000,
  last_seen_at: 1_787_950_000,
};

test("lists a connected tool and disconnects it after confirming", async () => {
  const requests: { url: string; method: string }[] = [];
  let listed = [tool];
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    const method = init?.method ?? "GET";
    requests.push({ url, method });
    if (method === "DELETE") { listed = []; return new Response(null, { status: 204 }); }
    if (url.endsWith("/api/v1/connections")) return ok({ connections: listed });
    throw new Error(`Unexpected request: ${method} ${url}`);
  }));

  render(<ConnectionsSettings operatorToken="operator-token" />);
  expect(await screen.findByText("Claude Desktop")).toBeInTheDocument();

  // Disconnecting is not one click: it cannot be undone.
  fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));
  expect(screen.getByText(/cannot reconnect without registering/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Disconnect it" }));

  await waitFor(() => expect(screen.queryByText("Claude Desktop")).not.toBeInTheDocument());
  expect(requests.some((request) => request.method === "DELETE"
    && request.url.endsWith(`/api/v1/connections/${tool.id}`))).toBe(true);
});

test("keeping it makes no request", async () => {
  const requests: { method: string }[] = [];
  vi.stubGlobal("fetch", vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
    requests.push({ method: init?.method ?? "GET" });
    return ok({ connections: [tool] });
  }));

  render(<ConnectionsSettings operatorToken="operator-token" />);
  fireEvent.click(await screen.findByRole("button", { name: "Disconnect" }));
  fireEvent.click(screen.getByRole("button", { name: "Keep it" }));

  expect(screen.getByText("Claude Desktop")).toBeInTheDocument();
  expect(requests.every((request) => request.method === "GET")).toBe(true);
});

/// A read that failed must not render as "nothing is connected". That reads as
/// a safe answer and is not one — this fleet has shipped that defect before.
test("a failed read says so rather than showing an empty list", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => { throw new Error("offline"); }));

  render(<ConnectionsSettings operatorToken="operator-token" />);
  expect(await screen.findByText(/could not be read/)).toBeInTheDocument();
  expect(screen.queryByText(/Nothing is connected/)).not.toBeInTheDocument();
});

test("an empty list says so plainly, and says how to connect one", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => ok({ connections: [] })));

  render(<ConnectionsSettings operatorToken="operator-token" />);
  expect(await screen.findByText(/Nothing is connected/)).toBeInTheDocument();
  expect(screen.queryByText(/could not be read/)).not.toBeInTheDocument();
});
