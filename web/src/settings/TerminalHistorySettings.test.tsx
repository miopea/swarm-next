import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import TerminalHistorySettings from "./TerminalHistorySettings";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
const ok = (body: unknown) => new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });

test("an operator chooses how long terminal history is kept, and sees whether the host keeps it", async () => {
  const fetch = vi.fn().mockImplementation((_input: RequestInfo | URL, init?: RequestInit) => Promise.resolve(ok(
    init?.method === "PUT"
      ? { days: JSON.parse(String(init.body)).days, set_by_apiary: null, host: "applying", host_max_age_seconds: 7 * 86_400 }
      : { days: 7, set_by_apiary: null, host: "in_effect", host_max_age_seconds: 7 * 86_400 },
  )));
  vi.stubGlobal("fetch", fetch);
  render(<TerminalHistorySettings operatorToken="secret" />);
  const keep = screen.getByLabelText("Keep");
  await waitFor(() => expect(keep).toBeEnabled());
  expect(screen.getByRole("status")).toHaveTextContent("In effect.");
  fireEvent.change(keep, { target: { value: "3" } });
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("The terminal host still keeps 7 days, and takes this within a minute."));
  expect(fetch.mock.calls[1]?.[1].body).toBe(JSON.stringify({ days: 3 }));
});

/** ⚠️ ADR 0113 and 0118: locked when the Apiary sets it, and honest that an older host hasn't taken it. */
test("days the Apiary sets are locked, and wait for the engine update", async () => {
  vi.stubGlobal("fetch", vi.fn().mockImplementation(() => Promise.resolve(ok({ days: 3, set_by_apiary: 3, host: "awaiting_engine_update", host_max_age_seconds: null }))));
  render(<TerminalHistorySettings operatorToken="secret" />);
  await waitFor(() => expect(screen.getByLabelText("Keep")).toHaveValue("3"));
  expect(screen.getByLabelText("Keep")).toBeDisabled();
  expect(screen.getByLabelText("Keep")).toHaveAccessibleDescription("Set by your Apiary. Only its Keeper can change this.");
  expect(screen.getByRole("status")).toHaveTextContent("Takes effect after the worker engine update");
});
