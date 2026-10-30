import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import SettingsRestartAction from "./SettingsRestartAction";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
const ok = (body: unknown) => new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });

/** ADR 0132: which workers still run without a setting they read only at start. */
test("names each worker still on an older setting and restarts them only once confirmed", async () => {
  const fetch = vi.fn().mockResolvedValue(ok({ workers: [
    { worker_id: "w1", name: "Admin", settings: ["remote_control", "queen_releases"] },
    { worker_id: "w2", name: "Hub", settings: ["queen_releases"] },
  ] }));
  vi.stubGlobal("fetch", fetch);
  const onRestart = vi.fn().mockResolvedValue(undefined);
  render(<SettingsRestartAction operatorToken="secret" busy={false} onRestart={onRestart} />);

  const card = await screen.findByRole("article", { name: "Settings waiting for a restart" });
  expect(card).toHaveTextContent("Remote Control and Queen releases changed · restart to apply");
  expect(card).toHaveTextContent("These 2 workers started before the change");
  expect(screen.getByText("Admin").closest("li")).toHaveTextContent("Admin: Remote Control and Queen releases");
  expect(screen.getByText("Hub").closest("li")).toHaveTextContent("Hub: Queen releases");
  expect(String(fetch.mock.calls[0]?.[0])).toContain("/api/v1/runtime/settings-restart");

  fireEvent.click(screen.getByRole("button", { name: "Restart 2 running workers" }));
  expect(onRestart).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Restart and apply" }));
  expect(onRestart).toHaveBeenCalledTimes(1);
});

test("shows nothing while every running worker has the current settings", async () => {
  const fetch = vi.fn().mockResolvedValue(ok({ workers: [] }));
  vi.stubGlobal("fetch", fetch);
  const { container } = render(<SettingsRestartAction operatorToken="secret" busy={false} onRestart={vi.fn()} />);
  await waitFor(() => expect(fetch).toHaveBeenCalled());
  expect(container).toBeEmptyDOMElement();
});

test("an unreadable list is said, not shown as nothing to do", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("{}", { status: 503 })));
  render(<SettingsRestartAction operatorToken="secret" busy={false} onRestart={vi.fn()} />);
  expect(await screen.findByRole("article", { name: "Settings waiting for a restart" })).toHaveTextContent("Could not check");
  expect(screen.queryByRole("button")).toBeNull();
});
