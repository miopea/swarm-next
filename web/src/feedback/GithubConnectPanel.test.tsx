import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import GithubConnectPanel from "./GithubConnectPanel";

afterEach(cleanup);

function ok(body: unknown) {
  return { ok: true, status: 200, json: () => Promise.resolve(body) } as Response;
}

/**
 * Connecting is an offer, never a gate.
 *
 * The operator's requirement was "frictionless for them to submit feedback",
 * and any connect flow — device flow included — is a code, a context switch and
 * an authorisation. If that stood in front of submitting, the un-connected case
 * would be WORSE than before this feature existed. So the panel states what
 * happens by default and offers the upgrade beside it.
 */
test("tells an unconnected person their report is anonymous, and offers the reason to change that", () => {
  render(
    <GithubConnectPanel
      operatorToken="token"
      connection={{ connected: false, lapsed: false, login: null }}
      onChanged={vi.fn()}
    />,
  );

  expect(screen.getByText(/filed anonymously, so nobody can reply/i)).toBeInTheDocument();
  // Named by what it BUYS. "Connect GitHub" describes a mechanism; hearing back
  // is the reason anyone would bother.
  expect(screen.getByRole("button", { name: /hear back/i })).toBeInTheDocument();
});

test("names the account once connected, and offers to undo it", () => {
  render(
    <GithubConnectPanel
      operatorToken="token"
      connection={{ connected: true, lapsed: false, login: "miopea" }}
      onChanged={vi.fn()}
    />,
  );

  expect(screen.getByText("miopea")).toBeInTheDocument();
  expect(screen.getByText(/tells you when it is closed/i)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Disconnect" })).toBeInTheDocument();
});

/**
 * THE ONE THAT MATTERS. GitHub answers the token endpoint with HTTP 200 and an
 * `error` field for as long as the person is still typing their code, so
 * "waiting" is the ordinary case and arrives repeatedly. A client that gives up
 * on the first one abandons an authorisation that is still live, and the person
 * watches the dialog quit while GitHub is still expecting them.
 */
test("keeps waiting while the person is still entering the code", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  let claims = 0;
  vi.stubGlobal("fetch", vi.fn((input: string | URL | Request) => {
    const url = String(input);
    if (url.endsWith("/connect")) {
      return Promise.resolve(ok({
        user_code: "2F49-5696",
        verification_uri: "https://github.com/login/device",
        expires_in: 899,
        interval: 1,
      }));
    }
    claims += 1;
    // Still typing, twice, then done — exactly the shape GitHub produces.
    return Promise.resolve(ok(claims < 3 ? { state: "waiting" } : { state: "connected", login: "miopea" }));
  }));
  const onChanged = vi.fn();

  render(
    <GithubConnectPanel operatorToken="token" connection={{ connected: false, lapsed: false, login: null }} onChanged={onChanged} />,
  );
  fireEvent.click(screen.getByRole("button", { name: /hear back/i }));

  // The code is shown so the person can type it, and where to type it.
  expect(await screen.findByText("2F49-5696")).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "https://github.com/login/device" })).toBeInTheDocument();

  await vi.advanceTimersByTimeAsync(5_000);

  // It did not stop at the first "waiting".
  await waitFor(() => expect(claims).toBeGreaterThanOrEqual(3));
  await waitFor(() => expect(onChanged).toHaveBeenCalled());
  vi.useRealTimers();
});

test("says so when the person declines, instead of waiting forever", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  vi.stubGlobal("fetch", vi.fn((input: string | URL | Request) => {
    const url = String(input);
    if (url.endsWith("/connect")) {
      return Promise.resolve(ok({
        user_code: "AAAA-BBBB", verification_uri: "https://github.com/login/device",
        expires_in: 899, interval: 1,
      }));
    }
    return Promise.resolve(ok({ state: "declined" }));
  }));

  render(
    <GithubConnectPanel operatorToken="token" connection={{ connected: false, lapsed: false, login: null }} onChanged={vi.fn()} />,
  );
  fireEvent.click(screen.getByRole("button", { name: /hear back/i }));
  await screen.findByText("AAAA-BBBB");

  await vi.advanceTimersByTimeAsync(3_000);

  expect(await screen.findByText(/declined on GitHub/i)).toBeInTheDocument();
  vi.useRealTimers();
});

/**
 * A lapse is not the same as never having connected.
 *
 * The App expires user tokens — measured from the operator's own grant: an
 * access token good for eight hours against a refresh token good for six
 * months. When one lapses, filing keeps working and silently goes anonymous, so
 * the person who connected SPECIFICALLY to hear back simply stops hearing
 * anything and nothing looks broken.
 *
 * Telling them "this will be filed anonymously" as though they had never
 * connected would be true and useless. They are owed the reason and the name of
 * the account that stopped working.
 */
test("says a connection expired, and names it, rather than pretending it never existed", () => {
  render(
    <GithubConnectPanel
      operatorToken="token"
      connection={{ connected: false, lapsed: true, login: "miopea" }}
      onChanged={vi.fn()}
    />,
  );

  expect(screen.getByText(/has expired/i)).toBeInTheDocument();
  expect(screen.getByText("miopea")).toBeInTheDocument();
  // The verb changes with the situation: this is not a first connection.
  expect(screen.getByRole("button", { name: "Reconnect GitHub" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Connect GitHub to hear back/ })).toBeNull();
});
