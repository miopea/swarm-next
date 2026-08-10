import { render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { App } from "./App";

afterEach(() => vi.unstubAllGlobals());

test("reports the connected runtime version", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, json: async () => ({ status: "ok", version: "0.1.0" }) }));
  render(<App />);
  expect(await screen.findByText("Runtime 0.1.0")).toBeInTheDocument();
  expect(screen.getByText("No workers running")).toBeInTheDocument();
});

test("makes runtime failure visible", async () => {
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));
  render(<App />);
  expect(await screen.findByText("Runtime unavailable")).toBeInTheDocument();
});
