import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import BlockedEscalationCard from "./BlockedEscalationCard";

const escalation = (title: string, seconds: number) => ({
  task_id: `id-${title}`,
  title,
  worker_name: "Platform",
  workspace: "/workspace/platform",
  blocked_for_seconds: seconds,
});

test("nothing renders when nothing has stalled", () => {
  const { container } = render(<BlockedEscalationCard escalations={[]} />);
  expect(container).toBeEmptyDOMElement();
});

test("names each stalled task and how long it has waited", () => {
  render(<BlockedEscalationCard escalations={[escalation("Backfill contacts", 18 * 3600)]} />);
  expect(screen.getByText("Backfill contacts")).toBeInTheDocument();
  expect(screen.getByText(/18 hours/)).toBeInTheDocument();
});

/** Rounded DOWN: the number is the argument for interrupting someone. */
test("never claims a longer wait than has actually elapsed", () => {
  render(<BlockedEscalationCard escalations={[escalation("Nearly two days", 47 * 3600 + 3599)]} />);
  expect(screen.getByText(/47 hours/)).toBeInTheDocument();
  expect(screen.queryByText(/2 days/)).toBeNull();
});

/**
 * Queen remains the arbitrator. The cheap way to make an escalation useful is
 * to let it act, and that is what the operator ruled out.
 */
test("offers no way to unblock", () => {
  render(<BlockedEscalationCard escalations={[escalation("Waiting", 20 * 3600)]} />);
  expect(screen.queryByRole("button", { name: /unblock|resume|activate/i })).toBeNull();
  expect(screen.getByText(/Queen moves work out of Blocked/)).toBeInTheDocument();
});

test("opening a task reports which one", () => {
  const onOpenTask = vi.fn();
  render(<BlockedEscalationCard escalations={[escalation("Pick me", 20 * 3600)]} onOpenTask={onOpenTask} />);
  fireEvent.click(screen.getByRole("button", { name: "Pick me" }));
  expect(onOpenTask).toHaveBeenCalledWith("id-Pick me");
});
