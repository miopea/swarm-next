import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import ApiaryAttentionCard from "./ApiaryAttentionCard";

test("surfaces pending Steward help without implying a terminal interruption", () => {
  const onReview = vi.fn();
  render(<ApiaryAttentionCard pendingAssistance={2} onReview={onReview} />);

  expect(screen.getByRole("heading", { name: "A trusted Steward offered help" })).toBeInTheDocument();
  expect(screen.getByText(/Nothing was sent to a worker or terminal/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Review in Apiary" }));
  expect(onReview).toHaveBeenCalledOnce();
});

test("stays absent when there is no pending help", () => {
  const { container } = render(<ApiaryAttentionCard pendingAssistance={0} onReview={() => undefined} />);
  expect(container).toBeEmptyDOMElement();
});
