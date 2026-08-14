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
    if (url.endsWith("/configuration") && !init?.method) {
      return ok({ configured: true, managed_by: "operator", tenant_id: "organizations", client_id: "client-id", callback_url: "https://swarm.test/auth/email/callback", secret_stored: true });
    }
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
  vi.stubGlobal("fetch", vi.fn(async () => ok({ configured: true, managed_by: "operator", tenant_id: "organizations", client_id: "client-id", callback_url: "https://swarm.test/auth/email/callback", secret_stored: true })));
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

test("configures the host registration without returning its client secret", async () => {
  const fetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/configuration") && !init?.method) {
      return ok({ configured: false, managed_by: null, tenant_id: null, client_id: null, callback_url: "https://swarm.test/auth/email/callback", secret_stored: false });
    }
    if (url.endsWith("/configuration") && init?.method === "PUT") {
      expect(JSON.parse(String(init.body))).toEqual({
        tenant_id: "organizations",
        client_id: "11112222-bbbb-3333-cccc-4444dddd5555",
        client_secret: "private-value",
      });
      return ok({ configured: true, managed_by: "operator", tenant_id: "organizations", client_id: "11112222-bbbb-3333-cccc-4444dddd5555", callback_url: "https://swarm.test/auth/email/callback", secret_stored: true });
    }
    throw new Error(`Unexpected request: ${init?.method ?? "GET"} ${url}`);
  });
  vi.stubGlobal("fetch", fetch);

  render(
    <EmailSettings
      operatorToken="operator-token"
      readiness={{ configured: false, connection: "not_connected", account_name: null, account_address: null }}
      unavailable={false}
    />,
  );

  expect(await screen.findByRole("form", { name: "Microsoft app setup" })).toHaveTextContent("User.Read, Mail.Read, Mail.Send");
  expect(screen.getByDisplayValue("https://swarm.test/auth/email/callback")).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Directory (tenant) ID"), { target: { value: "organizations" } });
  fireEvent.change(screen.getByLabelText("Application (client) ID"), { target: { value: "11112222-bbbb-3333-cccc-4444dddd5555" } });
  fireEvent.change(screen.getByLabelText("Client secret value"), { target: { value: "private-value" } });
  fireEvent.click(screen.getByRole("button", { name: "Save app registration" }));

  expect(await screen.findByText(/registration saved privately/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Connect Outlook" })).toBeEnabled();
  expect(screen.queryByDisplayValue("private-value")).not.toBeInTheDocument();
});

function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}
