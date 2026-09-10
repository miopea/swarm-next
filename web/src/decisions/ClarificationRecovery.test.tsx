import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import type { DecisionClarification } from "../api";
import ClarificationRecovery from "./ClarificationRecovery";

const round: DecisionClarification = {
  id: "question", decision_id: "decision", operator_id: "operator", question: "Why this scope?",
  asked_at: 1, reply: null, replied_at: null, replying_worker_id: null, replying_session_id: null,
  delivery_state: "uncertain", delivery_claim_id: "claim", delivery_session_id: "session",
};

test("retry requires explicit consent and submits the exact attempt only once", async () => {
  let complete!: (saved: DecisionClarification) => void;
  const onReconcile = vi.fn(() => new Promise<DecisionClarification>(resolve => { complete = resolve; }));
  const onReload = vi.fn();
  render(<ClarificationRecovery round={round} onReconcile={onReconcile} onReload={onReload} />);
  const retry = screen.getByRole("button", { name: "Retry this question" });
  expect(retry).toBeDisabled();
  fireEvent.click(retry);
  expect(onReconcile).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(retry);
  fireEvent.click(retry);
  expect(onReconcile).toHaveBeenCalledTimes(1);
  expect(onReconcile).toHaveBeenCalledWith({ clarification_id: "question", decision_id: "decision",
    claim_id: "claim", session_id: "session", choice: "retry", acknowledged_duplicate_risk: true });
  complete({ ...round, delivery_state: "queued" });
  expect(await screen.findByRole("status")).toHaveTextContent("Recovery recorded");
  expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Refresh delivery status" }));
  expect(onReload).toHaveBeenCalledTimes(1);
});

test("failed observation stays explicit and a new claim needs fresh retry consent", async () => {
  const onReconcile = vi.fn().mockRejectedValue(new Error("The delivery attempt changed. Refresh first."));
  const props = { onReconcile, onReload: vi.fn() };
  const { rerender } = render(<ClarificationRecovery key="claim" round={round} {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "I checked: the question is there" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Refresh first");
  expect(onReconcile.mock.calls[0][0].choice).toBe("confirm_delivered");
  expect(onReconcile.mock.calls[0][0].acknowledged_duplicate_risk).toBe(false);
  fireEvent.click(screen.getByRole("checkbox"));
  rerender(<ClarificationRecovery key="new-claim" round={{ ...round, delivery_claim_id: "new-claim" }} {...props} />);
  await waitFor(() => expect(screen.getByRole("checkbox")).not.toBeChecked());
  expect(screen.getByRole("button", { name: "Retry this question" })).toBeDisabled();
});
