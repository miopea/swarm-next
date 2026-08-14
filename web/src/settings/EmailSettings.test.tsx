import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import EmailSettings from "./EmailSettings";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("opens delegated Microsoft authorization and explains the reviewed reply guardrail", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/auth/start") && init?.method === "POST") {
      return ok({ authorization_url: "https://login.microsoftonline.test/authorize" });
    }
    throw new Error(`Unexpected request: ${init?.method ?? "GET"} ${url}`);
  }));
  const navigate = vi.fn();

  render(
    <EmailSettings
      operatorToken="operator-token"
      readiness={{ configured: true, connection: "not_connected", account_name: null, account_address: null }}
      unavailable={false}
      onNavigate={navigate}
    />,
  );

  expect(screen.getByText(/Completing a task does not send mail/)).toBeInTheDocument();
  expect(screen.getByText(/tokens remain private on this host/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Connect Outlook" }));
  await waitFor(() => expect(navigate).toHaveBeenCalledWith("https://login.microsoftonline.test/authorize"));
});

test("shows the connected account without exposing implementation settings", () => {
  render(
    <EmailSettings
      operatorToken="operator-token"
      readiness={{ configured: true, connection: "ready", account_name: "Bea", account_address: "bea@example.com" }}
      unavailable={false}
    />,
  );

  expect(screen.getByText("Connected as bea@example.com")).toBeInTheDocument();
  expect(screen.getByText(/Inbox access uses Bea's delegated identity/)).toBeInTheDocument();
  expect(screen.queryByText(/client secret|tenant id/i)).not.toBeInTheDocument();
});

function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}
