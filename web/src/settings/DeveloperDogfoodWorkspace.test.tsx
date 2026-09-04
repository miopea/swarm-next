import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import type { DevelopmentRuntime } from "../api";
import DeveloperDogfoodWorkspace from "./DeveloperDogfoodWorkspace";
import { terminalWorkspace } from "../terminal/TerminalWorkspace";

afterEach(() => { cleanup(); terminalWorkspace.logout(); });
const runtime = { enabled: true, version: "dev-test", source_revision: "abc123", source_dirty: false } as DevelopmentRuntime;

test("uses development detection without another enable toggle", () => {
  const { rerender } = render(<DeveloperDogfoodWorkspace runtime={undefined} version="test" reachable={false} />);
  expect(screen.queryByText("Evidence from your daily Hive")).toBeNull();
  rerender(<DeveloperDogfoodWorkspace runtime={{ ...runtime, enabled: false }} version="test" reachable />);
  expect(screen.queryByText("Evidence from your daily Hive")).toBeNull();
  rerender(<DeveloperDogfoodWorkspace runtime={runtime} version="test" reachable />);
  expect(screen.getByText("Evidence from your daily Hive")).toBeTruthy();
  expect(screen.queryByRole("checkbox")).toBeNull();
  expect(screen.getByText(/Means and maxima are not percentiles/)).toBeTruthy();
});

test("qualifies unavailable status and only previews evidence on request", () => {
  const { container } = render(<DeveloperDogfoodWorkspace runtime={runtime} version="running-version" reachable={false} />);
  expect(screen.getByRole("status").textContent).toContain("last known");
  expect(container.querySelector("pre")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Refresh evidence" }));
  fireEvent.click(screen.getByRole("button", { name: "Preview browser evidence" }));
  const evidence = JSON.parse(container.querySelector("pre")!.textContent!);
  expect(evidence.running_version).toBe("running-version");
  expect(evidence.checkout_revision).toBe("abc123");
  expect(evidence.browser.current.schema).toBe(1);
  expect(evidence.cold_view_restores.samples).toBe(0);
  expect(evidence.cold_view_restores.p95_ms).toBeNull();
  expect(evidence.renderer_pool.retained).toBe(0);
  fireEvent.click(screen.getByRole("button", { name: "Preview browser evidence" }));
  expect(container.querySelector("pre")).toBeNull();
});

test("warm pool is explicit, reversible, and disabled when development mode ends", () => {
  const { rerender } = render(<DeveloperDogfoodWorkspace runtime={runtime} version="test" reachable />);
  const toggle = screen.getByRole("button", { name: "Try five-renderer pool" });
  expect(toggle).toHaveAttribute("aria-pressed", "false");
  fireEvent.click(toggle);
  expect(terminalWorkspace.rendererRetention.limit).toBe(5);
  fireEvent.click(screen.getByRole("button", { name: "Stop warm-pool experiment" }));
  expect(terminalWorkspace.rendererRetention.limit).toBeUndefined();
  fireEvent.click(screen.getByRole("button", { name: "Try five-renderer pool" }));
  rerender(<DeveloperDogfoodWorkspace runtime={{ ...runtime, enabled: false }} version="test" reachable />);
  expect(terminalWorkspace.rendererRetention.limit).toBeUndefined();
  expect(screen.queryByRole("button", { name: "Stop warm-pool experiment" })).not.toBeInTheDocument();
});
