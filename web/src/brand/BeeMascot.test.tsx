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

test("the queen is drawn with her diadem and the worker is not", () => {
  // The control room header is the HIVE's mark, so it is the Queen. It
  // defaulted to a worker, which put one of the workers at the top of a page
  // whose whole roster is workers — and since every worker is the same drawing,
  // the header read as just another row.
  const { container: queen } = render(<BeeMascot role="queen" expression="available" />);
  expect(queen.querySelector(".bee-diadem")).not.toBeNull();
  cleanup();
  const { container: worker } = render(<BeeMascot role="worker" expression="available" />);
  expect(worker.querySelector(".bee-diadem")).toBeNull();
});
