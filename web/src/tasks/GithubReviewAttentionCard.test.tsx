import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { GithubReviewOwed } from "../api";
import GithubReviewAttentionCard from "./GithubReviewAttentionCard";

afterEach(cleanup);

const owed: GithubReviewOwed = {
  task_id: "task-42",
  url: "https://github.com/acme/petal/pull/42",
  reference: "acme/petal#42",
  title: "Review acme/petal#42: Make the petals bloom",
  requested_by: "sorrel",
  worker_name: "Petal",
  notes: "It renames the stem. Ask why the old name is still exported.",
  owed_since: 1_792_700_000,
};

test("an owed review names the pull request, who asked, and the notes, and links to GitHub", () => {
  const onOpenTask = vi.fn();
  render(<GithubReviewAttentionCard owed={[owed]} onOpenTask={onOpenTask} />);

  expect(screen.getByRole("heading", { name: "Review acme/petal#42 on GitHub" })).toBeInTheDocument();
  expect(screen.getByText(/“Make the petals bloom”\. sorrel asked for your review, and Petal wrote notes for you\./)).toBeInTheDocument();
  expect(screen.getByLabelText("The worker's review notes")).toHaveTextContent("Ask why the old name is still exported.");
  const link = screen.getByRole("link", { name: "Open on GitHub" });
  expect(link).toHaveAttribute("href", "https://github.com/acme/petal/pull/42");
  expect(link).toHaveAttribute("target", "_blank");
  expect(link).toHaveAttribute("rel", "noreferrer noopener");

  fireEvent.click(screen.getByRole("button", { name: "Open the task" }));
  expect(onOpenTask).toHaveBeenCalledWith("task-42");
});

test("nothing on the card claims the review is done: GitHub clears it", () => {
  render(<GithubReviewAttentionCard owed={[owed]} onOpenTask={vi.fn()} />);
  expect(screen.queryByRole("button", { name: /done|dismiss|reviewed/i })).not.toBeInTheDocument();
  expect(screen.getByText(/clears by itself once you submit your review on GitHub/)).toBeInTheDocument();
});

test("one card per review, and a link that is not GitHub's is not offered", () => {
  render(
    <GithubReviewAttentionCard
      owed={[owed, { ...owed, task_id: "task-43", reference: "acme/petal#43", url: "javascript:alert(1)", requested_by: null, worker_name: null, notes: null }]}
      onOpenTask={vi.fn()}
    />,
  );
  expect(screen.getAllByRole("heading", { name: /^Review acme\/petal#4[23] on GitHub$/ })).toHaveLength(2);
  expect(screen.getAllByRole("link", { name: "Open on GitHub" })).toHaveLength(1);
  expect(screen.getByText(/Someone asked for your review, and the notes are written\./)).toBeInTheDocument();
});

test("nothing owed renders nothing", () => {
  const { container } = render(<GithubReviewAttentionCard owed={[]} onOpenTask={vi.fn()} />);
  expect(container).toBeEmptyDOMElement();
});
