import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import ConversationRetry from "./ConversationRetry";

afterEach(cleanup);

test("a retry that finds nothing still says it ran", async () => {
  // THE WHOLE POINT. The previous button called the same refresh and, when
  // nothing had changed, changed nothing on screen — so it read as dead.
  // Operator: "The retry buttons don't seem to do anything."
  const onRetry = vi.fn(async () => undefined);
  render(<ConversationRetry onRetry={onRetry} />);

  fireEvent.click(screen.getByRole("button", { name: "Retry conversation checks" }));
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent(/Checked just now/));
  expect(onRetry).toHaveBeenCalledOnce();
  // And it does not claim anything was found or fixed.
  expect(screen.getByRole("status")).toHaveTextContent(/re-read and is unchanged/);
});

test("a failed retry says the results may be stale rather than reporting success", async () => {
  const onRetry = vi.fn(async () => { throw new Error("network"); });
  render(<ConversationRetry onRetry={onRetry} />);

  fireEvent.click(screen.getByRole("button", { name: "Retry conversation checks" }));
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent(/could not complete/));
  expect(screen.queryByText(/Checked just now/)).not.toBeInTheDocument();
});

test("the button is disabled while a check is in flight", async () => {
  let release: () => void = () => undefined;
  const onRetry = vi.fn(() => new Promise<void>((resolve) => { release = resolve; }));
  render(<ConversationRetry onRetry={onRetry} />);

  fireEvent.click(screen.getByRole("button", { name: "Retry conversation checks" }));
  const button = await screen.findByRole("button", { name: "Checking…" });
  expect(button).toBeDisabled();

  release();
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent(/Checked just now/));
  // One click, one check — the underlying refresh coalesces, so a second
  // request here would be invisible rather than harmless.
  expect(onRetry).toHaveBeenCalledOnce();
});
