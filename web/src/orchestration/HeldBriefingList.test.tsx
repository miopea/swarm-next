import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { HeldBriefing } from "../api";
import HeldBriefingList, { briefingWait, holdReason } from "./HeldBriefingList";

afterEach(cleanup);

test("explains scoped prompt observations without replacing current durable holds", () => {
  const held = briefing({ reason: "awaiting_safe_delivery", last_delivery_check: "delivery_held_unsent_text" });
  expect(holdReason(held)).toBe("last delivery check found unsent text; Swarm will not change that input");
  expect(holdReason({ ...held, last_delivery_check: "delivery_held_open_prompt" })).toBe("last delivery check found a prompt waiting for an answer");
  for (const last_delivery_check of [undefined, null, "future_kind"]) {
    expect(holdReason({ ...held, last_delivery_check })).toBe("awaiting safe delivery; no task-order blocker is recorded");
  }
  expect(holdReason({ ...held, reason: "operator_in_the_terminal" })).toBe("you are in that terminal");
  expect(holdReason({ ...held, reason: "operator_decision_pending" })).toBe("waiting for your answer in Needs You");
  const view = render(<HeldBriefingList briefings={[held]} />);
  expect(screen.getByText(/last delivery check found unsent text/)).toBeVisible();
  view.rerender(<HeldBriefingList briefings={[{ ...held, last_delivery_check: null }]} />);
  expect(screen.queryByText(/last delivery check found unsent text/)).not.toBeInTheDocument();
});

test("queue age distinguishes exact evidence, migrated bounds and older API responses", () => {
  const exact = briefing({ queued_at: 100, queued_at_is_lower_bound: false });
  const legacy = briefing({ queued_at: 200, queued_at_is_lower_bound: true });
  expect(briefingWait([exact], 7300)).toBe("2.0 hours");
  expect(briefingWait([exact, legacy], 7300)).toBe("at least 2.0 hours");
  expect(briefingWait([{ ...exact, queued_at_is_lower_bound: undefined }], 7300)).toBe("at least 2.0 hours");
});

test("names and opens the recorded Active blocker without trusting old queue-order titles", () => {
  const open = vi.fn();
  const held = briefing({ reason: "worker_already_working", blocked_by: "Actual active task", blocking_task_id: "active-id" });
  const view = render(<HeldBriefingList briefings={[held]} onOpenTask={open} />);
  expect(screen.getByText(/worker has Active work: Actual active task/)).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Open blocking task" }));
  expect(open).toHaveBeenCalledExactlyOnceWith("active-id");
  view.rerender(<HeldBriefingList briefings={[{ ...held, blocking_task_id: undefined }]} onOpenTask={open} />);
  expect(screen.queryByText(/worker has Active work/)).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Open blocking task" })).not.toBeInTheDocument();
  view.rerender(<HeldBriefingList briefings={[{ ...held, reason: "operator_in_the_terminal" }]} onOpenTask={open} />);
  expect(screen.queryByRole("button", { name: "Open blocking task" })).not.toBeInTheDocument();
});

function briefing(overrides: Partial<HeldBriefing> = {}): HeldBriefing {
  return {
    task_id: "019fedfc-1c30-70e1-a5e2-9a3c94268099",
    title: "Reconcile the household roster",
    worker_id: "worker-1",
    worker_name: "Platform",
    queued_at: Math.floor(Date.now() / 1000) - 7_200,
    queued_at_is_lower_bound: false,
    reason: "waiting_its_turn",
    blocked_by: "Backfill the contact index",
    ...overrides,
  };
}

/**
 * The gap this closes: the server computed these and no file under web/src
 * read them. Two existed on the operator's Hive with no surface to find them.
 */
test("shows a queued briefing that nothing rendered before", () => {
  render(<HeldBriefingList briefings={[briefing()]} />);

  expect(screen.getByText("One briefing is queued")).toBeInTheDocument();
  expect(screen.getByText("Reconcile the household roster")).toBeInTheDocument();
});

/**
 * "Waiting its turn" is unfalsifiable on its own — sixteen briefings reported
 * it at once on 2026-08-24 and named nothing to go and look at.
 */
test("names the task a briefing is queued behind", () => {
  render(<HeldBriefingList briefings={[briefing()]} />);
  expect(screen.getByText(/behind Backfill the contact index/)).toBeInTheDocument();
});

test("explains experimental Night Watch holds instead of blaming earlier work", () => {
  render(<HeldBriefingList briefings={[briefing({ reason: "experimental_during_night_watch", blocked_by: null })]} />);
  expect(screen.getByText(/experimental provider — queued until Night Watch ends/)).toBeInTheDocument();
  expect(screen.queryByText(/behind earlier work/)).not.toBeInTheDocument();
});

test("names the pending decision hold without blaming worker inactivity", () => {
  render(<HeldBriefingList briefings={[briefing({ reason: "operator_decision_pending", blocked_by: null })]} />);
  expect(screen.getByText(/waiting for your answer in Needs You/)).toBeVisible();
  expect(screen.queryByText(/the worker is on something else/)).not.toBeInTheDocument();
});

test("does not invent earlier work when an older API names no blocker", () => {
  render(<HeldBriefingList briefings={[briefing({ blocked_by: null })]} />);
  expect(screen.getByText(/awaiting safe delivery; no earlier task is recorded/)).toBeInTheDocument();
  expect(screen.queryByText(/behind earlier work/)).not.toBeInTheDocument();
});

test("shows an unconfirmed delivery gate without inventing task order or an operator question", () => {
  render(<HeldBriefingList briefings={[briefing({ reason: "awaiting_safe_delivery", blocked_by: null })]} />);
  expect(screen.getByText(/awaiting safe delivery; no task-order blocker is recorded/)).toBeVisible();
  expect(screen.queryByRole("button", { name: "Open blocking task" })).not.toBeInTheDocument();
});

/**
 * The age is the whole point. Benign for minutes, a stalled predecessor after
 * hours, and only the operator can tell which by looking.
 */
test("says how long it has been waiting, coarsely", () => {
  render(<HeldBriefingList briefings={[briefing()]} />);
  expect(screen.getByText(/waiting 2.0 hours/)).toBeInTheDocument();
});

test("reads the operator's own terminal back to them", () => {
  render(<HeldBriefingList briefings={[briefing({ reason: "operator_in_the_terminal" })]} />);
  expect(screen.getByText(/you are in that terminal/)).toBeInTheDocument();
});

test("opens the task it names", () => {
  const onOpenTask = vi.fn();
  render(<HeldBriefingList briefings={[briefing()]} onOpenTask={onOpenTask} />);
  fireEvent.click(screen.getByRole("button", { name: "Reconcile the household roster" }));
  expect(onOpenTask).toHaveBeenCalledWith("019fedfc-1c30-70e1-a5e2-9a3c94268099");
});

/**
 * A panel about nothing is noise, and noise is how an operator learns to skip
 * the surface where it mattered. Held briefings are usually zero.
 */
test("renders nothing when none are held", () => {
  const { container } = render(<HeldBriefingList briefings={[]} />);
  expect(container).toBeEmptyDOMElement();
});

/*
 * NO TEST HOLDS THE STYLING DEFECT, and that is worth saying rather than
 * leaving a gap someone assumes is covered.
 *
 * The bug was a class applied and never defined, so the button kept this app's
 * default filled styling. jsdom applies no stylesheet at all, so nothing here
 * can see visual weight — the operator caught it by looking at a screenshot,
 * which was the only way it was catchable.
 *
 * A stylesheet lint would catch it, and it does not belong in this file: it
 * needs to read styles.css outside the browser tsconfig, which has no node
 * types. Reaching for node:fs here typechecked under an incremental `tsc -b`
 * that reused a stale cache and then broke the operator's development build.
 * Filed rather than half-built.
 */

/**
 * The operator's screenshot, 2026-08-28: seven rows each ending "BFG Watchfaces
 * · the worker is on something else · waiting 41 minutes", four identical but
 * for the title. The same fact restated four times, with the eye travelling the
 * width of the window to read it each time.
 */
test("briefings behind one worker are stated once, not once per row", () => {
  const queued = Math.floor(Date.now() / 1000);
  render(<HeldBriefingList briefings={[
    briefing({ task_id: "a", title: "Procedural texture engines", worker_name: "BFG Watchfaces", reason: "worker_already_working", queued_at: queued - 2_460 }),
    briefing({ task_id: "b", title: "Detect the target watch", worker_name: "BFG Watchfaces", reason: "worker_already_working", queued_at: queued - 2_460 }),
    briefing({ task_id: "c", title: "Complication spacing", worker_name: "BFG Watchfaces", reason: "worker_already_working", queued_at: queued - 2_460 }),
    briefing({ task_id: "d", title: "Correct a logged set's load", worker_id: "worker-2", worker_name: "Sculpt Studio", reason: "worker_already_working", queued_at: queued - 1_260 }),
  ]} />);

  // Every title still reachable — grouping must not hide work.
  for (const title of ["Procedural texture engines", "Detect the target watch", "Complication spacing", "Correct a logged set's load"]) {
    expect(screen.getByRole("button", { name: title })).toBeInTheDocument();
  }

  // But the worker and its wait are said ONCE per worker, not once per row.
  expect(screen.getAllByText(/BFG Watchfaces/)).toHaveLength(1);
  expect(screen.getAllByText(/Sculpt Studio/)).toHaveLength(1);
  // And the group says how many are behind that worker rather than repeating.
  expect(screen.getByText(/3 briefings, longest waiting/)).toBeInTheDocument();
});

/** A worker with one briefing reads as one, not as "1 briefings". */
test("a single briefing keeps its own waiting time", () => {
  const { container } = render(<HeldBriefingList briefings={[briefing({ worker_name: "Platform" })]} />);
  const heading = container.querySelector(".held-briefing-group-heading")?.textContent ?? "";
  expect(heading).toContain("Platform");
  expect(heading).toMatch(/waiting /);
  expect(heading).not.toContain("1 briefings");
});

test("different blockers stay attached to their own briefing within one owner group", () => {
  render(<HeldBriefingList briefings={[
    briefing({ task_id: "a", title: "First", blocked_by: "Schema migration" }),
    briefing({ task_id: "b", title: "Second", blocked_by: "API rollout" }),
    briefing({ task_id: "c", title: "Third", reason: "experimental_during_night_watch" }),
  ]} />);
  expect(screen.getAllByText("Platform")).toHaveLength(1);
  for (const [title, reason] of [["First", "behind Schema migration"], ["Second", "behind API rollout"], ["Third", "experimental provider — queued until Night Watch ends"]]) {
    const row = screen.getByRole("button", { name: title }).closest("li")!;
    expect(within(row).getByText(reason)).toBeVisible();
  }
  expect(screen.queryByText(/Nothing is wrong with these/)).not.toBeInTheDocument();
});

test("different worker identities with the same name do not share a blocker", () => {
  render(<HeldBriefingList briefings={[
    briefing({ task_id: "a", worker_id: "first", blocked_by: "Schema migration" }),
    briefing({ task_id: "b", worker_id: "second", blocked_by: "API rollout" }),
  ]} />);
  expect(screen.getAllByText("Platform")).toHaveLength(2);
  expect(screen.getByText(/behind Schema migration/)).toBeVisible();
  expect(screen.getByText(/behind API rollout/)).toBeVisible();
});
