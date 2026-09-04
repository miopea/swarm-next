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
  expect(screen.getByText("1 conversation default to review")).toBeInTheDocument();
  expect(screen.getByText(/saved default is still the conversation you want/)).toBeInTheDocument();
  expect(screen.queryByText(/loses whatever happened/)).not.toBeInTheDocument();
});

test("unknown histories do not manufacture an attention card", () => {
  const { container } = render(<ConversationDriftCard workers={[unknown, current]} onOpenWorker={vi.fn()} />);
  expect(container).toBeEmptyDOMElement();
});

test("mixed results only ask for review of confirmed stale defaults", () => {
  render(<ConversationDriftCard workers={[stale, unknown, current]} onOpenWorker={vi.fn()} />);
  expect(screen.getByText("Scout")).toBeInTheDocument();
  expect(screen.queryByText("ShotCraft")).not.toBeInTheDocument();
  expect(screen.getByText("1 conversation default to review")).toBeInTheDocument();
});

test("says nothing when every conversation is the newest", () => {
  const { container } = render(<ConversationDriftCard workers={[current]} onOpenWorker={vi.fn()} />);
  expect(container).toBeEmptyDOMElement();
});
