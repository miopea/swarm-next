import { render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import AppErrorBoundary from "./AppErrorBoundary";
import { readClientFailures } from "./clientDiagnostics";

afterEach(() => {
  window.sessionStorage.clear();
  vi.restoreAllMocks();
});

test("replaces a blank render failure with a safe recovery screen", () => {
  vi.spyOn(console, "error").mockImplementation(() => undefined);
  function BrokenView(): never { throw new Error("private path should not be stored"); }

  render(<AppErrorBoundary><BrokenView /></AppErrorBoundary>);

  expect(screen.getByRole("heading", { name: "Swarm hit a problem drawing this view" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Reload control room" })).toBeInTheDocument();
  expect(readClientFailures()).toEqual([{ kind: "react_render", occurred_at: expect.any(Number) }]);
  expect(window.sessionStorage.getItem("swarm-next.client-failures.v1")).not.toContain("private path");
});
