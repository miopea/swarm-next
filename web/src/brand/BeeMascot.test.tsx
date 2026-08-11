import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";

import BeeMascot from "./BeeMascot";

afterEach(cleanup);

test("exposes a labelled mascot as an image", () => {
  render(<BeeMascot role="queen" expression="complete" label="Task completed by the queen" />);

  const mascot = screen.getByRole("img", { name: "Task completed by the queen" });
  expect(mascot).toHaveClass("bee-queen", "bee-complete");
});

test("keeps decorative worker avatars out of the accessibility tree", () => {
  const { container } = render(<BeeMascot expression="focused" />);

  expect(container.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
  expect(screen.queryByRole("img")).not.toBeInTheDocument();
});
