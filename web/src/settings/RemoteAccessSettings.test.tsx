import { cleanup, render, screen, waitFor, fireEvent } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import RemoteAccessSettings from "./RemoteAccessSettings";

afterEach(cleanup);
beforeEach(() => vi.restoreAllMocks());

function reply(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });
}

test("says what is missing when cloudflared is not installed", async () => {
  vi.stubGlobal("fetch", vi.fn(async () =>
    reply({ available: false, running: false, url: null, started_at: null, qr_svg: null })));

  render(<RemoteAccessSettings busy={false} operatorToken="secret" />);

  expect(await screen.findByText(/needs/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Open on my phone" })).toBeDisabled();
});

/**
 * The warning is the feature. A random hostname changes every time, and
 * passkeys, the installed app and the signed-in session are all bound to it —
 * so the operator has to be told before they scan, not after their passkey
 * silently stops working.
 */
test("shows the address with a QR and says the address will not last", async () => {
  vi.stubGlobal("fetch", vi.fn(async (url: string) =>
    reply(String(url).includes("/start")
      ? { available: true, running: true, url: "https://neat-lion.trycloudflare.com", started_at: 1787452543, qr_svg: "<svg role='img'></svg>" }
      : { available: true, running: false, url: null, started_at: null, qr_svg: null })));

  render(<RemoteAccessSettings busy={false} operatorToken="secret" />);
  fireEvent.click(await screen.findByRole("button", { name: "Open on my phone" }));

  const link = await screen.findByRole("link", { name: "https://neat-lion.trycloudflare.com" });
  expect(link).toHaveAttribute("href", "https://neat-lion.trycloudflare.com");
  expect(screen.getByText(/This address is temporary/)).toBeInTheDocument();
  expect(screen.getByText(/will not follow it/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Stop sharing" })).toBeInTheDocument();
});

test("never puts the operator token in the shared address", async () => {
  vi.stubGlobal("fetch", vi.fn(async () =>
    reply({ available: true, running: true, url: "https://neat-lion.trycloudflare.com", started_at: 1, qr_svg: null })));

  const { container } = render(<RemoteAccessSettings busy={false} operatorToken="super-secret-token" />);

  await waitFor(() => expect(screen.getByRole("link")).toBeInTheDocument());
  expect(container.innerHTML).not.toContain("super-secret-token");
});

test("surfaces a failure to open the address instead of looking stuck", async () => {
  vi.stubGlobal("fetch", vi.fn(async (url: string) => {
    if (String(url).includes("/start")) throw new Error("cloudflared did not report an address");
    return reply({ available: true, running: false, url: null, started_at: null, qr_svg: null });
  }));

  render(<RemoteAccessSettings busy={false} operatorToken="secret" />);
  fireEvent.click(await screen.findByRole("button", { name: "Open on my phone" }));

  expect(await screen.findByRole("alert")).toHaveTextContent("cloudflared did not report an address");
});
