import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import WorkerModelSettings from "./WorkerModelSettings";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
const ok = (body: unknown) => new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });

test("an operator chooses the model every repository worker starts on", async () => {
  const fetch = vi.fn().mockImplementation((_input: RequestInfo | URL, init?: RequestInit) => Promise.resolve(ok(
    init?.method === "PUT"
      ? { model: JSON.parse(String(init.body)).model, set_by_apiary: null, workers_short: 2 }
      : { model: "default", set_by_apiary: null, workers_short: 0 },
  )));
  vi.stubGlobal("fetch", fetch);
  render(<WorkerModelSettings operatorToken="secret" />);
  const model = screen.getByLabelText("Model");
  await waitFor(() => expect(model).toBeEnabled());
  expect(screen.getByText("Swarm names no model, and Claude Code uses its own default.")).toBeInTheDocument();
  fireEvent.change(model, { target: { value: "sonnet" } });
  expect(await screen.findByText("Every repository worker starts on Sonnet, which costs less than Opus.")).toBeInTheDocument();
  expect(fetch.mock.calls[1]?.[1].body).toBe(JSON.stringify({ model: "sonnet" }));
  expect(screen.getByRole("status")).toHaveTextContent("2 workers are still on another model, and switch to Sonnet when they next start.");
});

/** ⚠️ ADR 0113: a model the Apiary sets is locked here and says who can change it. */
test("a model the Apiary sets is locked", async () => {
  vi.stubGlobal("fetch", vi.fn().mockImplementation(() => Promise.resolve(ok({ model: "haiku", set_by_apiary: "haiku", workers_short: 1 }))));
  render(<WorkerModelSettings operatorToken="secret" />);
  await waitFor(() => expect(screen.getByLabelText("Model")).toHaveValue("haiku"));
  expect(screen.getByLabelText("Model")).toBeDisabled();
  expect(screen.getByLabelText("Model")).toHaveAccessibleDescription("Set by your Apiary. Only its Keeper can change this.");
  expect(screen.getByRole("status")).toHaveTextContent("1 worker is still on another model, and switches to Haiku when it next starts.");
});
