import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

vi.mock("./terminal/XtermSurface", () => ({ XtermSurface: class {} }));

import { App } from "./App";
import { terminalWorkspace } from "./terminal/TerminalWorkspace";

afterEach(() => {
  cleanup();
  terminalWorkspace.logout();
  vi.unstubAllGlobals();
});

test("reports the connected runtime version", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, json: async () => ({ status: "ok", version: "0.1.0" }) }));
  render(<App />);
  expect(await screen.findByText("Runtime 0.1.0")).toBeInTheDocument();
  expect(screen.getByText("Unlock the local runtime to view workers.")).toBeInTheDocument();
});

test("makes runtime failure visible", async () => {
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));
  render(<App />);
  expect(await screen.findByText("Runtime unavailable")).toBeInTheDocument();
});

test("keeps the operator token in memory and reveals the worker controls", async () => {
  const fetch = vi
    .fn()
    .mockResolvedValueOnce({ ok: true, json: async () => ({ status: "ok", version: "0.1.0" }) })
    .mockResolvedValueOnce({ ok: true, json: async () => ({ type: "sessions", sessions: [] }) });
  vi.stubGlobal("fetch", fetch);
  render(<App />);

  fireEvent.change(screen.getByLabelText("Operator token"), { target: { value: "secret" } });
  fireEvent.click(screen.getByRole("button", { name: "Unlock" }));

  expect(await screen.findByText("No workers running")).toBeInTheDocument();
  expect(screen.getByLabelText("Workspace path")).toBeInTheDocument();
  expect(fetch).toHaveBeenNthCalledWith(
    2,
    "/api/v1/terminal/sessions",
    expect.objectContaining({ cache: "no-store" }),
  );
  expect(screen.queryByDisplayValue("secret")).not.toBeInTheDocument();
});
