import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import SharedProfileHint from "./SharedProfileHint";

afterEach(cleanup);
test.each(["Operator", "", "  "])("missing name %s offers review without publishing", (name) => {
  const onReview = vi.fn();
  render(<SharedProfileHint name={name} onReview={onReview} />);
  expect(onReview).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Review shared profile" }));
  expect(onReview).toHaveBeenCalledOnce();
});
test("a saved name removes the prompt without requiring a contact email", () => {
  const { rerender } = render(<SharedProfileHint name="Operator" onReview={vi.fn()} />);
  expect(screen.getByRole("button")).toBeInTheDocument();
  rerender(<SharedProfileHint name="Cora Bee" onReview={vi.fn()} />);
  expect(screen.queryByRole("region")).not.toBeInTheDocument();
});
