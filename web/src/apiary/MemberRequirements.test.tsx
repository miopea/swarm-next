import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";

import MemberRequirements from "./MemberRequirements";

test("a member is told what its Apiary requires, who set it, and the way out", () => {
  render(<MemberRequirements settings={[{ key: "release.auto_install", value: "on" }]} />);
  expect(screen.getByRole("heading", { name: "Your Apiary requires" })).toBeInTheDocument();
  expect(screen.getByText("Ordinary releases install automatically")).toBeInTheDocument();
  expect(screen.getByText("Required on")).toBeInTheDocument();
  expect(screen.getByText(/leaving the Apiary is the way out/)).toBeInTheDocument();
});

test("an Apiary that requires nothing adds nothing to the page", () => {
  const { container } = render(<MemberRequirements settings={[]} />);
  expect(container).toBeEmptyDOMElement();
});
