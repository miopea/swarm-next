import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";

import type { TakeoverAuditEntry } from "../api";
import TakeoverAudit from "./TakeoverAudit";

const entry = (over: Partial<TakeoverAuditEntry["lease"]> = {}, reclaim: string | null = null): TakeoverAuditEntry => ({
  lease: {
    id: "lease-1", apiary_id: "apiary", source_hive_id: "keeper-hive", target_hive_id: "hive-2",
    source_operator_id: "op-1", stewardship_id: null, reason: "Incident: operator unreachable.",
    state: "reclaimed", revision: 4, requested_at: 1, acknowledged_at: 2, expires_at: 300, ended_at: 9,
    ...over,
  },
  reclaim_reason: reclaim,
});

/**
 * ⚠️ BOTH REASONS, BECAUSE THEY ARE DIFFERENT CLAIMS. Why a takeover started,
 * and the operator's account of taking their machine back. The second is what
 * matters when the question is whether the first should have happened.
 */
test("a reclaimed takeover shows why it started and why it ended", () => {
  render(<TakeoverAudit entries={[entry({}, "Mid-deploy; taking my machine back.")]} nameFor={() => "Paul's Hive"} />);
  const history = screen.getByRole("list", { name: "Takeover history" });
  expect(history).toHaveTextContent("Paul's Hive");
  expect(history).toHaveTextContent("Incident: operator unreachable.");
  expect(history).toHaveTextContent("Reclaimed by operator");
  expect(history).toHaveTextContent("Reclaimed: Mid-deploy; taking my machine back.");
});

/** Keeper's own authority is legible, rather than showing as a missing steward. */
test("a Keeper takeover reads as Keeper, not as a blank stewardship", () => {
  render(<TakeoverAudit entries={[entry({ state: "active" })]} />);
  expect(screen.getByRole("list", { name: "Takeover history" })).toHaveTextContent("Keeper");
  expect(screen.getByText("Active now")).toBeInTheDocument();
});

test("a Steward takeover is distinguished from Keeper's", () => {
  render(<TakeoverAudit entries={[entry({ stewardship_id: "scope-1", state: "released" })]} />);
  expect(screen.getByRole("list", { name: "Takeover history" })).toHaveTextContent("Steward");
});

/**
 * An empty audit says nothing has happened, rather than looking like a panel
 * that failed to load — and an unshaped body must not crash the room.
 */
test("nothing taken over says so, and an unshaped body renders empty", () => {
  const view = render(<TakeoverAudit entries={[]} />);
  expect(screen.getByText("No Hive has been taken over.")).toBeInTheDocument();
  view.rerender(<TakeoverAudit entries={{} as unknown as TakeoverAuditEntry[]} />);
  expect(screen.getByText("No Hive has been taken over.")).toBeInTheDocument();
});
