import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { DecisionClarification } from "../api";
import DecisionClarificationPanel from "./DecisionClarificationPanel";

afterEach(cleanup);
const round: DecisionClarification = {
  id: "round-1", decision_id: "decision-1", operator_id: "operator-1",
  question: "Why?", asked_at: 100, reply: null, replied_at: null,
  replying_worker_id: null, replying_session_id: null, delivery_state: "queued",
};
const defaults = {
  requester: "Petal", pending: true, waiting: false, history: [] as DecisionClarification[],
  workerNames: new Map([["queen", "Queen"], ["petal", "Petal"]]), onReload: vi.fn(),
};

test("failed question retains the exact draft and identity for a deliberate retry", async () => {
  const onAsk = vi.fn().mockRejectedValueOnce(new Error("Connection interrupted"))
    .mockImplementation(async (id: string, question: string) => ({ ...round, id, question }));
  render(<DecisionClarificationPanel {...defaults} onAsk={onAsk} />);
  fireEvent.click(screen.getByRole("button", { name: "Ask a question" }));
  const input = screen.getByLabelText("What would you like clarified?");
  expect(input).toHaveFocus();
  fireEvent.change(input, { target: { value: "  Why this option?\n" } });
  fireEvent.click(screen.getByRole("button", { name: "Send question" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Connection interrupted");
  expect(input).toHaveValue("  Why this option?\n");
  fireEvent.click(screen.getByRole("button", { name: "Send question" }));
  await waitFor(() => expect(onAsk).toHaveBeenCalledTimes(2));
  expect(onAsk.mock.calls[1]).toEqual(onAsk.mock.calls[0]);
  expect(onAsk.mock.calls[0][1]).toBe("  Why this option?\n");
  expect(await screen.findByRole("status")).toHaveTextContent("Waiting for Petal");
  expect(screen.getByRole("status")).toHaveFocus();
  expect(screen.queryByRole("button", { name: "Ask a question" })).not.toBeInTheDocument();
});

test("closing the question composer preserves its draft and keyboard focus", () => {
  render(<DecisionClarificationPanel {...defaults} onAsk={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Ask a question" }));
  fireEvent.change(screen.getByLabelText("What would you like clarified?"), { target: { value: "Tell me more" } });
  fireEvent.click(screen.getByRole("button", { name: "Keep draft for later" }));
  expect(screen.getByRole("button", { name: "Ask a question" })).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: "Ask a question" }));
  expect(screen.getByLabelText("What would you like clarified?")).toHaveValue("Tell me more");
});

test("counts UTF-8 bytes and refuses an oversized question without truncating it", () => {
  const onAsk = vi.fn();
  render(<DecisionClarificationPanel {...defaults} onAsk={onAsk} />);
  fireEvent.click(screen.getByRole("button", { name: "Ask a question" }));
  const text = "🐝".repeat(1001);
  fireEvent.change(screen.getByLabelText("What would you like clarified?"), { target: { value: text } });
  expect(screen.getByRole("alert")).toHaveTextContent("4,000-byte limit");
  expect(screen.getByRole("button", { name: "Send question" })).toBeDisabled();
  expect(screen.getByLabelText("What would you like clarified?")).toHaveValue(text);
  expect(onAsk).not.toHaveBeenCalled();
});

test("a delayed receipt does not steal focus from another decision", async () => {
  let finish!: (value: DecisionClarification) => void;
  const onAsk = vi.fn(() => new Promise<DecisionClarification>((resolve) => { finish = resolve; }));
  render(<><button type="button">Another decision</button><DecisionClarificationPanel {...defaults} onAsk={onAsk} /></>);
  fireEvent.click(screen.getByRole("button", { name: "Ask a question" }));
  fireEvent.change(screen.getByLabelText("What would you like clarified?"), { target: { value: "Why?" } });
  fireEvent.click(screen.getByRole("button", { name: "Send question" }));
  screen.getByRole("button", { name: "Another decision" }).focus();
  finish(round);
  await screen.findByRole("status");
  expect(screen.getByRole("button", { name: "Another decision" })).toHaveFocus();
});

test("attributes a Queen reply correctly and never offers a new question on a settled decision", () => {
  render(<DecisionClarificationPanel {...defaults} pending={false} onAsk={vi.fn()}
    history={[{ ...round, reply: "Petal checked this; here is the explanation.", replied_at: 102, replying_worker_id: "queen", replying_session_id: "queen-session" }]} />);
  expect(screen.getByText("Queen replied · saved in history")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Ask a question" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /approve|resolve/i })).not.toBeInTheDocument();
});
