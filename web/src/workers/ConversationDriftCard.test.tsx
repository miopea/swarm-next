import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import ConversationDriftCard, { type WorkerConversation } from "./ConversationDriftCard";

afterEach(cleanup);

const stale: WorkerConversation = {
  worker_id: "w-1", name: "Scout",
  freshness: { state: "stale", newest_conversation: "4c59142a", pinned_last_entry: "2026-09-02T00:57:40.565Z", newest_last_entry: "2026-09-02T02:13:26.742Z" },
};
// A worker that has never run in its workspace. Ordinary, and not the
// operator's — five of these were the rows they objected to.
const unknown: WorkerConversation = {
  worker_id: "w-2", name: "ShotCraft",
  freshness: {
    state: "unknown",
    cause: { kind: "never_run", fault: false },
    reason: "no Claude project directory exists for this workspace",
  },
};
// A directory that exists and cannot be read. Same "unknown" state, opposite
// verdict — this one is somebody's to fix.
const unreadable: WorkerConversation = {
  worker_id: "w-4", name: "Aria",
  freshness: {
    state: "unknown",
    cause: { kind: "directory_unreadable", fault: true },
    reason: "the Claude project directory could not be read",
  },
};
const current: WorkerConversation = { worker_id: "w-3", name: "Platform", freshness: { state: "current" } };

test("names the workers that would resume an older conversation", () => {
  render(<ConversationDriftCard workers={[stale, current]} onOpenWorker={vi.fn()} />);
  expect(screen.getByText("Scout")).toBeInTheDocument();
  expect(screen.queryByText("Platform")).toBeNull();
});

/**
 * The operator's own requirement: "We need a way to notify if we don't know."
 * A worker Swarm cannot check must appear, not be quietly counted as healthy.
 */
/**
 * ⚠️ THIS TEST ASSERTED THE OPPOSITE UNTIL THE OPERATOR SAW THE PAGE. It required
 * unknown workers to be listed here — "reported rather than assumed healthy" —
 * and that was the right instinct on the wrong surface.
 *
 * They were shown eight rows under "What needs you" and said "there is nothing I
 * can do about it". Five were unknown, and every one of those five has ZERO
 * transcripts in its workspace: they have never run there. "Swarm could not tell
 * which conversation is newest" is the ORDINARY state of a worker that has never
 * started, and there is no second thread for anyone to choose between.
 *
 * Nothing assumes them healthy. The server still reports Unknown and still
 * refuses to call it current; it simply stopped being filed as the operator's
 * move on a page that promises only their own work.
 */
test("a worker that has never run is not the operator's to act on", () => {
  const { container } = render(<ConversationDriftCard workers={[unknown, current]} onOpenWorker={vi.fn()} />);
  expect(container).toBeEmptyDOMElement();
});

test("a stale worker still brings the card back, with the unknown ones left out", () => {
  render(<ConversationDriftCard workers={[stale, unknown, current]} onOpenWorker={vi.fn()} />);
  expect(screen.getByText(/would resume an older conversation/i)).toBeInTheDocument();
  expect(screen.queryByText(/could not tell which conversation is newest/i)).not.toBeInTheDocument();
  expect(screen.queryByText("ShotCraft")).not.toBeInTheDocument();
});

test("says nothing when every conversation is the newest", () => {
  const { container } = render(<ConversationDriftCard workers={[current]} onOpenWorker={vi.fn()} />);
  expect(container).toBeEmptyDOMElement();
});

/**
 * ⚠️ THE TWO UNKNOWNS ARE THE SAME STATE AND OPPOSITE ANSWERS, and this is the
 * web half of the guard that stops them collapsing.
 *
 * Both arrive as state "unknown". A worker that has never run has no second
 * thread to choose between and is not the operator's business; a project
 * directory that exists and cannot be read is a permissions fault and is the
 * one case in this bucket where something is wrong and someone can fix it.
 *
 * Selected on the server's `fault` verdict, never on the reason sentence.
 * Matching prose would make a display string load-bearing, and rewording the
 * message would break detection with nothing failing.
 */
test("an unreadable project directory is shown; a worker that never ran is not", () => {
  render(<ConversationDriftCard workers={[unreadable, unknown, current]} onOpenWorker={vi.fn()} />);
  expect(screen.getByText("Aria")).toBeInTheDocument();
  expect(screen.queryByText("ShotCraft")).not.toBeInTheDocument();
  expect(screen.getByText(/permissions or filesystem problem/i)).toBeInTheDocument();
});

/**
 * The closing line tells the operator to pick a thread. A worker whose history
 * cannot be READ has no threads to pick between, so the instruction is false
 * for it — the same defect that put five never-run workers on this page.
 */
test("the pick-a-thread instruction appears only when there is a thread to pick", () => {
  const { container } = render(<ConversationDriftCard workers={[unreadable]} onOpenWorker={vi.fn()} />);
  expect(container.textContent).not.toMatch(/Swarm does not switch for you/i);
  cleanup();
  render(<ConversationDriftCard workers={[stale]} onOpenWorker={vi.fn()} />);
  expect(screen.getByText(/Swarm does not switch for you/i)).toBeInTheDocument();
});
