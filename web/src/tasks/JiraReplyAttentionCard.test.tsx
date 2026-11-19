import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { JiraReplyAwaiting } from "../api";
import JiraReplyAttentionCard from "./JiraReplyAttentionCard";

afterEach(cleanup);

const drafted: JiraReplyAwaiting = {
  task_id: "task-204",
  issue_key: "HELP-204",
  project_name: "Help Desk",
  title: "The garden map will not load on a phone",
  worker_name: "Garden",
  requester_name: "Robin Vale",
  body: "The map loads again on phones.",
  drafted_at: 1_792_700_000,
  resolution_held: false,
  resolution_waiting: false,
  posted: null,
};

test("the operator edits the draft and one press posts their words", () => {
  const onPost = vi.fn();
  render(<JiraReplyAttentionCard replies={[drafted]} onPost={onPost} onDiscard={vi.fn()} onOpenTask={vi.fn()} />);

  expect(screen.getByRole("heading", { name: "Reply on HELP-204" })).toBeInTheDocument();
  expect(screen.getByText(/Garden drafted this reply/)).toHaveTextContent(
    "Garden drafted this reply to Robin Vale, who raised it. Nothing is posted until you press.",
  );
  expect(screen.queryByRole("button", { name: "Post reply and resolve" })).not.toBeInTheDocument();

  fireEvent.change(screen.getByRole("textbox", { name: "Reply posted on HELP-204" }), { target: { value: "Edited." } });
  fireEvent.click(screen.getByRole("button", { name: "Post reply" }));
  expect(onPost).toHaveBeenCalledWith(drafted, "Edited.", false);
});

test("a requester Swarm has not read yet is the person who raised it", () => {
  render(<JiraReplyAttentionCard replies={[{ ...drafted, requester_name: null }]} onPost={vi.fn()} onDiscard={vi.fn()} onOpenTask={vi.fn()} />);

  expect(screen.getByText(/Garden drafted this reply/)).toHaveTextContent("drafted this reply to the person who raised it.");
});

test("a held resolution makes post-and-resolve the first choice", () => {
  const onPost = vi.fn();
  render(<JiraReplyAttentionCard replies={[{ ...drafted, resolution_held: true }]} onPost={onPost} onDiscard={vi.fn()} onOpenTask={vi.fn()} />);

  const buttons = screen.getAllByRole("button").map((button) => button.textContent);
  expect(buttons.slice(0, 2)).toEqual(["Post reply and resolve", "Post reply"]);
  expect(screen.getByRole("button", { name: "Post reply and resolve" })).toHaveClass("primary-action");
  fireEvent.click(screen.getByRole("button", { name: "Post reply and resolve" }));
  expect(onPost).toHaveBeenCalledWith(expect.objectContaining({ task_id: "task-204" }), drafted.body, true);
});

test("a completion waiting on the reply says the ticket resolves once it is posted", () => {
  render(<JiraReplyAttentionCard replies={[{ ...drafted, resolution_waiting: true }]} onPost={vi.fn()} onDiscard={vi.fn()} onOpenTask={vi.fn()} />);

  expect(screen.getByText(/Jira resolves it once this reply is posted/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Post reply and resolve" })).not.toBeInTheDocument();
});

test("an empty reply cannot be posted, and don't post drops it", () => {
  const onPost = vi.fn();
  const onDiscard = vi.fn();
  render(<JiraReplyAttentionCard replies={[drafted]} onPost={onPost} onDiscard={onDiscard} onOpenTask={vi.fn()} />);

  fireEvent.change(screen.getByRole("textbox", { name: "Reply posted on HELP-204" }), { target: { value: "   " } });
  expect(screen.getByRole("button", { name: "Post reply" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Don’t post" }));
  expect(onDiscard).toHaveBeenCalledWith("task-204");
  expect(onPost).not.toHaveBeenCalled();
});

test("Jira's refusal is shown in words and the reply can be posted again", () => {
  const onPost = vi.fn();
  render(
    <JiraReplyAttentionCard
      replies={[{ ...drafted, posted: { state: "conflict", error: "permission_denied", posted_at: 1_792_700_100, resolve: false } }]}
      onPost={onPost}
      onDiscard={vi.fn()}
      onOpenTask={vi.fn()}
    />,
  );

  expect(screen.getByRole("alert")).toHaveTextContent("Not posted: this Hive's Jira account is not allowed to comment on it.");
  fireEvent.click(screen.getByRole("button", { name: "Post reply" }));
  expect(onPost).toHaveBeenCalledTimes(1);
});

test("a reply on its way waits, and one Swarm lost track of says to check the ticket", () => {
  const { rerender } = render(
    <JiraReplyAttentionCard
      replies={[{ ...drafted, posted: { state: "queued", error: "network_unavailable", posted_at: 1_792_700_100, resolve: true } }]}
      onPost={vi.fn()}
      onDiscard={vi.fn()}
      onOpenTask={vi.fn()}
    />,
  );
  expect(screen.getByRole("status")).toHaveTextContent("Not posted yet: Jira could not be reached. Swarm tries again by itself.");
  expect(screen.getByRole("button", { name: "Post reply" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Don’t post" })).toBeDisabled();
  expect(screen.getByRole("textbox", { name: "Reply posted on HELP-204" })).toBeDisabled();

  rerender(
    <JiraReplyAttentionCard
      replies={[{ ...drafted, posted: { state: "uncertain", error: "delivery_interrupted", posted_at: 1_792_700_100, resolve: false } }]}
      onPost={vi.fn()}
      onDiscard={vi.fn()}
      onOpenTask={vi.fn()}
    />,
  );
  expect(screen.getByRole("alert")).toHaveTextContent("may already be on the ticket. Check HELP-204");
  expect(screen.getByRole("button", { name: "Post reply" })).toBeEnabled();
});

test("a reply Jira took stays on screen until dismissed", () => {
  const onDismiss = vi.fn();
  render(
    <JiraReplyAttentionCard
      replies={[]}
      results={[{ task_id: "task-201", issue_key: "HELP-201", state: "delivered", error: null, resolution: "delivered" }]}
      onDismissResult={onDismiss}
      onOpenTask={vi.fn()}
    />,
  );

  expect(screen.getByRole("heading", { name: "Reply posted on HELP-201" })).toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("Jira has the reply. The ticket is resolved.");
  fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
  expect(onDismiss).toHaveBeenCalledWith("task-201");
});

test("nothing waiting renders nothing", () => {
  const { container } = render(<JiraReplyAttentionCard replies={[]} onOpenTask={vi.fn()} />);
  expect(container).toBeEmptyDOMElement();
});
