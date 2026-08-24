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
    reply({ available: false, running: false, serving: false, error: null, url: null, started_at: null, qr_svg: null })));

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
      ? { available: true, running: true, serving: true, error: null, url: "https://neat-lion.trycloudflare.com", started_at: 1787452543, qr_svg: "<svg role='img'></svg>" }
      : { available: true, running: false, serving: false, error: null, url: null, started_at: null, qr_svg: null })));

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
    reply({ available: true, running: true, serving: true, error: null, url: "https://neat-lion.trycloudflare.com", started_at: 1, qr_svg: null })));

  const { container } = render(<RemoteAccessSettings busy={false} operatorToken="super-secret-token" />);

  await waitFor(() => expect(screen.getByRole("link")).toBeInTheDocument());
  expect(container.innerHTML).not.toContain("super-secret-token");
});

test("surfaces a failure to open the address instead of looking stuck", async () => {
  vi.stubGlobal("fetch", vi.fn(async (url: string) => {
    if (String(url).includes("/start")) throw new Error("cloudflared did not report an address");
    return reply({ available: true, running: false, serving: false, error: null, url: null, started_at: null, qr_svg: null });
  }));

  render(<RemoteAccessSettings busy={false} operatorToken="secret" />);
  fireEvent.click(await screen.findByRole("button", { name: "Open on my phone" }));

  expect(await screen.findByRole("alert")).toHaveTextContent("cloudflared did not report an address");
});

test("a tunnel that never served tells the operator why, rather than a bare status code", async () => {
  // The operator saw "Runtime request returned 502" and nothing else. 502 is in
  // this app's transient set — it means "the runtime is being replaced", is
  // retried as infrastructure noise, and its detail is deliberately not worth
  // showing. A tunnel that never started serving is the opposite: the API
  // answered fine and the reason is the only thing worth reading.
  const reason = "The address was created but never started serving within 45 seconds — Cloudflare answered 404 Not Found for it without routing to this machine. Nothing was published; try again.";
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/api/v1/runtime/tunnel/start")) {
      return new Response(JSON.stringify({ code: "cloudflared_not_reachable", message: reason }), {
        status: 409,
        headers: { "content-type": "application/json" },
      });
    }
    return reply({ available: true, running: false, serving: false, error: null, url: null, started_at: null, qr_svg: null });
  }));

  render(<RemoteAccessSettings operatorToken="operator-token" busy={false} />);
  fireEvent.click(await screen.findByRole("button", { name: "Open on my phone" }));

  const alert = await screen.findByRole("alert");
  await waitFor(() => expect(alert).toHaveTextContent(/never started serving/));
  expect(alert).toHaveTextContent(/without routing to this machine/);
});

test("an address that never served is reported, and no QR is offered until it does", async () => {
  // The operator was handed a QR for an address that served nothing, twice.
  // Measured 2026-08-24: cloudflared can be healthy, registered and in DNS
  // while Cloudflare's edge still routes nothing to it.
  const statuses = [
    { available: true, running: true, serving: false, error: null, url: "https://x.trycloudflare.com", started_at: 1, qr_svg: "<svg/>" },
    { available: true, running: false, serving: false, error: "The address was created but never started serving within 45 seconds.", url: null, started_at: null, qr_svg: null },
  ];
  let read = 0;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/v1/runtime/tunnel/start")) return reply(statuses[0]);
    return reply(statuses[Math.min(read++, statuses.length - 1)]);
  }));

  const { rerender } = render(<RemoteAccessSettings operatorToken="operator-token" busy={false} />);
  fireEvent.click(await screen.findByRole("button", { name: "Open on my phone" }));

  // While it is being checked: said plainly, and no code to scan.
  await waitFor(() => expect(screen.getByText(/Checking the address is reachable/)).toBeInTheDocument());
  expect(document.querySelector(".tunnel-qr")).toBeNull();

  // Once it has given up, the operator is told why.
  read = 1;
  rerender(<RemoteAccessSettings operatorToken="operator-token" busy={true} />);
  rerender(<RemoteAccessSettings operatorToken="operator-token" busy={false} />);
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(/never started serving/));
});

test("the card keeps looking while the address is being checked, and reports the outcome itself", async () => {
  // Moving the check off the request left the card with no reason to look
  // again, so it sat on "Checking the address is reachable" for good. The
  // operator saw a permanent spinner for an answer that had already arrived.
  const checking = { available: true, running: true, serving: false, error: null, url: "https://x.trycloudflare.com", started_at: 1, qr_svg: null };
  const gaveUp = { available: true, running: false, serving: false, error: "The address was created but never started serving within 45 seconds.", url: null, started_at: null, qr_svg: null };
  let reads = 0;
  vi.stubGlobal("fetch", vi.fn(async () => reply(reads++ < 2 ? checking : gaveUp)));

  render(<RemoteAccessSettings operatorToken="operator-token" busy={false} />);
  expect(await screen.findByText(/Checking the address is reachable/)).toBeInTheDocument();

  // Without the poll this never changes, however long anyone waits. The card
  // has to go and look for itself.
  expect(await screen.findByRole("alert", {}, { timeout: 9_000 })).toHaveTextContent(/never started serving/);
}, 12_000);
