import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
  render(<BroadcastToWorkers onBroadcast={onBroadcast} />);

  fireEvent.click(screen.getByRole("button", { name: "Tell every worker" }));
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
  render(<BroadcastToWorkers onBroadcast={onBroadcast} />);
  fireEvent.click(screen.getByRole("button", { name: "Tell every worker" }));
  const send = screen.getByRole("button", { name: "Send to every worker" });
  expect(send).toBeDisabled();
  fireEvent.click(send);
  expect(onBroadcast).not.toHaveBeenCalled();
});
