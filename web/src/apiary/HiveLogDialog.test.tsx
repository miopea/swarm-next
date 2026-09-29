import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import HiveLogDialog from "./HiveLogDialog";

const ok = (body: unknown) => Promise.resolve(new Response(JSON.stringify(body), { headers: { "Content-Type": "application/json" } }));

afterEach(() => { vi.unstubAllGlobals(); });

const received = {
  entries: [
    { sequence: 1, at_ms: 1_790_700_000_000, level: "info", target: "swarm_api", message: "Swarm API listening" },
    { sequence: 2, at_ms: 1_790_700_001_000, level: "warn", target: "swarm_api::takeover_producer", message: "the takeover holder's keystrokes were refused" },
  ],
  dropped: 3,
  received_at: 1_790_700_002,
};

/**
 * ⚠️ THE POINT OF ADR 0112: what a member said about a failure is readable from
 * the Keeper, newest first, without anyone going to that machine to look.
 */
test("a member's shared log reads newest first and says what was lost", async () => {
  const fetch = vi.fn((_input: RequestInfo | URL) => ok(received));
  vi.stubGlobal("fetch", fetch);
  render(<HiveLogDialog operatorToken="secret" hiveName="WSL Hive" hiveId="hive-2" onClose={vi.fn()} />);

  const lines = await screen.findByRole("list", { name: "Log lines from WSL Hive, newest first" });
  const items = within(lines).getAllByRole("listitem");
  expect(items[0]).toHaveTextContent("the takeover holder's keystrokes were refused");
  expect(items[1]).toHaveTextContent("Swarm API listening");
  expect(screen.getByRole("status")).toHaveTextContent("3 lines were lost before they could be sent");
  expect(String(fetch.mock.calls[0][0])).toBe("/api/v1/apiary/hives/hive-2/diagnostics");

  fireEvent.click(screen.getByLabelText("Warnings and errors only"));
  expect(within(lines).getAllByRole("listitem")).toHaveLength(1);
});

test("this Hive's own log is read from this Hive, and a member that sent nothing says so", async () => {
  const fetch = vi.fn((input: RequestInfo | URL) => String(input) === "/api/v1/diagnostics/log"
    ? ok({ entries: received.entries, dropped: 0, received_at: null })
    : ok({ entries: [], dropped: 0, received_at: null }));
  vi.stubGlobal("fetch", fetch);
  const local = render(<HiveLogDialog operatorToken="secret" hiveName="Keeper Hive" hiveId={undefined} onClose={vi.fn()} />);
  expect(await screen.findByText("This Hive's own log, 2 recent lines.")).toBeInTheDocument();
  local.unmount();

  render(<HiveLogDialog operatorToken="secret" hiveName="Old Hive" hiveId="hive-3" onClose={vi.fn()} />);
  expect(await screen.findByText(/Nothing received from this Hive yet/)).toBeInTheDocument();
});

test("Escape and Close both close it", async () => {
  vi.stubGlobal("fetch", vi.fn(() => ok(received)));
  const onClose = vi.fn();
  render(<HiveLogDialog operatorToken="secret" hiveName="WSL Hive" hiveId="hive-2" onClose={onClose} />);
  const dialog = await screen.findByRole("dialog", { name: "WSL Hive" });
  fireEvent.keyDown(dialog, { key: "Escape" });
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(onClose).toHaveBeenCalledTimes(2);
});
