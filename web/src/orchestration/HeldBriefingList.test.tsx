import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { HeldBriefing } from "../api";
import HeldBriefingList from "./HeldBriefingList";

afterEach(cleanup);

function briefing(overrides: Partial<HeldBriefing> = {}): HeldBriefing {
  return {
    task_id: "019fedfc-1c30-70e1-a5e2-9a3c94268099",
    title: "Reconcile the household roster",
    worker_id: "worker-1",
    worker_name: "Platform",
    queued_at: Math.floor(Date.now() / 1000) - 7_200,
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

test("falls back to a plain reason when nothing is named", () => {
  render(<HeldBriefingList briefings={[briefing({ blocked_by: null })]} />);
  expect(screen.getByText(/behind earlier work/)).toBeInTheDocument();
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
