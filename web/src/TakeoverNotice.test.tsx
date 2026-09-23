import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";

import type { TakeoverLease } from "./api";
import TakeoverNotice, { remaining } from "./TakeoverNotice";

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

describe("how long a takeover has left", () => {
  /** The number that answers "wait, or take it back?" — hidden until now. */
  test("an active takeover says when it ends, and why that can move", () => {
    const now = 1_000_000_000;
    expect(remaining(now / 1000 + 250, true, now)).toBe("Ends in about 5 min unless they keep typing.");
    expect(remaining(now / 1000 + 40, true, now)).toBe("Ends in about 40s unless they keep typing.");
  });

  /** A request that has not started asks nothing of the person reading it. */
  test("a pending request does not read as a prompt", () => {
    const now = 1_000_000_000;
    const text = remaining(now / 1000 + 30, false, now);
    expect(text).toContain("Starting shortly");
    expect(text).not.toMatch(/accept|approve/i);
  });

  test("a lease already past its time never shows a negative countdown", () => {
    const now = 1_000_000_000;
    expect(remaining(now / 1000 - 10, true, now)).toBe("Ends in about 0s unless they keep typing.");
  });
});
