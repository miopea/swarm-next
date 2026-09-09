import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { fetchQueenRunHistory, type QueenRunHistory as History } from "../api/queenHistory";
import QueenRunHistory from "./QueenRunHistory";

vi.mock("../api/queenHistory", () => ({ fetchQueenRunHistory: vi.fn() }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });
function history(): History {
  return { retention_days: 30, max_retained: 4096, retained_count: 200, records: [{
    run_id: "fixture", finished_on_build: "build-a", trigger: "manual", requested_at: 100,
    delivered_at: 102, finished_at: 110, attempts: 1, initial_actionable_count: 0,
    requested_outcome: "needs_operator", accepted_outcome: "no_action",
  }] };
}

test("review follow-ups retain build and sample boundaries without claiming pending work or yield", async () => {
  vi.mocked(fetchQueenRunHistory).mockResolvedValue({ ...history(), review_returns: {
    retained_count: 200, max_retained: 4096, retention_days: 30, records: [
      { request_id: "a", returned_on_build: "return-build", returned_at: 100, answered_at: 110 },
      { request_id: "b", returned_on_build: "return-build", returned_at: 101, answered_at: null },
      { request_id: "c", returned_on_build: "return-build", returned_at: 102, answered_at: 99 },
    ],
  } });
  render(<QueenRunHistory operatorToken="token" />);
  expect(await screen.findByText(/3 recent returns shown of 200 retained/)).toBeInTheDocument();
  expect(screen.getByText("return-build · 3 follow-ups · 2 exact answers")).toBeInTheDocument();
  expect(screen.getByText("Mean return-to-answer: 10 seconds (1 sample).")).toBeInTheDocument();
  expect(screen.getByText(/Without a recorded exact answer: 1/)).toHaveTextContent("not the current waiting queue");
});

test("an older API does not imply that no reviews were returned", async () => {
  vi.mocked(fetchQueenRunHistory).mockResolvedValue(history());
  render(<QueenRunHistory operatorToken="token" />);
  expect(await screen.findByText(/Review-return history is unavailable on this API/)).toBeInTheDocument();
  expect(screen.queryByText(/0 recent returns/)).not.toBeInTheDocument();
});

test("shows partial coverage and normalized outcomes without claiming task productivity", async () => {
  vi.mocked(fetchQueenRunHistory).mockResolvedValue(history());
  render(<QueenRunHistory operatorToken="token" />);
  expect(await screen.findByText(/1 recent finishes shown of 200 retained/)).toBeInTheDocument();
  expect(screen.getByText("build-a · 1 finishes · 1 no action · 0 incomplete")).toBeInTheDocument();
  expect(screen.getByText(/Mean request-to-delivery: 2 seconds \(1 samples\)/)).toHaveTextContent("Mean delivery-to-finish: 8 seconds");
  expect(screen.getByText(/1 requested outcomes changed/)).toBeInTheDocument();
  expect(screen.getByText(/not task completions or a productivity score/)).toBeInTheDocument();
});

test("missing and backwards clocks are unavailable rather than zero", async () => {
  const data = history();
  data.records[0] = { ...data.records[0], finished_on_build: null, requested_at: null, delivered_at: 120 };
  vi.mocked(fetchQueenRunHistory).mockResolvedValue(data);
  render(<QueenRunHistory operatorToken="token" />);
  expect(await screen.findByText(/Build not recorded ·/)).toBeInTheDocument();
  expect(screen.getByText("Mean request-to-delivery: Unavailable. Mean delivery-to-finish: Unavailable.")).toBeInTheDocument();
});

test("failed refresh preserves last known history and a later refresh recovers", async () => {
  vi.mocked(fetchQueenRunHistory).mockResolvedValueOnce(history()).mockRejectedValueOnce(new Error("offline"))
    .mockResolvedValueOnce({ ...history(), records: [], retained_count: 0 });
  render(<QueenRunHistory operatorToken="token" />);
  await screen.findByText(/build-a ·/);
  fireEvent.click(screen.getByRole("button", { name: "Refresh Queen history" }));
  expect(await screen.findByRole("status")).toHaveTextContent("last known");
  expect(screen.getByText(/build-a ·/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Refresh Queen history" }));
  expect(await screen.findByText(/No recorded finishes yet/)).toBeInTheDocument();
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});

test("initial failure never masquerades as an empty history and unmount cancels reads", async () => {
  vi.mocked(fetchQueenRunHistory).mockRejectedValueOnce(new Error("offline")).mockImplementation(() => new Promise(() => undefined));
  const { unmount } = render(<QueenRunHistory operatorToken="token" />);
  expect(await screen.findByText("Queen history is unavailable. Any displayed records are last known.")).toHaveAttribute("role", "status");
  expect(screen.queryByText(/No recorded finishes yet/)).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Refresh Queen history" }));
  await act(async () => { await Promise.resolve(); });
  const signal = vi.mocked(fetchQueenRunHistory).mock.calls.at(-1)![1];
  expect(signal.aborted).toBe(false);
  unmount();
  expect(signal.aborted).toBe(true);
});
