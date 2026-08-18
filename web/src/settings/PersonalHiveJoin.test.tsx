import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import PersonalHiveJoin from "./PersonalHiveJoin";

afterEach(() => {
  cleanup();
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
