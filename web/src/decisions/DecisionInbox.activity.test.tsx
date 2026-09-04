import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { TaskActivityPage } from "../api";
import DecisionInbox from "./DecisionInbox";

afterEach(() => { cleanup(); vi.useRealTimers(); });

function pendingRead() {
  let resolve!: (page: TaskActivityPage) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<TaskActivityPage>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

test("activity reads coalesce, cancel on leaving, and reject late results", async () => {
  const first = pendingRead();
  const second = pendingRead();
  const read = vi.fn<(signal: AbortSignal) => Promise<TaskActivityPage>>()
    .mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  const { unmount } = render(<DecisionInbox decisions={[]} tasks={[]} workers={[]} busy={false} onResolve={vi.fn()} onFetchActivity={read} />);
  expect(read).not.toHaveBeenCalled();
  await act(async () => { fireEvent.click(screen.getByRole("tab", { name: "Activity" })); });
  await act(async () => {
    fireEvent.click(screen.getByRole("tab", { name: "Activity" }));
    fireEvent.click(screen.getByRole("tab", { name: "Activity" }));
  });
  expect(read).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("tab", { name: /Needs you/ }));
  expect(read.mock.calls[0][0].aborted).toBe(true);
  await act(async () => { fireEvent.click(screen.getByRole("tab", { name: "Activity" })); });
  expect(read).toHaveBeenCalledTimes(2);
  await act(async () => { first.resolve({ events: [], truncated: true }); });
  expect(screen.getByRole("status")).toHaveTextContent("Loading recent work");
  await act(async () => { second.resolve({ events: [], truncated: false }); });
  expect(screen.getByText("No matching activity")).toBeInTheDocument();
  expect(screen.queryByText(/Showing the latest/)).not.toBeInTheDocument();
  unmount();
});

test("activity timeout offers retry and disposal aborts the replacement read", async () => {
  vi.useFakeTimers();
  const second = pendingRead();
  const read = vi.fn<(signal: AbortSignal) => Promise<TaskActivityPage>>()
    .mockImplementationOnce((signal) => new Promise((_resolve, reject) => {
      signal.addEventListener("abort", () => reject(signal.reason), { once: true });
    })).mockReturnValueOnce(second.promise);
  const { unmount } = render(<DecisionInbox decisions={[]} tasks={[]} workers={[]} busy={false} onResolve={vi.fn()} onFetchActivity={read} />);
  await act(async () => { fireEvent.click(screen.getByRole("tab", { name: "Activity" })); });
  await act(async () => { await vi.advanceTimersByTimeAsync(8_000); });
  expect(read.mock.calls[0][0].aborted).toBe(true);
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Retry" })); });
  expect(read).toHaveBeenCalledTimes(2);
  unmount();
  expect(read.mock.calls[1][0].aborted).toBe(true);
  await act(async () => { second.reject(new DOMException("Aborted", "AbortError")); });
});
