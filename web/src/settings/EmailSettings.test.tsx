import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import EmailSettings from "./EmailSettings";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

// What every Hive reports now. There is no other shape: Swarm ships the
// registration, so `managed_by` is "bundled" unless the HOST pinned one through
// SWARM_EMAIL_*, which no browser can reach.
const bundled = {
  configured: true,
  managed_by: "bundled",
  tenant_id: "common",
  client_id: "059c82a8-4d77-4b19-a6c7-d702dde10960",
  callback_url: "https://swarm.test/auth/email/callback",
};

test("setup is one consent click, with nothing to type", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/configuration") && !init?.method) return ok(bundled);
    if (url.endsWith("/auth/start") && init?.method === "POST") return ok({ authorization_url: "https://login.microsoftonline.test/authorize" });
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

  // THE WHOLE POINT OF THE TICKET. Setup used to be a tenant id, a client id
  // and a client secret, all required, behind an errand in the Entra portal.
  expect(await screen.findByRole("button", { name: "Sign in with Microsoft" })).toBeEnabled();
  expect(screen.queryByRole("textbox", { name: /Application \(client\) ID/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("form", { name: "Microsoft app setup" })).not.toBeInTheDocument();
  expect(screen.queryByRole("radio")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /app registration|own Microsoft app|own app/i })).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Sign in with Microsoft" }));
  await waitFor(() => expect(navigate).toHaveBeenCalledWith("https://login.microsoftonline.test/authorize"));
});

test("the redirect URI stays legible, because the registration needs it", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => ok(bundled)));
  render(
    <EmailSettings
      operatorToken="operator-token"
      readiness={{ configured: true, connection: "not_connected", account_name: null, account_address: null }}
      unavailable={false}
    />,
  );

  // Microsoft matches redirect URIs exactly. A Hive published at its own
  // address is refused with AADSTS50011 until this exact string is on the
  // shared registration, and nobody can add what they cannot read.
  expect(await screen.findByDisplayValue("https://swarm.test/auth/email/callback")).toBeInTheDocument();
});

test("an expired connection offers a reconnect rather than a setup form", async () => {
  // THE MIGRATION PATH, such as it is. A Hive configured the old way holds
  // tokens issued to a DIFFERENT client id; they stop working, and the whole
  // recovery is this one button. Operator: "I don't care if we break users
  // config, they are all RCG users and we'll just reconnect them."
  vi.stubGlobal("fetch", vi.fn(async () => ok(bundled)));
  render(
    <EmailSettings
      operatorToken="operator-token"
      readiness={{ configured: true, connection: "credentials_invalid", account_name: null, account_address: null }}
      unavailable={false}
    />,
  );

  expect(await screen.findByRole("button", { name: "Reconnect Microsoft account" })).toBeEnabled();
  expect(screen.queryByRole("form", { name: "Microsoft app setup" })).not.toBeInTheDocument();
});

test("shows the connected account without exposing implementation settings", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => ok(bundled)));
  render(
    <EmailSettings
      operatorToken="operator-token"
      readiness={{ configured: true, connection: "ready", account_name: "Bea", account_address: "bea@example.com" }}
      unavailable={false}
    />,
  );

  expect(await screen.findByText("Connected as bea@example.com")).toBeInTheDocument();
  expect(screen.getByText(/Inbox access uses Bea's delegated identity/)).toBeInTheDocument();
  expect(screen.queryByText(/client secret|tenant id/i)).not.toBeInTheDocument();
});

test("offers a direct retry when Outlook readiness is temporarily unavailable", () => {
  vi.stubGlobal("fetch", vi.fn(async () => ok(bundled)));
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
  expect(screen.queryByRole("button", { name: "Sign in with Microsoft" })).not.toBeInTheDocument();
});

function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}
