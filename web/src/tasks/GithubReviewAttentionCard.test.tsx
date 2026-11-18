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
  proposal: null,
  submission: null,
};

const suggestion = {
  verdict: "approve" as const,
  comment: "Why is the old stem name still exported?",
  head_sha: "888059e91c0ffee0123456789abcdef012345678",
  proposed_at: 1_792_700_100,
};

const proposed: GithubReviewOwed = { ...owed, proposal: suggestion };

test("with no proposal, an owed review names the pull request, who asked, and the notes, and links to GitHub", () => {
  const onOpenTask = vi.fn();
  render(<GithubReviewAttentionCard owed={[owed]} onOpenTask={onOpenTask} />);

  expect(screen.getByRole("heading", { name: "Review acme/petal#42" })).toBeInTheDocument();
  expect(screen.getByText(/“Make the petals bloom”\. sorrel asked for your review, and Petal wrote notes for you\./)).toBeInTheDocument();
  expect(screen.getByLabelText("The worker's review notes")).toHaveTextContent("Ask why the old name is still exported.");
  expect(screen.getByText(/Petal recorded no review to submit, so review it on GitHub\./)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Approve and merge" })).not.toBeInTheDocument();
  const link = screen.getByRole("link", { name: "Open on GitHub" });
  expect(link).toHaveAttribute("href", "https://github.com/acme/petal/pull/42");
  expect(link).toHaveAttribute("target", "_blank");
  expect(link).toHaveAttribute("rel", "noreferrer noopener");
  expect(link).toHaveClass("primary-action");

  fireEvent.click(screen.getByRole("button", { name: "Open the task" }));
  expect(onOpenTask).toHaveBeenCalledWith("task-42");
});

/** THE OPERATOR'S ASK: "I should just approve it, add the comment approve and merge". */
test("the worker's suggested review is shown, editable, and one press submits the edited words", () => {
  const onSubmit = vi.fn();
  render(<GithubReviewAttentionCard owed={[proposed]} onSubmit={onSubmit} onOpenTask={vi.fn()} />);

  expect(screen.getByText(/sorrel asked for your review\. Petal read it and suggests you approve\./)).toBeInTheDocument();
  const comment = screen.getByRole("textbox", { name: "Comment to sorrel, posted as you" });
  expect(comment).toHaveValue("Why is the old stem name still exported?");
  // The notes are still there, folded beneath what will be posted.
  expect(screen.getByLabelText("The worker's review notes")).toHaveTextContent("Ask why the old name is still exported.");
  expect(screen.getByText("Petal’s notes")).toBeInTheDocument();

  const approve = screen.getByRole("button", { name: "Approve and merge" });
  expect(approve).toHaveClass("primary-action");
  expect(screen.getByRole("link", { name: "Open on GitHub" })).not.toHaveClass("primary-action");

  fireEvent.change(comment, { target: { value: "Approved. Why is the old stem name still exported?" } });
  fireEvent.click(approve);
  expect(onSubmit).toHaveBeenCalledWith(proposed, "approve", "Approved. Why is the old stem name still exported?");
});

test("requesting changes needs a comment; approving does not", () => {
  const onSubmit = vi.fn();
  render(<GithubReviewAttentionCard owed={[proposed]} onSubmit={onSubmit} onOpenTask={vi.fn()} />);

  fireEvent.change(screen.getByRole("textbox", { name: /Comment to sorrel/ }), { target: { value: "  " } });
  expect(screen.getByRole("button", { name: "Request changes" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Approve and merge" }));
  expect(onSubmit).toHaveBeenCalledWith(proposed, "approve", "  ");
});

test("a suggestion to request changes is the first and primary button", () => {
  const onSubmit = vi.fn();
  render(
    <GithubReviewAttentionCard
      owed={[{ ...proposed, proposal: { ...suggestion, verdict: "request_changes" } }]}
      onSubmit={onSubmit}
      onOpenTask={vi.fn()}
    />,
  );
  expect(screen.getByText(/Petal read it and suggests you request changes\./)).toBeInTheDocument();
  const buttons = screen.getAllByRole("button").map((button) => button.textContent);
  expect(buttons.indexOf("Request changes")).toBeLessThan(buttons.indexOf("Approve and merge"));
  const requestChanges = screen.getByRole("button", { name: "Request changes" });
  expect(requestChanges).toHaveClass("primary-action");
  expect(screen.getByRole("button", { name: "Approve and merge" })).not.toHaveClass("primary-action");
  fireEvent.click(requestChanges);
  expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({ task_id: "task-42" }), "request_changes", "Why is the old stem name still exported?");
});

test("a press in flight holds the buttons, and one GitHub refused says why in its words", () => {
  const { rerender } = render(
    <GithubReviewAttentionCard owed={[{ ...proposed, submission: { id: 1, state: "posting", outcome: null } }]} onSubmit={vi.fn()} onOpenTask={vi.fn()} />,
  );
  expect(screen.getByRole("status")).toHaveTextContent("Submitting to GitHub…");
  expect(screen.getByRole("button", { name: "Approve and merge" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Request changes" })).toBeDisabled();

  rerender(
    <GithubReviewAttentionCard
      owed={[{ ...proposed, submission: { id: 1, state: "post_uncertain", outcome: "Swarm is not sure GitHub took the review (no answer), and is checking" } }]}
      onSubmit={vi.fn()}
      onOpenTask={vi.fn()}
    />,
  );
  expect(screen.getByRole("status")).toHaveTextContent("is checking");
  expect(screen.getByRole("button", { name: "Approve and merge" })).toBeDisabled();

  rerender(
    <GithubReviewAttentionCard
      owed={[{ ...proposed, submission: { id: 1, state: "failed", outcome: "Can not approve your own pull request" } }]}
      onSubmit={vi.fn()}
      onOpenTask={vi.fn()}
    />,
  );
  expect(screen.getByRole("alert")).toHaveTextContent("Not submitted: Can not approve your own pull request");
  expect(screen.getByRole("button", { name: "Approve and merge" })).toBeEnabled();
});

test("while another action runs, nothing can be pressed", () => {
  render(<GithubReviewAttentionCard owed={[proposed]} busy onSubmit={vi.fn()} onOpenTask={vi.fn()} />);
  expect(screen.getByRole("button", { name: "Approve and merge" })).toBeDisabled();
  expect(screen.getByRole("textbox", { name: /Comment to sorrel/ })).toBeDisabled();
});

test("what a press did stays after the review leaves Needs you, until dismissed", () => {
  const onDismissResult = vi.fn();
  const result = {
    task_id: "task-42",
    reference: "acme/petal#42",
    state: "done" as const,
    outcome: "Approved. Not merged: it is still a draft. Merge it on GitHub once its author marks it ready.",
  };
  const { rerender } = render(
    <GithubReviewAttentionCard owed={[]} results={[result]} onDismissResult={onDismissResult} onOpenTask={vi.fn()} />,
  );
  expect(screen.getByRole("heading", { name: "acme/petal#42 reviewed" })).toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("Approved. Not merged: it is still a draft.");
  fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
  expect(onDismissResult).toHaveBeenCalledWith("task-42");

  // While the review is still owed, its own card says where it stands.
  rerender(
    <GithubReviewAttentionCard owed={[proposed]} results={[result]} onDismissResult={onDismissResult} onOpenTask={vi.fn()} />,
  );
  expect(screen.queryByRole("heading", { name: "acme/petal#42 reviewed" })).not.toBeInTheDocument();
});

test("nothing on an owed review's card claims it is done: GitHub clears it", () => {
  render(<GithubReviewAttentionCard owed={[owed, { ...proposed, task_id: "task-43" }]} onOpenTask={vi.fn()} />);
  expect(screen.queryByRole("button", { name: /done|dismiss|reviewed/i })).not.toBeInTheDocument();
  expect(screen.getByText(/clears by itself once you submit your review there/)).toBeInTheDocument();
});

test("one card per review, and a link that is not GitHub's is not offered", () => {
  render(
    <GithubReviewAttentionCard
      owed={[owed, { ...owed, task_id: "task-43", reference: "acme/petal#43", url: "javascript:alert(1)", requested_by: null, worker_name: null, notes: null }]}
      onOpenTask={vi.fn()}
    />,
  );
  expect(screen.getAllByRole("heading", { name: /^Review acme\/petal#4[23]$/ })).toHaveLength(2);
  expect(screen.getAllByRole("link", { name: "Open on GitHub" })).toHaveLength(1);
  expect(screen.getByText(/Someone asked for your review, and the notes are written\./)).toBeInTheDocument();
});

test("nothing owed and nothing to report renders nothing", () => {
  const { container } = render(<GithubReviewAttentionCard owed={[]} onOpenTask={vi.fn()} />);
  expect(container).toBeEmptyDOMElement();
});
