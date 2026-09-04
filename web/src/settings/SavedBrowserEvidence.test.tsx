import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { fetchBrowserEvidence, type BrowserEvidenceHour } from "../api";
import SavedBrowserEvidence, { summarizeBrowserEvidence } from "./SavedBrowserEvidence";

vi.mock("../api", () => ({ fetchBrowserEvidence: vi.fn() }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });
function capture(build: string, count: number, total: number): BrowserEvidenceHour {
  const timing = { count: 0, total_ms: 0, max_ms: 0 };
  return { capture_id: "id", build, hour: 3600, revision: 1, long_task: timing, interaction: timing,
    route: { count, total_ms: total, max_ms: total / count }, terminal_render: timing, terminal_reconnect: timing };
}

test("build summaries use sample-weighted means and separate builds", () => {
  const rows = summarizeBrowserEvidence([capture("a", 1, 100), capture("a", 9, 90), capture("b", 1, 20)]);
  expect(rows).toHaveLength(2);
  expect(rows[0].metrics.route).toEqual({ count: 10, total_ms: 190, max_ms: 100 });
});

test("failed refresh retains saved evidence with unavailable status", async () => {
  vi.mocked(fetchBrowserEvidence).mockResolvedValueOnce([capture("build-a", 1, 100)])
    .mockRejectedValueOnce(new Error("offline"));
  render(<SavedBrowserEvidence operatorToken="token" />);
  expect(await screen.findByText("build-a · 1 captures")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Refresh saved history" }));
  expect(await screen.findByRole("status")).toHaveTextContent("unavailable");
  expect(screen.getByText("build-a · 1 captures")).toBeInTheDocument();
});

test("unmount cancels the history read", async () => {
  vi.mocked(fetchBrowserEvidence).mockImplementation((_token, signal) => {
    expect(signal.aborted).toBe(false);
    return new Promise(() => undefined);
  });
  const { unmount } = render(<SavedBrowserEvidence operatorToken="token" />);
  await act(async () => { await Promise.resolve(); });
  expect(fetchBrowserEvidence).toHaveBeenCalledTimes(1);
  unmount();
  for (const [, signal] of vi.mocked(fetchBrowserEvidence).mock.calls) expect(signal.aborted).toBe(true);
});
