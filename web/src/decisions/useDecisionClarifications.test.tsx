import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { DecisionClarification, DecisionRequest } from "../api";
import { useDecisionClarifications } from "./useDecisionClarifications";

afterEach(() => { cleanup(); vi.useRealTimers(); });
const decision = (id: string, replyAt: number | null = null) => ({ id, state: "pending",
  clarification: { round_count: 1, waiting_clarification_id: replyAt === null ? `round-${id}` : null,
    delivery_state: replyAt === null ? "delivered" : null, latest_reply_at: replyAt,
    next_move: replyAt === null ? "requester" : "operator" } } as DecisionRequest);
const history = (id: string): DecisionClarification[] => [{ id: `round-${id}`, decision_id: id,
  operator_id: "operator", question: "Why?", asked_at: 1, reply: "Because.", replied_at: 2,
  replying_worker_id: "worker", replying_session_id: "session", delivery_state: "delivered" }];

test("history is demand loaded and only the selected revision refreshes", async () => {
  const read = vi.fn(async (id: string) => history(id));
  const { result, rerender } = renderHook(({ decisions }) => useDecisionClarifications(decisions, read),
    { initialProps: { decisions: [decision("a"), decision("b")] } });
  expect(read).not.toHaveBeenCalled();
  act(() => result.current.reload("a"));
  await waitFor(() => expect(result.current.forDecision(decision("a")).loaded).toBe(true));
  rerender({ decisions: [decision("a"), decision("b", 2)] });
  expect(read).toHaveBeenCalledTimes(1);
  rerender({ decisions: [decision("a", 2), decision("b", 2)] });
  await waitFor(() => expect(read).toHaveBeenCalledTimes(2));
  await waitFor(() => expect(result.current.forDecision(decision("a", 2)).loaded).toBe(true));
});

test("switching the inspected decision aborts the old read and ignores late completion", async () => {
  let finish!: (value: DecisionClarification[]) => void;
  let previousSignal!: AbortSignal;
  const read = vi.fn((id: string, signal: AbortSignal) => {
    if (id === "a") { previousSignal = signal; return new Promise<DecisionClarification[]>(resolve => { finish = resolve; }); }
    return Promise.resolve(history(id));
  });
  const { result, unmount } = renderHook(() => useDecisionClarifications([decision("a"), decision("b")], read));
  act(() => result.current.reload("a"));
  act(() => result.current.reload("b"));
  expect(previousSignal.aborted).toBe(true);
  await waitFor(() => expect(result.current.forDecision(decision("b")).loaded).toBe(true));
  await act(async () => finish(history("a")));
  expect(result.current.forDecision(decision("a")).loaded).toBe(false);
  unmount();
  expect(read.mock.calls[1][1].aborted).toBe(true);
});

test("retains at most eight histories and prunes decisions removed from the inbox", async () => {
  const decisions = Array.from({ length: 9 }, (_, index) => decision(String(index)));
  const { result, rerender } = renderHook(({ items }) => useDecisionClarifications(items, async id => history(id)),
    { initialProps: { items: decisions } });
  for (const item of decisions) {
    act(() => result.current.reload(item.id));
    await waitFor(() => expect(result.current.forDecision(item).loaded).toBe(true));
  }
  expect(result.current.forDecision(decisions[0]).loaded).toBe(false);
  expect(result.current.forDecision(decisions[1]).loaded).toBe(true);
  rerender({ items: [decisions[8]] });
  expect(result.current.forDecision(decisions[1]).loaded).toBe(false);
});

test("a failed read is explicit and retries only on request", async () => {
  const read = vi.fn().mockRejectedValueOnce(new Error("Connection lost")).mockResolvedValue(history("a"));
  const { result } = renderHook(() => useDecisionClarifications([decision("a")], read));
  act(() => result.current.reload("a"));
  await waitFor(() => expect(result.current.forDecision(decision("a")).loadError).toBe("Connection lost"));
  expect(read).toHaveBeenCalledTimes(1);
  act(() => result.current.reload("a"));
  await waitFor(() => expect(result.current.forDecision(decision("a")).loaded).toBe(true));
});

test("a hung read has a bounded deadline without automatic replay", async () => {
  vi.useFakeTimers();
  let signal!: AbortSignal;
  const read = vi.fn((_id: string, current: AbortSignal) => { signal = current; return new Promise<DecisionClarification[]>(() => {}); });
  const { result } = renderHook(() => useDecisionClarifications([decision("a")], read));
  act(() => result.current.reload("a"));
  act(() => vi.advanceTimersByTime(15_000));
  expect(signal.aborted).toBe(true);
  expect(result.current.forDecision(decision("a")).loading).toBe(false);
  expect(result.current.forDecision(decision("a")).loadError).toContain("too long");
  expect(read).toHaveBeenCalledTimes(1);
});
