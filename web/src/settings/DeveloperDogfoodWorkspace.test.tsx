import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { DevelopmentRuntime } from "../api";
import DeveloperDogfoodWorkspace from "./DeveloperDogfoodWorkspace";
import { terminalWorkspace } from "../terminal/TerminalWorkspace";
import * as heapEvidence from "../runtime/browserHeapEstimate";

afterEach(() => { cleanup(); terminalWorkspace.logout(); vi.restoreAllMocks(); });
const runtime = { enabled: true, version: "dev-test", source_revision: "abc123", source_dirty: false } as DevelopmentRuntime;

test("heap sampling is explicit and unsupported memory is not presented as zero", () => {
  const read = vi.spyOn(heapEvidence, "readBrowserHeapEstimate").mockReturnValue({ available: false });
  render(<DeveloperDogfoodWorkspace runtime={runtime} version="test" reachable />);
  expect(read).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Sample heap estimate" }));
  expect(read).toHaveBeenCalledTimes(1);
  expect(screen.getByText("JS heap estimate unavailable in this browser.")).toBeVisible();
  read.mockReturnValue({ available: true, source: "chromium_legacy_heap_estimate", captured_at: 1, used_bytes: 1048576, allocated_bytes: 2097152, limit_bytes: 4194304 });
  fireEvent.click(screen.getByRole("button", { name: "Sample heap estimate" }));
  expect(screen.getByText("JS heap estimate: 1.0 MiB used · 2.0 MiB allocated.")).toBeVisible();
  expect(screen.getByText(/Not total browser\/GPU memory/)).toBeVisible();
});

test("uses development detection without another enable toggle", () => {
  const { rerender } = render(<DeveloperDogfoodWorkspace runtime={undefined} version="test" reachable={false} />);
  expect(screen.queryByText("Evidence from your daily Hive")).toBeNull();
  rerender(<DeveloperDogfoodWorkspace runtime={{ ...runtime, enabled: false }} version="test" reachable />);
  expect(screen.queryByText("Evidence from your daily Hive")).toBeNull();
  rerender(<DeveloperDogfoodWorkspace runtime={runtime} version="test" reachable />);
  expect(screen.getByText("Evidence from your daily Hive")).toBeTruthy();
  expect(screen.queryByRole("checkbox")).toBeNull();
  expect(screen.getByText(/Means and maxima are not percentiles/)).toBeTruthy();
  expect(screen.getByText("Terminal apply latency: No samples")).toBeInTheDocument();
  expect(screen.getByText(/Terminal apply includes queueing, parsing and snapshot setup, not confirmed screen paint/)).toBeInTheDocument();
  expect(screen.queryByText(/^Terminal paint:/)).toBeNull();
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

test("shows paired slowest-return phases without labeling them as percentiles", () => {
  vi.spyOn(terminalWorkspace, "coldRestoreEvidence", "get").mockReturnValue({
    started: 20, pending: 0, interrupted: 0, failed: 0, samples: 20, p95_ms: 400, max_ms: 1000,
    slowest: { total_ms: 1000, setup_ms: 700, connection_ms: 300 },
    slowest_fit: { opening_ms: 10, font_ms: 20, layout_ms: 670, frame_count: 2, max_frame_gap_ms: 640 },
  });
  render(<DeveloperDogfoodWorkspace runtime={runtime} version="test" reachable />);
  expect(screen.getByText("Slowest cold return: 1000 ms total · 700 ms renderer setup · 300 ms connection through applied state.")).toBeInTheDocument();
  expect(screen.getByText(/same slowest return, not independent maxima or p95 phases/)).toBeInTheDocument();
  expect(screen.getByText("Setup breakdown for that return: 10 ms opening · 20 ms font readiness · 670 ms layout and initial sizing.")).toBeInTheDocument();
  expect(screen.getByText(/Sizing frames for that return: 2 · longest interval 640 ms/)).toBeInTheDocument();
});
