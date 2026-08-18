import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import KeeperInvitationManager from "./KeeperInvitationManager";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("clears a temporary invitation status warning after an explicit retry", async () => {
  let attempts = 0;
  vi.stubGlobal("fetch", vi.fn(async () => {
    attempts += 1;
    if (attempts === 1) return new Response("keeper unavailable", { status: 502 });
    return new Response(JSON.stringify([]), { status: 200, headers: { "Content-Type": "application/json" } });
  }));

  render(<KeeperInvitationManager busy={false} operatorToken="secret" onInvitationCreated={vi.fn()} />);

  expect(await screen.findByText("Invitation status could not be refreshed. No membership changed.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Check invitation status again" }));
  await waitFor(() => expect(screen.queryByText("Invitation status could not be refreshed. No membership changed.")).not.toBeInTheDocument());
  expect(screen.getByText("No active invitation links. Create one when another Hive is ready to join.")).toBeInTheDocument();
});
