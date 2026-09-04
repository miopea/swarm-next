import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import ConversationDriftCard, { type WorkerConversation } from "./ConversationDriftCard";

afterEach(cleanup);

const stale: WorkerConversation = {
  worker_id: "w-1", name: "Scout",
  freshness: { state: "stale", newest_conversation: "4c59142a", pinned_last_entry: "2026-09-02T00:57:40.565Z", newest_last_entry: "2026-09-02T02:13:26.742Z" },
};
const unknown: WorkerConversation = {
  worker_id: "w-2", name: "ShotCraft",
  freshness: { state: "unknown", reason: "no Claude project directory exists for this workspace" },
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
