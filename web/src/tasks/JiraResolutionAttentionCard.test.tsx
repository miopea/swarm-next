import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { HeldJiraResolution } from "../api";
import JiraResolutionAttentionCard from "./JiraResolutionAttentionCard";

afterEach(cleanup);

const unanswered: HeldJiraResolution = {
  task_id: "task-10736",
  issue_key: "IS-10736",
  project_name: "IT: Support Requests",
  title: "Photo downloads are slow",
  held_since: 1_792_700_000,
  last_reply_at: null,
  handoff: "The library throttles downloads above 20 photos; batching fixes it.",
};

test("a ticket nobody answered says so before it can be resolved", () => {
  const onConfirm = vi.fn();
  const onLeaveOpen = vi.fn();
  render(<JiraResolutionAttentionCard held={[unanswered]} onConfirm={onConfirm} onLeaveOpen={onLeaveOpen} onOpenTask={vi.fn()} />);

  expect(screen.getByRole("heading", { name: "Resolve IS-10736 in Jira?" })).toBeInTheDocument();
  expect(screen.getByText("No reply has been posted on this ticket.")).toBeInTheDocument();
  expect(screen.getByLabelText("What the worker reported")).toHaveTextContent("batching fixes it");

  fireEvent.click(screen.getByRole("button", { name: "Resolve in Jira" }));
  expect(onConfirm).toHaveBeenCalledWith("task-10736");
  fireEvent.click(screen.getByRole("button", { name: "Leave open" }));
  expect(onLeaveOpen).toHaveBeenCalledWith("task-10736");
});

test("one card per ticket, and an answered one does not warn", () => {
  render(
    <JiraResolutionAttentionCard
      held={[unanswered, { ...unanswered, task_id: "task-10688", issue_key: "IS-10688", last_reply_at: 1_792_700_100, handoff: null }]}
      onConfirm={vi.fn()}
      onLeaveOpen={vi.fn()}
      onOpenTask={vi.fn()}
    />,
  );

  expect(screen.getAllByRole("button", { name: "Resolve in Jira" })).toHaveLength(2);
  expect(screen.getAllByText("No reply has been posted on this ticket.")).toHaveLength(1);
});

test("nothing waiting renders nothing", () => {
  const { container } = render(<JiraResolutionAttentionCard held={[]} onConfirm={vi.fn()} onLeaveOpen={vi.fn()} onOpenTask={vi.fn()} />);
  expect(container).toBeEmptyDOMElement();
});
