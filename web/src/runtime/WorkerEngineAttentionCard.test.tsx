import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import WorkerEngineAttentionCard from "./WorkerEngineAttentionCard";

test("a partial stop names the workers it left down and opens the engine card", () => {
  const onReview = vi.fn();
  render(<WorkerEngineAttentionCard
    attention={{ kind: "stopped_partway", detail: "Stopped Ledger while resting, then could not stop Pantry.", stopped: ["Ledger"] }}
    onReview={onReview}
  />);
  expect(screen.getByRole("heading", { name: "The automatic engine update stopped partway" })).toBeInTheDocument();
  expect(screen.getByText("Ledger")).toBeInTheDocument();
  expect(screen.getByText(/comes back once the engine is updated/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Open worker engine" }));
  expect(onReview).toHaveBeenCalledOnce();
});

test("a day of waiting says what holds it up, or that it is switched off", () => {
  const { rerender } = render(<WorkerEngineAttentionCard
    attention={{ kind: "waiting", hours: 26, reason: "Pantry is mid-turn", enabled: true }}
    onReview={vi.fn()}
  />);
  expect(screen.getByRole("heading", { name: "The worker engine update has waited 26 hours" })).toBeInTheDocument();
  expect(screen.getByText(/Not yet: Pantry is mid-turn/)).toBeInTheDocument();
  rerender(<WorkerEngineAttentionCard attention={{ kind: "waiting", hours: 26, reason: null, enabled: false }} onReview={vi.fn()} />);
  expect(screen.getByText(/Automatic engine updates are off/)).toBeInTheDocument();
  rerender(<WorkerEngineAttentionCard attention={null} onReview={vi.fn()} />);
  expect(screen.queryByRole("heading")).toBeNull();
});
