import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import TerminalLoadBoundary from "./TerminalLoadBoundary";
import { readClientFailures } from "../feedback/clientDiagnostics";

afterEach(() => { window.sessionStorage.clear(); vi.restoreAllMocks(); });

test.each(["Failed to fetch dynamically imported module", "Unexpected renderer failure at private/path"])("contains %s without inventing an update or worker status", (message) => {
  vi.spyOn(console, "error").mockImplementation(() => undefined);
  const reload = vi.fn();

  render(<TerminalLoadBoundary onReload={reload}><BrokenTerminal message={message} /></TerminalLoadBoundary>);

  expect(screen.getByRole("alert")).toHaveTextContent("Swarm could not display this terminal");
  expect(screen.getByRole("alert")).toHaveTextContent("Refreshing reloads this view; it does not restart the worker");
  expect(screen.getByRole("alert")).not.toHaveTextContent("Swarm was updated");
  expect(screen.getByRole("alert")).not.toHaveTextContent(message);
  expect(readClientFailures()).toEqual([{ kind: "react_render", occurred_at: expect.any(Number) }]);
  expect(window.sessionStorage.getItem("swarm-next.client-failures.v1")).not.toContain(message);
  fireEvent.click(screen.getByRole("button", { name: "Refresh Swarm" }));
  expect(reload).toHaveBeenCalledOnce();
});

function BrokenTerminal({ message }: { message: string }): never {
  throw new Error(message);
}
