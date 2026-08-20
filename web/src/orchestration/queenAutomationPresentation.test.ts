import { expect, test } from "vitest";

import type { QueenAutomationStatus } from "../api";
import { queenAutomationNeedsAttention } from "./queenAutomationPresentation";

const idle: QueenAutomationStatus = {
  enabled: false,
  state: "idle",
  run_id: null,
  trigger: null,
  actionable_count: 0,
  attempts: 0,
  requested_at: null,
  delivered_at: null,
  finished_at: null,
  outcome: null,
  waiting_reason: null,
};

test("only treats interrupted or operator-blocked Queen reviews as attention", () => {
  expect(queenAutomationNeedsAttention(undefined)).toBe(false);
  expect(queenAutomationNeedsAttention(idle)).toBe(false);
  expect(queenAutomationNeedsAttention({ ...idle, state: "running" })).toBe(false);
  expect(queenAutomationNeedsAttention({ ...idle, state: "completed", outcome: "completed" })).toBe(false);
  expect(queenAutomationNeedsAttention({ ...idle, state: "completed", outcome: "no_action" })).toBe(false);
  expect(queenAutomationNeedsAttention({ ...idle, state: "uncertain" })).toBe(true);

  // "Queen needs you" is a claim about something the operator can act on, so it
  // only holds while one of her requests is actually pending. Without that the
  // control room said she had "filed a request and stopped" when she had filed
  // nothing, on every run, with nothing behind it when the operator opened her.
  const blocked = { ...idle, state: "completed", outcome: "needs_operator" } as const;
  expect(queenAutomationNeedsAttention(blocked)).toBe(false);
  expect(queenAutomationNeedsAttention(blocked, true)).toBe(true);

  // An unconfirmed delivery is a real stall either way, and is untouched.
  expect(queenAutomationNeedsAttention({ ...idle, state: "uncertain" }, false)).toBe(true);
});
