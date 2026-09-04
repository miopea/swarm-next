import { expect, test } from "vitest";
import { isQueuedDeliveryObservation } from "./deliveryAttention";

test("only recognized delivery holds move out of operator recovery attention", () => {
  for (const kind of ["delivery_held", "delivery_held_open_prompt", "delivery_held_unsent_text", "wake_uncertain", "future_kind"]) {
    expect(isQueuedDeliveryObservation({ kind, subject: "queen-review", worker_name: null, reason: "", first_observed_at: 0, observations: 1 }))
      .toBe(kind.startsWith("delivery_held"));
  }
});
