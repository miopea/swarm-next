import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import PersonalHiveJoin from "./PersonalHiveJoin";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test("does not describe unavailable saved invitations as empty and retries them", async () => {
  let unavailable = true;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (unavailable && url.endsWith("/api/v1/apiary/keeper-links")) return new Response("unavailable", { status: 502 });
    return new Response(JSON.stringify([]), { status: 200, headers: { "Content-Type": "application/json" } });
  }));

  render(<PersonalHiveJoin busy={false} operatorToken="secret" onError={vi.fn()} onMessage={vi.fn()} onJoined={vi.fn()} />);

  expect(await screen.findByText("Saved Keeper invitations could not be fully refreshed. Last-known links remain unchanged.")).toBeInTheDocument();
  unavailable = false;
  fireEvent.click(screen.getByRole("button", { name: "Retry saved invitations" }));
  await waitFor(() => expect(screen.queryByText(/Saved Keeper invitations could not be fully refreshed/)).not.toBeInTheDocument());
  expect(screen.getByText("No Apiary invitation is saved on this Hive.")).toBeInTheDocument();
});

test("keeps a pending invitation visible when the Keeper is temporarily unreachable", async () => {
  vi.useFakeTimers();
  const link = {
    link_id: "link-1",
    keeper_endpoint: "https://keeper.example.test",
    apiary_id: "apiary-1",
    apiary_name: "Wildflower Garden",
    state: "awaiting_approval",
    created_at: 10,
    expires_at: 86_410,
    last_poll_at: null,
    last_error: null,
  };
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/api/v1/apiary/keeper-links/link-1/poll") && init?.method === "POST") {
      return new Response("keeper unavailable", { status: 502 });
    }
    if (url.endsWith("/api/v1/apiary/keeper-links")) return new Response(JSON.stringify([link]), { status: 200, headers: { "Content-Type": "application/json" } });
    return new Response(JSON.stringify([]), { status: 200, headers: { "Content-Type": "application/json" } });
  }));

  render(<PersonalHiveJoin busy={false} operatorToken="secret" onError={vi.fn()} onMessage={vi.fn()} onJoined={vi.fn()} />);
  await vi.waitFor(() => expect(screen.getByText("Wildflower Garden")).toBeInTheDocument());

  await act(async () => { await vi.advanceTimersByTimeAsync(5_000); });

  await vi.waitFor(() => expect(screen.getByText("The Keeper was not reachable on the last check. This Hive keeps the invitation safely and retries every five seconds.")).toBeInTheDocument());
  expect(screen.getByText("Waiting for Keeper approval")).toBeInTheDocument();
});
