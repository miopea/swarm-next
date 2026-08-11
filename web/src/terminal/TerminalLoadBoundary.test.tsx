import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import TerminalLoadBoundary from "./TerminalLoadBoundary";

afterEach(() => vi.restoreAllMocks());

test("keeps a terminal load failure contained and offers a safe refresh", () => {
  vi.spyOn(console, "error").mockImplementation(() => undefined);
  const reload = vi.fn();

  render(<TerminalLoadBoundary onReload={reload}><BrokenTerminal /></TerminalLoadBoundary>);

  expect(screen.getByRole("alert")).toHaveTextContent("Your worker is still running");
  fireEvent.click(screen.getByRole("button", { name: "Refresh Swarm" }));
  expect(reload).toHaveBeenCalledOnce();
});

function BrokenTerminal(): never {
  throw new Error("Failed to fetch dynamically imported module");
}
