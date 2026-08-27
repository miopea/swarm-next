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

test("a mark draws without touching the eyes or the mouth", () => {
  // The property the whole design rests on: identity and state have to coexist.
  // A bee can be the one with the spectacles AND be blocked, and neither
  // reading may interfere with the other.
  const { container } = render(<BeeMascot expression="blocked" mark="spectacles" />);
  expect(container.querySelector(".bee-kit")).not.toBeNull();
  expect(container.querySelectorAll(".bee-eye")).toHaveLength(2);
  expect(container.querySelector(".bee-mouth")).not.toBeNull();
});

test("hair is drawn before the head so the head overlaps it", () => {
  // Order is what makes it read as hair rather than as a hat sitting on top.
  const { container } = render(<BeeMascot mark="pigtails" />);
  const svg = container.querySelector("svg")!;
  const nodes = Array.from(svg.children);
  const hair = nodes.findIndex((node) => node.classList.contains("bee-hair"));
  const head = nodes.findIndex((node) => node.classList.contains("bee-head"));
  expect(hair).toBeGreaterThanOrEqual(0);
  expect(hair).toBeLessThan(head);
});

test("no mark leaves the bee plain", () => {
  const { container } = render(<BeeMascot />);
  expect(container.querySelector(".bee-kit")).toBeNull();
  expect(container.querySelector(".bee-hair")).toBeNull();
});
