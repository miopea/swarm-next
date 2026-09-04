import type { HeldDelivery } from "../api";

/** A refused delivery is evidence of a queue, not proof it needs the operator. */
export function isQueuedDeliveryObservation(held: HeldDelivery): boolean {
  return held.kind === "delivery_held_open_prompt"
    || held.kind === "delivery_held_unsent_text"
    || held.kind === "delivery_held";
}
