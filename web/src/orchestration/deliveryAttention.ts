import type { HeldDelivery } from "../api";

/** Automatic start admission belongs to system status, not an operator decision. */
export function isRuntimeStartHold(held: HeldDelivery): boolean {
  return held.kind === "wake_not_admitted";
}

/**
 * A refused delivery is evidence of a queue, not proof it needs the operator.
 *
 * ⚠️ AN UNLISTED KIND LANDS IN NEEDS YOU. A message waiting behind its
 * recipient's other Active task got its own kind, and until it was listed here
 * those holds left Queues and raised a "Delivery paused" card that asked the
 * operator about a hold ADR 0067 says raises nothing.
 */
export function isQueuedDeliveryObservation(held: HeldDelivery): boolean {
  return held.kind === "delivery_held_open_prompt"
    || held.kind === "delivery_held_unsent_text"
    || held.kind === "task_message_reconciliation"
    || held.kind === "task_message_held_behind_active_task"
    || held.kind === "delivery_held";
}
