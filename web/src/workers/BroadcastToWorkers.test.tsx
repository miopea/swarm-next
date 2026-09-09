import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import BroadcastToWorkers from "./BroadcastToWorkers";

afterEach(cleanup);

/**
 * The count is the feature. A worker with no live session is excluded from
 * delivery rather than queued, so a control that answered "sent" would let the
 * operator believe everyone was told — worse than telling them by hand, because
 * then they would at least know they had stopped.
 */
test("says how many workers it could not reach", async () => {
  const onBroadcast = vi.fn().mockResolvedValue({ reached: 13, skipped: 32 });
  render(<BroadcastToWorkers open onClose={vi.fn()} onBroadcast={onBroadcast} />);

  fireEvent.change(screen.getByLabelText("What to tell every running worker"), {
    target: { value: "reloading in five minutes" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send to every worker" }));

  await waitFor(() => {
    const status = screen.getByRole("status");
    expect(status.textContent).toContain("13");
    expect(status.textContent).toContain("32 had no live session");
  });
  expect(onBroadcast).toHaveBeenCalledWith("reloading in five minutes");
});

test("does not send an empty broadcast", () => {
  const onBroadcast = vi.fn();
  render(<BroadcastToWorkers open onClose={vi.fn()} onBroadcast={onBroadcast} />);
  const send = screen.getByRole("button", { name: "Send to every worker" });
  expect(send).toBeDisabled();
  fireEvent.click(send);
  expect(onBroadcast).not.toHaveBeenCalled();
});

test("closing a draft asks once, Escape keeps it, and only explicit discard clears it", () => {
  const onClose = vi.fn();
  const onBroadcast = vi.fn();
  render(<BroadcastToWorkers open onClose={onClose} onBroadcast={onBroadcast} />);
  const field = screen.getByRole("textbox");
  expect(screen.getByRole("dialog")).toHaveClass("dialog", "broadcast-modal");
  expect(screen.getByRole("presentation")).toHaveClass("dialog-backdrop");
  expect(field).toHaveFocus();
  fireEvent.change(field, { target: { value: "Fictional draft" } });
  fireEvent.keyDown(field, { key: "Escape" });
  expect(screen.getByRole("alertdialog", { name: "Discard this broadcast?" })).toBeInTheDocument();
  expect(onClose).not.toHaveBeenCalled();
  fireEvent.keyDown(screen.getByRole("button", { name: "Keep editing" }), { key: "Escape" });
  expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  expect(field).toHaveValue("Fictional draft");
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  fireEvent.click(screen.getByRole("button", { name: "Discard message" }));
  expect(onClose).toHaveBeenCalledOnce();
  expect(field).toHaveValue("");
  expect(onBroadcast).not.toHaveBeenCalled();
});

test("a pending broadcast cannot lose its draft or be submitted twice and failures remain editable", async () => {
  let reject!: (reason: Error) => void;
  const onBroadcast = vi.fn(() => new Promise<{ reached: number; skipped: number }>((_resolve, fail) => { reject = fail; }));
  const onClose = vi.fn();
  render(<BroadcastToWorkers open onClose={onClose} onBroadcast={onBroadcast} />);
  const field = screen.getByRole("textbox");
  fireEvent.change(field, { target: { value: "Fictional draft" } });
  fireEvent.click(screen.getByRole("button", { name: "Send to every worker" }));
  expect(field).toBeDisabled();
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  fireEvent.click(screen.getByRole("presentation"));
  fireEvent.click(screen.getByRole("button", { name: "Sending…" }));
  expect(onClose).not.toHaveBeenCalled();
  expect(onBroadcast).toHaveBeenCalledOnce();
  await act(async () => { reject(new Error("Fictional refusal")); });
  expect(field).toBeEnabled();
  expect(field).toHaveValue("Fictional draft");
  expect(screen.getByRole("alert")).toHaveTextContent("Fictional refusal");
});
