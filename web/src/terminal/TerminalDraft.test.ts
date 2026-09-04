import { expect, test, vi } from "vitest";
import { MAX_TERMINAL_DRAFT_LENGTH, TerminalDraftStore } from "./TerminalDraft";

function storage() {
  const values = new Map<string, string>();
  return { getItem: (key: string) => values.get(key) ?? null, setItem: vi.fn((key: string, value: string) => { values.set(key, value); }), removeItem: (key: string) => { values.delete(key); } };
}

test("one draft survives explicit flush/reload without per-keystroke storage", () => {
  const disk = storage();
  const draft = new TerminalDraftStore(() => disk);
  draft.update("session-a", "Exact 🐝\ntext");
  draft.update("session-a", "Exact 🐝\ntext again");
  expect(disk.setItem).not.toHaveBeenCalled();
  expect(draft.update("session-b", "wrong terminal")).toBe(false);
  draft.flush();
  const restored = new TerminalDraftStore(() => disk);
  expect(restored.snapshot().draft).toEqual({ sessionId: "session-a", text: "Exact 🐝\ntext again", uncertain: false });
  restored.clear();
  expect(new TerminalDraftStore(() => disk).snapshot().draft).toBeUndefined();
});

test("uncertain state persists before delivery and requires explicit clearing", () => {
  const disk = storage();
  const draft = new TerminalDraftStore(() => disk);
  draft.update("a", "message");
  draft.markUncertain("a", true);
  const restored = new TerminalDraftStore(() => disk);
  expect(restored.snapshot().draft?.uncertain).toBe(true);
  restored.markUncertain("b", false);
  expect(restored.snapshot().draft?.uncertain).toBe(true);
  restored.markUncertain("a", false);
  expect(new TerminalDraftStore(() => disk).snapshot().draft?.uncertain).toBe(false);
});

test("invalid/oversized storage is not restored and draft admission is bounded", () => {
  const disk = storage();
  disk.setItem("swarm.terminal-draft.v1", JSON.stringify({ schema: 1, sessionId: "a", text: "x".repeat(MAX_TERMINAL_DRAFT_LENGTH + 1), uncertain: false }));
  const draft = new TerminalDraftStore(() => disk);
  expect(draft.snapshot().draft).toBeUndefined();
  expect(draft.update("a", "x".repeat(MAX_TERMINAL_DRAFT_LENGTH + 1))).toBe(false);
  expect(draft.update("a", "valid")).toBe(true);
});

test("storage denial preserves the in-memory draft and is visible", () => {
  const draft = new TerminalDraftStore(() => { throw new Error("denied"); });
  draft.update("a", "keep me");
  draft.flush();
  expect(draft.snapshot()).toEqual({ storageUnavailable: true, draft: { sessionId: "a", text: "keep me", uncertain: false } });
});
