import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import KeeperInvitationManager from "./KeeperInvitationManager";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

// Preserve the original real-API-adapter regression alongside mocked lifecycle
// tests: retry must clear an actual failed HTTP response, not just a mock result.
test("clears a temporary invitation status warning after an explicit HTTP retry", async () => {
  let attempts = 0;
  const fetch = vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
    expect(init?.signal).toBeInstanceOf(AbortSignal);
    expect(init?.method).toBeUndefined();
    attempts += 1;
    if (attempts === 1) return new Response("keeper unavailable", { status: 502 });
    return new Response(JSON.stringify([]), { status: 200, headers: { "Content-Type": "application/json" } });
  });
  vi.stubGlobal("fetch", fetch);
  render(<KeeperInvitationManager busy={false} operatorToken="fixture" onInvitationCreated={vi.fn()} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("Invitation status could not be refreshed");
  fireEvent.click(screen.getByRole("button", { name: "Check invitation status again" }));
  await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  expect(screen.getByText("No active invitation links. Create one when another Hive is ready to join.")).toBeVisible();
  expect(fetch).toHaveBeenCalledTimes(2);
});
