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

test("registers a public client from one field, sending no secret at all", async () => {
  let sent: unknown;
  const fetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/configuration") && !init?.method) {
      return ok({ configured: false, managed_by: null, tenant_id: null, client_id: null, callback_url: "https://swarm.test/auth/email/callback", secret_stored: false });
    }
    if (url.endsWith("/configuration") && init?.method === "PUT") {
      sent = JSON.parse(String(init.body));
      return ok({ configured: true, managed_by: "operator", tenant_id: "consumers", client_id: "11112222-bbbb-3333-cccc-4444dddd5555", callback_url: "https://swarm.test/auth/email/callback", secret_stored: false });
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

  const form = await screen.findByRole("form", { name: "Microsoft app setup" });
  expect(form).toHaveTextContent("User.Read, Mail.Read, Mail.Send");
  expect(screen.getByDisplayValue("https://swarm.test/auth/email/callback")).toBeInTheDocument();

  // ONE required field. The tenant and the secret are behind Advanced, so a
  // person setting this up is not asked for either.
  expect(screen.queryByLabelText("Directory (tenant) ID")).not.toBeInTheDocument();
  expect(screen.queryByLabelText(/Client secret/)).not.toBeInTheDocument();

  fireEvent.change(screen.getByLabelText("Application (client) ID"), { target: { value: "11112222-bbbb-3333-cccc-4444dddd5555" } });
  fireEvent.click(screen.getByRole("button", { name: "Save app registration" }));

  expect(await screen.findByText(/registration saved privately/)).toBeInTheDocument();
  // NOT `client_secret: ""`. Microsoft refuses an empty secret as an invalid
  // client rather than reading it as absent, so the key must not be there.
  expect(sent).toEqual({ tenant_id: "consumers", client_id: "11112222-bbbb-3333-cccc-4444dddd5555" });
  expect(screen.getByRole("button", { name: "Connect Outlook" })).toBeEnabled();
});

test("defaults to a personal account and switches the authority for work or school", async () => {
  let sent: unknown;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/configuration") && !init?.method) {
      return ok({ configured: false, managed_by: null, tenant_id: null, client_id: null, callback_url: "https://swarm.test/auth/email/callback", secret_stored: false });
    }
    sent = JSON.parse(String(init?.body));
    return ok({ configured: true, managed_by: "operator", tenant_id: "organizations", client_id: "client-id", callback_url: "https://swarm.test/auth/email/callback", secret_stored: false });
  }));

  render(
    <EmailSettings
      operatorToken="operator-token"
      readiness={{ configured: false, connection: "not_connected", account_name: null, account_address: null }}
      unavailable={false}
    />,
  );

  // The default has to be `consumers`, not `organizations`. An organizations
  // authority refuses every outlook.com address with a sign-in page that does
  // not say why, which is the failure this selector exists to prevent.
  const personal = await screen.findByRole("radio", { name: /Personal Microsoft account/ });
  expect(personal).toBeChecked();

  fireEvent.click(screen.getByRole("radio", { name: /Work or school/ }));
  fireEvent.change(screen.getByLabelText("Application (client) ID"), { target: { value: "client-id" } });
  fireEvent.click(screen.getByRole("button", { name: "Save app registration" }));

  await waitFor(() => expect(sent).toEqual({ tenant_id: "organizations", client_id: "client-id" }));
});

test("keeps a Hive that already has a confidential registration working", async () => {
  let sent: unknown;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/configuration") && !init?.method) {
      return ok({ configured: false, managed_by: null, tenant_id: null, client_id: null, callback_url: "https://swarm.test/auth/email/callback", secret_stored: false });
    }
    sent = JSON.parse(String(init?.body));
    return ok({ configured: true, managed_by: "operator", tenant_id: "organizations", client_id: "client-id", callback_url: "https://swarm.test/auth/email/callback", secret_stored: true });
  }));

  render(
    <EmailSettings
      operatorToken="operator-token"
      readiness={{ configured: false, connection: "not_connected", account_name: null, account_address: null }}
      unavailable={false}
    />,
  );

  fireEvent.click(await screen.findByRole("button", { name: "Advanced settings" }));
  fireEvent.change(screen.getByLabelText("Directory (tenant) ID"), { target: { value: "organizations" } });
  fireEvent.change(screen.getByLabelText("Application (client) ID"), { target: { value: "client-id" } });
  fireEvent.change(screen.getByLabelText(/Client secret/), { target: { value: "private-value" } });
  fireEvent.click(screen.getByRole("button", { name: "Save app registration" }));

  await waitFor(() => expect(sent).toEqual({ tenant_id: "organizations", client_id: "client-id", client_secret: "private-value" }));
  expect(screen.queryByDisplayValue("private-value")).not.toBeInTheDocument();
});

test("a fresh Hive is already registered, so setup is one consent click", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/configuration") && !init?.method) {
      // `common` and the bundled application: what a Hive reports before
      // anyone has touched Entra.
      return ok({ configured: true, managed_by: "bundled", tenant_id: "common", client_id: "e7c58c91-ef37-44e8-ac20-b8df5feb2618", callback_url: "https://swarm.test/auth/email/callback", secret_stored: false });
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

  // NOT A FIELD IN SIGHT. This is the whole point: no tenant, no client id, no
  // secret, and no question about which kind of account it is -- `common`
  // routes personal and work alike.
  expect(await screen.findByRole("button", { name: "Connect Outlook" })).toBeEnabled();
  expect(screen.queryByRole("form", { name: "Microsoft app setup" })).not.toBeInTheDocument();
  expect(screen.queryByLabelText("Application (client) ID")).not.toBeInTheDocument();
  expect(screen.queryByRole("radio")).not.toBeInTheDocument();
  expect(screen.getByText(/Personal and work accounts both work/)).toBeInTheDocument();
  // But the callback URI stays on screen. Microsoft matches redirect URIs
  // exactly, so a published Hive is refused until this exact string is on the
  // shared registration -- and hiding the setup form hid the only place it
  // was legible.
  expect(screen.getByDisplayValue("https://swarm.test/auth/email/callback")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Connect Outlook" }));
  await waitFor(() => expect(navigate).toHaveBeenCalledWith("https://login.microsoftonline.test/authorize"));
});

test("an organisation that must own its own consent screen still can", async () => {
  let sent: unknown;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/configuration") && !init?.method) {
      return ok({ configured: true, managed_by: "bundled", tenant_id: "common", client_id: "e7c58c91-ef37-44e8-ac20-b8df5feb2618", callback_url: "https://swarm.test/auth/email/callback", secret_stored: false });
    }
    sent = JSON.parse(String(init?.body));
    return ok({ configured: true, managed_by: "operator", tenant_id: "organizations", client_id: "their-app", callback_url: "https://swarm.test/auth/email/callback", secret_stored: false });
  }));

  render(
    <EmailSettings
      operatorToken="operator-token"
      readiness={{ configured: true, connection: "not_connected", account_name: null, account_address: null }}
      unavailable={false}
    />,
  );

  fireEvent.click(await screen.findByRole("button", { name: "Use your own Microsoft app" }));
  fireEvent.click(screen.getByRole("radio", { name: /Work or school/ }));
  fireEvent.change(screen.getByLabelText("Application (client) ID"), { target: { value: "their-app" } });
  fireEvent.click(screen.getByRole("button", { name: "Save app registration" }));

  await waitFor(() => expect(sent).toEqual({ tenant_id: "organizations", client_id: "their-app" }));
});

test("offers a direct retry when Outlook readiness is temporarily unavailable", () => {
  vi.stubGlobal("fetch", vi.fn(async () => ok({ configured: true, managed_by: "operator", tenant_id: "organizations", client_id: "client-id", callback_url: "https://swarm.test/auth/email/callback", secret_stored: true })));
  const onRetryReadiness = vi.fn();
  render(
    <EmailSettings
      operatorToken="operator-token"
      readiness={undefined}
      unavailable
      onRetryReadiness={onRetryReadiness}
    />,
  );

  fireEvent.click(screen.getByRole("button", { name: "Retry Outlook status" }));
  expect(onRetryReadiness).toHaveBeenCalledOnce();
  expect(screen.queryByRole("form", { name: "Microsoft app setup" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Connect Outlook" })).not.toBeInTheDocument();
});

function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}
