import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import CheckGateSettings from "./CheckGateSettings";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
const ok = (body: unknown) => new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });

test("an operator chooses how strictly work is held to its CI", async () => {
  const fetch = vi.fn().mockImplementation((_input: RequestInfo | URL, init?: RequestInit) => Promise.resolve(ok(
    init?.method === "PUT" ? { level: JSON.parse(String(init.body)).level, set_by_apiary: null } : { level: "off", set_by_apiary: null },
  )));
  vi.stubGlobal("fetch", fetch);
  render(<CheckGateSettings operatorToken="secret" />);
  const level = screen.getByLabelText("Level");
  await waitFor(() => expect(level).toBeEnabled());
  expect(screen.getByText("CI plays no part in closing work.")).toBeInTheDocument();
  fireEvent.change(level, { target: { value: "require" } });
  expect(await screen.findByText(/can't close while its CI fails or is still running/)).toBeInTheDocument();
  expect(fetch.mock.calls[1]?.[1].body).toBe(JSON.stringify({ level: "require" }));
});

/** ⚠️ ADR 0113: a level the Apiary sets is locked here and says who can change it. */
test("a level the Apiary sets is locked", async () => {
  vi.stubGlobal("fetch", vi.fn().mockImplementation(() => Promise.resolve(ok({ level: "require", set_by_apiary: "require" }))));
  render(<CheckGateSettings operatorToken="secret" />);
  await waitFor(() => expect(screen.getByLabelText("Level")).toHaveValue("require"));
  expect(screen.getByLabelText("Level")).toBeDisabled();
  expect(screen.getByLabelText("Level")).toHaveAccessibleDescription("Set by your Apiary. Only its Keeper can change this.");
});
