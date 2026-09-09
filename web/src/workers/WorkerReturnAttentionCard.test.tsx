import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import WorkerReturnAttentionCard from "./WorkerReturnAttentionCard";

afterEach(cleanup);

test("distinguishes failure from uncertainty and offers inspection, not a blind wake", () => {
  const review = vi.fn();
  render(<WorkerReturnAttentionCard workers={[
    { id: "one", name: "Poppy", return_attention: "failed" },
    { id: "two", name: "Clover", return_attention: "unconfirmed" },
    { id: "three", name: "Healthy" },
  ]} onReview={review} />);
  expect(screen.getByText("2 workers need help returning")).toBeInTheDocument();
  expect(screen.getByText(/The engine could not start/)).toBeInTheDocument();
  expect(screen.getByText(/Check existing sessions before retrying/)).toBeInTheDocument();
  expect(screen.queryByText("Healthy")).not.toBeInTheDocument();
  expect(screen.getAllByRole("button")).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Review recovery" }));
  expect(review).toHaveBeenCalledOnce();
});

test("resolved outcomes disappear instead of leaving a stale alert", () => {
  const { rerender } = render(<WorkerReturnAttentionCard workers={[
    { id: "one", name: "Poppy", return_attention: "unconfirmed" },
  ]} onReview={vi.fn()} />);
  expect(screen.getByRole("region")).toBeInTheDocument();
  rerender(<WorkerReturnAttentionCard workers={[{ id: "one", name: "Poppy" }]} onReview={vi.fn()} />);
  expect(screen.queryByRole("region")).not.toBeInTheDocument();
});
