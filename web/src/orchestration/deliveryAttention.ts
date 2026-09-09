import type { HeldDelivery } from "../api";

/** Automatic start admission belongs to system status, not an operator decision. */
export function isRuntimeStartHold(held: HeldDelivery): boolean {
  return held.kind === "wake_not_admitted";
}

/** A refused delivery is evidence of a queue, not proof it needs the operator. */
export function isQueuedDeliveryObservation(held: HeldDelivery): boolean {
  return held.kind === "delivery_held_open_prompt"
    || held.kind === "delivery_held_unsent_text"
    || held.kind === "task_message_reconciliation"
    || held.kind === "delivery_held";
}
