import { render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import type { ApiaryWatch } from "./api";
import WatchedByNotice from "./WatchedByNotice";

const watch = (over: Partial<ApiaryWatch> = {}): ApiaryWatch => ({
  id: "watch-1", apiary_id: "apiary", watcher_operator_id: "keeper-1", target_hive_id: "hive-1",
  authority: "keeper", state: "active", requested_at: 0, acknowledged_at: 1, expires_at: 300,
  ended_at: null, ...over,
});

test("a live watch is named on screen and can be stopped from here", async () => {
  const onEnd = vi.fn();
  render(<WatchedByNotice watches={[watch()]} nameFor={(id) => (id === "keeper-1" ? "Cora" : undefined)} onEnd={onEnd} />);

  expect(screen.getByRole("status")).toHaveTextContent("Cora is watching this Hive");
  expect(screen.getByText("A live window. Nothing is recorded.")).toBeInTheDocument();
  screen.getByRole("button", { name: "Stop it" }).click();
  expect(onEnd).toHaveBeenCalledWith("watch-1");
});

/**
 * ⚠️ THE NOTICE MUST NOT LAG THE AUTHORIZATION. A watch that has been granted
 * but not yet confirmed is already shown, so the operator learns a window is
 * opening rather than finding out once someone is already looking.
 */
test("a watch that is still opening is already announced", () => {
  render(<WatchedByNotice watches={[watch({ state: "requested", acknowledged_at: null })]} />);
  expect(screen.getByRole("status")).toHaveTextContent("Another operator is watching this Hive");
  expect(screen.getByText(/Nothing is relayed until this Hive confirms/)).toBeInTheDocument();
});

/**
 * ⚠️ A CRASH HERE IS AN INVISIBLE WATCH, not just a broken panel. This notice is
 * mounted app-wide, so throwing on an unexpected body took the whole app down —
 * and an operator being watched with no notice rendering is the one state this
 * feature must never be in.
 */
test("an unshaped response renders nothing instead of taking the app down", () => {
  const view = render(<WatchedByNotice watches={{} as unknown as ApiaryWatch[]} />);
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
  view.rerender(<WatchedByNotice watches={[null as unknown as ApiaryWatch]} />);
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});

test("nothing is claimed when nobody is watching", () => {
  const view = render(<WatchedByNotice watches={[]} />);
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
  view.rerender(<WatchedByNotice watches={undefined} />);
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
  view.rerender(<WatchedByNotice watches={[watch({ state: "ended", ended_at: 9 })]} />);
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});

test("the watched operator is told how long the window lasts if nobody renews it", () => {
  const now = 1_000_000;
  vi.spyOn(Date, "now").mockReturnValue(now * 1000);
  render(<WatchedByNotice watches={[watch({ expires_at: now + 240 })]} />);
  expect(screen.getByRole("status")).toHaveTextContent("Ends by itself in about 4 min unless they keep watching.");
  vi.restoreAllMocks();
});

test("a watch with no expiry still renders the notice rather than a nonsense countdown", () => {
  render(<WatchedByNotice watches={[watch({ expires_at: undefined as unknown as number })]} />);
  expect(screen.getByRole("status")).toHaveTextContent("Another operator is watching this Hive");
  expect(screen.queryByText(/Ends by itself/)).not.toBeInTheDocument();
});
