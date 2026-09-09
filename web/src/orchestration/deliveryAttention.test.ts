import { expect, test } from "vitest";
import { isQueuedDeliveryObservation, isRuntimeStartHold } from "./deliveryAttention";

test("only recognized delivery holds move out of operator recovery attention", () => {
  for (const kind of ["delivery_held", "delivery_held_open_prompt", "delivery_held_unsent_text", "task_message_reconciliation", "wake_uncertain", "future_kind"]) {
    expect(isQueuedDeliveryObservation({ kind, subject: "queen-review", worker_name: null, reason: "", first_observed_at: 0, observations: 1 }))
      .toBe(kind.startsWith("delivery_held") || kind === "task_message_reconciliation");
  }
});

test("only known automatic admission holds move to runtime; uncertain wakes still need attention", () => {
  for (const kind of ["wake_not_admitted", "wake_uncertain", "future_kind", "delivery_held", "task_message_reconciliation"]) {
    expect(isRuntimeStartHold({ kind, subject: "wake:one", worker_name: "Orchard", reason: "", first_observed_at: 0, observations: 1 }))
      .toBe(kind === "wake_not_admitted");
  }
});
