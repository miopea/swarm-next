import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import type { TakeoverLease } from "./api";
import TakeoverNotice from "./TakeoverNotice";

const lease = (over: Partial<TakeoverLease> = {}): TakeoverLease => ({
  id: "lease-1", apiary_id: "apiary", source_hive_id: "keeper", target_hive_id: "mine",
  source_operator_id: "op-1", stewardship_id: null, reason: "Incident: operator unreachable.",
  state: "active", revision: 2, requested_at: 1, acknowledged_at: 2, expires_at: 300, ended_at: null,
  ...over,
});

/**
 * ⚠️ THE RELEASE CONDITION. ADR 0036 will not ship takeover unless the operator
 * can see it and end it from any authenticated local surface.
 */
test("an active takeover is announced and can be ended from here", async () => {
  const onReclaim = vi.fn().mockResolvedValue(undefined);
  render(<TakeoverNotice leases={[lease()]} onReclaim={onReclaim} />);

  expect(screen.getByRole("alert")).toHaveTextContent("Someone else is controlling this Hive");
  expect(screen.getByRole("alert")).toHaveTextContent("Incident: operator unreachable.");

  // A reason is required: the audit can answer "why was this taken back" only
  // because the operator is made to say.
  const button = screen.getByRole("button", { name: "Take back control" });
  expect(button).toBeDisabled();
  fireEvent.change(screen.getByRole("textbox", { name: /taking it back/ }), {
    target: { value: "Mid-deploy; taking my machine back" },
  });
  expect(button).toBeEnabled();
  fireEvent.click(button);
  expect(onReclaim).toHaveBeenCalledWith("lease-1", "Mid-deploy; taking my machine back");
});

/** A request that has not been acknowledged is already shown, so the operator
 * learns before anyone is typing rather than after. */
test("a requested takeover is announced before it becomes active", () => {
  render(<TakeoverNotice leases={[lease({ state: "requested", acknowledged_at: null })]} onReclaim={vi.fn()} />);
  expect(screen.getByRole("alert")).toHaveTextContent("Someone is asking to control this Hive");
});

test("nothing is claimed when no takeover is open, or the body is unshaped", () => {
  const view = render(<TakeoverNotice leases={[]} onReclaim={vi.fn()} />);
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  view.rerender(<TakeoverNotice leases={[lease({ state: "reclaimed" })]} onReclaim={vi.fn()} />);
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  view.rerender(<TakeoverNotice leases={{} as unknown as TakeoverLease[]} onReclaim={vi.fn()} />);
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  view.rerender(<TakeoverNotice leases={[null as unknown as TakeoverLease]} onReclaim={vi.fn()} />);
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});
