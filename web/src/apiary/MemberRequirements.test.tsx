import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";

import MemberRequirements from "./MemberRequirements";

test("a member is told what its Apiary requires, who set it, and the way out", () => {
  render(<MemberRequirements requirements={{ settings: [{ key: "release.auto_install", value: "on" }], shared_work_refusal: null }} />);
  expect(screen.getByRole("heading", { name: "Your Apiary requires" })).toBeInTheDocument();
  expect(screen.getByText("Ordinary releases install automatically")).toBeInTheDocument();
  expect(screen.getByText("Required on")).toBeInTheDocument();
  expect(screen.getByText(/leaving the Apiary is the way out/)).toBeInTheDocument();
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});

test("a member below the Apiary's minimum is told here, before a claim is turned down", () => {
  render(<MemberRequirements requirements={{
    settings: [{ key: "shared_work.minimum_version", value: "1.16.10" }],
    shared_work_refusal: "this Hive runs 1.16.9; the Apiary needs 1.16.10 or newer to take shared work, so update it",
  }} />);
  expect(screen.getByText("Minimum version for shared work")).toBeInTheDocument();
  expect(screen.getByText("1.16.10 or newer")).toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("This Hive runs 1.16.9; the Apiary needs 1.16.10 or newer to take shared work, so update it.");
});

test("an Apiary that requires nothing adds nothing to the page", () => {
  const { container } = render(<MemberRequirements requirements={{ settings: [], shared_work_refusal: null }} />);
  expect(container).toBeEmptyDOMElement();
});
