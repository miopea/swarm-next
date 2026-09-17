import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import {
  composeTerminalSubmission,
  MAX_TERMINAL_DRAFT_LENGTH,
  CLAUDE_REWIND_PRESSES,
  MOBILE_TERMINAL_KEYS,
  MobileTerminalComposer,
} from "./MobileTerminalComposer";
import { TerminalDraftStore, terminalDraft } from "./TerminalDraft";

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.useRealTimers(); });
beforeEach(() => { terminalDraft.clear(); localStorage.clear(); sessionStorage.clear(); });

/**
 * ⚠️ THE MOBILE EVICTION JOURNEY, WITHOUT A PHONE.
 *
 * The real failure is an OS killing a backgrounded tab while a message is
 * half-written. That was treated as needing physical hardware, and it does not:
 * the OS drives `visibilitychange` and `pagehide`, the composer listens for
 * exactly those, and both are scriptable. What a real device still decides is
 * WHETHER it backgrounds the tab — not what happens when it does, which is this.
 *
 * Deliberately asserts the intermediate state too. Drafts are NOT written per
 * keystroke (that is the documented design — sessionStorage writes on every
 * character would be the bug), so a test that only checked the end state would
 * pass just as well if the draft had been persisted all along and the lifecycle
 * listeners did nothing.
 */
test("a draft survives the tab being backgrounded and evicted", () => {
  render(<MobileTerminalComposer sessionId="session-1" connectionState="connected" onInput={() => true} />);
  fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: "half-written thought" } });

  expect(sessionStorage.getItem("swarm.terminal-draft.v1")).toBeNull();

  const hidden = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
  fireEvent(document, new Event("visibilitychange"));
  hidden.mockRestore();

  // Written now, because this is the last moment the page is guaranteed to run.
  expect(sessionStorage.getItem("swarm.terminal-draft.v1")).toContain("half-written thought");

  // The tab is killed and the operator comes back: a FRESH store over the same
  // storage is what a restored tab actually constructs.
  const recovered = new TerminalDraftStore(() => sessionStorage).snapshot();
  expect(recovered.draft?.text).toBe("half-written thought");
  expect(recovered.draft?.sessionId).toBe("session-1");
});

test("pagehide persists the draft too, for the platforms that never report hidden", () => {
  // iOS has historically fired pagehide without a usable visibilitychange.
  // Listening for one and not the other loses the draft on whichever platform
  // picks the other event, and no desktop fixture would show that.
  render(<MobileTerminalComposer sessionId="session-1" connectionState="connected" onInput={() => true} />);
  fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: "sent from a train" } });

  expect(sessionStorage.getItem("swarm.terminal-draft.v1")).toBeNull();
  fireEvent(window, new Event("pagehide"));

  expect(new TerminalDraftStore(() => sessionStorage).snapshot().draft?.text).toBe("sent from a train");
});

test("a tab merely becoming visible again does not write a draft", () => {
  // `visibilitychange` fires in BOTH directions. Flushing on the visible edge
  // would write on every app-switch back, which is noise on a phone.
  render(<MobileTerminalComposer sessionId="session-1" connectionState="connected" onInput={() => true} />);
  fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: "still typing" } });

  const visible = vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  fireEvent(document, new Event("visibilitychange"));
  visible.mockRestore();

  expect(sessionStorage.getItem("swarm.terminal-draft.v1")).toBeNull();
});

test("holds terminal geometry while focus remains inside mobile controls", () => {
  const hold = vi.fn();
  // Keys collapsed, so this measures the FOCUS rule alone. They default to
  // visible, and an open keys panel holds geometry in its own right.
  render(<MobileTerminalComposer connectionState="connected" onInput={() => true} onGeometryHold={hold} keysExpanded={false} />);
  const draft = screen.getByLabelText(/Message worker/);
  const send = screen.getByRole("button", { name: "Send" });
  fireEvent.focus(draft);
  expect(hold).toHaveBeenLastCalledWith(true);
  fireEvent.blur(draft, { relatedTarget: send });
  expect(hold).not.toHaveBeenCalledWith(false);
  fireEvent.blur(send, { relatedTarget: null });
  expect(hold).toHaveBeenLastCalledWith(false);
});

/**
 * ⚠️ OPENING THE KEYS MUST NOT RESHAPE THE TERMINAL.
 *
 * Operator, 2026-09-12: "When I toggle to show the keys, it redraws, but
 * redraws broken and I don't see the opening text." Showing the keys made the
 * terminal shorter, the grid re-fitted, and a multi-part AskUser batch already
 * drawn for the taller shape no longer fitted — its opening lines scrolled away
 * and its option descriptions ran into the next question.
 *
 * Holding geometry keeps the row count the terminal already had, so nothing
 * reflows. The keys cover part of the view, which they always did; they simply
 * stop rewriting the terminal to do it.
 */
test("an open keys panel holds terminal geometry, and closing it releases", () => {
  const hold = vi.fn();
  const view = render(<MobileTerminalComposer connectionState="connected" onInput={() => true} onGeometryHold={hold} keysExpanded={false} />);
  hold.mockClear();

  view.rerender(<MobileTerminalComposer connectionState="connected" onInput={() => true} onGeometryHold={hold} keysExpanded />);
  expect(hold).toHaveBeenLastCalledWith(true);

  view.rerender(<MobileTerminalComposer connectionState="connected" onInput={() => true} onGeometryHold={hold} keysExpanded={false} />);
  expect(hold).toHaveBeenLastCalledWith(false);
});

/** Focus leaving must NOT release while the keys are still covering the view. */
test("blurring out of the composer keeps the hold while the keys are open", () => {
  const hold = vi.fn();
  render(<MobileTerminalComposer connectionState="connected" onInput={() => true} onGeometryHold={hold} keysExpanded />);
  const draft = screen.getByLabelText(/Message worker/);
  fireEvent.focus(draft);
  hold.mockClear();
  fireEvent.blur(draft, { relatedTarget: null });
  expect(hold).not.toHaveBeenCalledWith(false);
});

test("a bound draft survives remount and cannot move into another session", () => {
  const input = vi.fn(() => true);
  const view = render(<MobileTerminalComposer sessionId="a" connectionState="connected" onInput={input} />);
  fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: "Keep this thought" } });
  view.unmount();
  const other = render(<MobileTerminalComposer sessionId="b" connectionState="connected" onInput={input} />);
  expect(screen.getByLabelText(/Message worker/)).toHaveValue("");
  expect(screen.getByLabelText(/Message worker/)).toHaveAttribute("readonly");
  expect(screen.getByText(/An unsent draft belongs/)).toBeInTheDocument();
  other.unmount();
  render(<MobileTerminalComposer sessionId="a" connectionState="connected" onInput={input} />);
  expect(screen.getByLabelText(/Message worker/)).toHaveValue("Keep this thought");
  expect(input).not.toHaveBeenCalled();
});

test("remount between paste and Enter keeps uncertain text and never replays", async () => {
  vi.useFakeTimers();
  const input = vi.fn(() => true);
  const view = render(<MobileTerminalComposer sessionId="a" connectionState="connected" onInput={input} />);
  fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: "Possibly pasted" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  view.unmount();
  render(<MobileTerminalComposer sessionId="a" connectionState="connected" onInput={input} />);
  await act(async () => { await vi.advanceTimersByTimeAsync(100); });
  expect(input).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  expect(screen.getByLabelText(/Message worker/)).toHaveValue("Possibly pasted");
  fireEvent.click(screen.getByRole("button", { name: "I checked; allow editing or resending" }));
  expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
  expect(input).toHaveBeenCalledTimes(1);
});

test("source recording never waits before Enter and is aborted on disposal", async () => {
  vi.useFakeTimers();
  const onInput = vi.fn(() => true);
  const record = vi.fn((_text: string, _signal: AbortSignal) => new Promise<void>(() => {}));
  const view = render(<MobileTerminalComposer connectionState="connected" onInput={onInput} onRecordSubmission={record} />);
  fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: " Exact 🐝 " } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(record.mock.calls[0][0]).toBe(" Exact 🐝 ");
  expect(onInput).toHaveBeenCalledTimes(1);
  await act(async () => { vi.advanceTimersByTime(75); });
  expect(onInput.mock.calls[1]).toEqual(["\r"]);
  expect(screen.getByLabelText(/Message worker/)).toHaveValue("");
  view.unmount();
  expect(record.mock.calls[0][1].aborted).toBe(true);
});

test("source recording has a four-request cap and timeout without replaying terminal input", async () => {
  vi.useFakeTimers();
  const onInput = vi.fn(() => true);
  const record = vi.fn((_text: string, _signal: AbortSignal) => new Promise<void>(() => {}));
  render(<MobileTerminalComposer connectionState="connected" onInput={onInput} onRecordSubmission={record} />);
  for (let index = 0; index < 5; index++) {
    fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: `Message ${index}` } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await act(async () => { vi.advanceTimersByTime(75); });
  }
  expect(record).toHaveBeenCalledTimes(4);
  expect(onInput).toHaveBeenCalledTimes(10);
  expect(screen.getByText(/operator-source record could not be confirmed/)).toBeVisible();
  await act(async () => { vi.advanceTimersByTime(8_000); });
  expect(record.mock.calls.every((call) => call[1].aborted)).toBe(true);
  expect(onInput).toHaveBeenCalledTimes(10);
});

test("source failures are visible but rejected terminal text is not recorded", async () => {
  const record = vi.fn().mockRejectedValue(new Error("unavailable"));
  const view = render(<MobileTerminalComposer connectionState="connected" onInput={() => false} onRecordSubmission={record} />);
  fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: "Message" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(record).not.toHaveBeenCalled();
  view.rerender(<MobileTerminalComposer connectionState="connected" onInput={() => true} onRecordSubmission={record} />);
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByText(/operator-source record could not be confirmed/)).toBeVisible();
});

test("passive control preserves editable drafts and disables Send and terminal keys", () => {
  const onInput = vi.fn(() => true);
  const view = render(<MobileTerminalComposer connectionState="connected" inputAvailable={false} keysExpanded onInput={onInput} />);
  const draft = screen.getByRole("textbox", { name: "Message worker" });
  fireEvent.change(draft, { target: { value: "keep my thought" } });
  expect(draft).toHaveAccessibleName("Message worker");
  expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Arrow up" })).toBeDisabled();
  expect(onInput).not.toHaveBeenCalled();
  view.rerender(<MobileTerminalComposer connectionState="connected" inputAvailable keysExpanded onInput={onInput} />);
  expect(draft).toHaveValue("keep my thought");
  expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
});

test("sends slash commands as bracketed paste before a separated Enter frame", async () => {
  const onInput = vi.fn<(text: string) => boolean>(() => true);
  render(<MobileTerminalComposer connectionState="connected" onInput={onInput} />);

  fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: "/status" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));

  expect(onInput.mock.calls.map(([value]) => value)).toEqual(["\u001b[200~/status\u001b[201~"]);
  await waitFor(() => expect(onInput.mock.calls.map(([value]) => value)).toEqual(["\u001b[200~/status\u001b[201~", MOBILE_TERMINAL_KEYS.enter]));
  expect(screen.getByLabelText(/Message worker/)).toHaveValue("");
});

test("terminal keys cannot interleave with a pending composer submission", async () => {
  vi.useFakeTimers();
  const onInput = vi.fn(() => true);
  render(<MobileTerminalComposer connectionState="connected" keysExpanded onInput={onInput} />);
  fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: "one message" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  // No "Enter": a blank Send is Enter now, and the panel no longer carries one.
  const keys = ["Esc", "Tab", "Ctrl+C", "Cycle mode", "Arrow up", "Arrow down", "Arrow left", "Arrow right"];
  for (const name of keys) fireEvent.click(screen.getByRole("button", { name }));
  expect(onInput.mock.calls).toEqual([["\u001b[200~one message\u001b[201~"]]);
  for (const name of keys) expect(screen.getByRole("button", { name })).toBeDisabled();
  await act(async () => { await vi.advanceTimersByTimeAsync(75); });
  expect(onInput.mock.calls).toEqual([["\u001b[200~one message\u001b[201~"], ["\r"]]);
  for (const name of keys) expect(screen.getByRole("button", { name })).toBeEnabled();
  fireEvent.click(screen.getByRole("button", { name: "Esc" }));
  expect(onInput).toHaveBeenCalledTimes(3);
});

test("refused input preserves the draft instead of silently clearing it", () => {
  const onInput = vi.fn(() => false);
  render(<MobileTerminalComposer connectionState="connected" onInput={onInput} />);
  fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: "keep this" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(screen.getByLabelText(/Message worker/)).toHaveValue("keep this");
  expect(screen.getByText(/did not accept your text/)).toBeInTheDocument();
  expect(onInput).toHaveBeenCalledTimes(1);
});

test("a failed attachment keeps the draft and blocks Send until resolved or removed", () => {
  const onInput = vi.fn(() => true);
  const view = render(<MobileTerminalComposer connectionState="connected" onInput={onInput} attachmentState="error" />);
  fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: "see the image" } });
  expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  view.rerender(<MobileTerminalComposer connectionState="connected" onInput={onInput} attachmentState="idle" />);
  expect(screen.getByLabelText(/Message worker/)).toHaveValue("see the image");
  expect(screen.getByRole("button", { name: "Send" })).not.toBeDisabled();
  expect(onInput).not.toHaveBeenCalled();
});

test("disconnect cancels a pending Enter and never replays it on reconnect", async () => {
  vi.useFakeTimers();
  const onInput = vi.fn(() => true);
  const view = render(<MobileTerminalComposer connectionState="connected" onInput={onInput} />);
  fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: "keep this" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  view.rerender(<MobileTerminalComposer connectionState="disconnected" onInput={onInput} />);
  view.rerender(<MobileTerminalComposer connectionState="connected" onInput={onInput} />);
  await act(async () => { await vi.advanceTimersByTimeAsync(100); });
  expect(onInput).toHaveBeenCalledTimes(1);
  expect(screen.getByLabelText(/Message worker/)).toHaveValue("keep this");
  expect(screen.getByText(/inspect it before sending again/)).toBeInTheDocument();
});

test("unmount cancels delayed submission and upload-in-progress blocks Send", async () => {
  vi.useFakeTimers();
  const onInput = vi.fn(() => true);
  const view = render(<MobileTerminalComposer connectionState="connected" onInput={onInput} attachmentState="uploading" />);
  fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: "wait for image" } });
  expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  view.rerender(<MobileTerminalComposer connectionState="connected" onInput={onInput} attachmentState="ready" />);
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  view.unmount();
  await act(async () => { await vi.advanceTimersByTimeAsync(100); });
  expect(onInput).toHaveBeenCalledTimes(1);
});

test("uses bracketed paste for multiline dictation before a separate Enter frame", () => {
  expect(composeTerminalSubmission("first\r\nsecond")).toEqual([
    "\u001b[200~first\nsecond\u001b[201~",
    MOBILE_TERMINAL_KEYS.enter,
  ]);
});

test("bounds drafts kept in the browser view", () => {
  render(<MobileTerminalComposer connectionState="connected" onInput={vi.fn()} />);
  const input = screen.getByLabelText(/Message worker/);

  fireEvent.change(input, { target: { value: "x".repeat(MAX_TERMINAL_DRAFT_LENGTH + 100) } });

  expect(input).toHaveValue("x".repeat(MAX_TERMINAL_DRAFT_LENGTH));
});

test("sends mobile navigation and Claude mode controls as terminal key sequences", () => {
  const onInput = vi.fn();
  render(<MobileTerminalComposer connectionState="connected" onInput={onInput} />);

  fireEvent.click(screen.getByRole("button", { name: "Arrow up" }));
  fireEvent.click(screen.getByRole("button", { name: "Arrow left" }));
  fireEvent.click(screen.getByRole("button", { name: "Arrow down" }));
  fireEvent.click(screen.getByRole("button", { name: "Arrow right" }));
  fireEvent.click(screen.getByRole("button", { name: "Esc" }));
  fireEvent.click(screen.getByRole("button", { name: "Tab" }));
  fireEvent.click(screen.getByRole("button", { name: "Ctrl+C" }));
  fireEvent.click(screen.getByRole("button", { name: "Cycle mode" }));

  expect(onInput.mock.calls.map(([value]) => value)).toEqual([
    MOBILE_TERMINAL_KEYS.up,
    MOBILE_TERMINAL_KEYS.left,
    MOBILE_TERMINAL_KEYS.down,
    MOBILE_TERMINAL_KEYS.right,
    MOBILE_TERMINAL_KEYS.escape,
    MOBILE_TERMINAL_KEYS.tab,
    MOBILE_TERMINAL_KEYS.interrupt,
    MOBILE_TERMINAL_KEYS.modeCycle,
  ]);
});

test("remembers when the operator collapses the mobile key pad", () => {
  const first = render(<MobileTerminalComposer connectionState="connected" onInput={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Hide extra keys" }));
  expect(screen.queryByRole("button", { name: "Ctrl+C" })).not.toBeInTheDocument();
  // The arrows are NOT behind this toggle any more, and that is the point of
  // the change: collapsing the panel must not take them away.
  expect(screen.getByRole("button", { name: "Arrow up" })).toBeVisible();
  first.unmount();

  render(<MobileTerminalComposer connectionState="connected" onInput={vi.fn()} />);
  expect(screen.getByRole("button", { name: "Show extra keys" })).toHaveAttribute("aria-expanded", "false");
});

test("reports controlled key visibility for the durable mobile profile", () => {
  const onKeysExpandedChange = vi.fn();
  render(
    <MobileTerminalComposer
      connectionState="connected"
      keysExpanded={false}
      onInput={vi.fn()}
      onKeysExpandedChange={onKeysExpandedChange}
    />,
  );

  fireEvent.click(screen.getByRole("button", { name: "Show extra keys" }));

  expect(onKeysExpandedChange).toHaveBeenCalledWith(true);
});

test("blocked preference storage cannot prevent opening or toggling terminal keys", () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new DOMException("Blocked", "SecurityError"); });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new DOMException("Full", "QuotaExceededError"); });
  const input = vi.fn(() => true);
  render(<MobileTerminalComposer connectionState="connected" onInput={input} />);
  fireEvent.click(screen.getByRole("button", { name: "Hide extra keys" }));
  expect(screen.queryByRole("button", { name: "Ctrl+C" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Show extra keys" }));
  fireEvent.click(screen.getByRole("button", { name: "Ctrl+C" }));
  expect(input).toHaveBeenCalledExactlyOnceWith(MOBILE_TERMINAL_KEYS.interrupt);
});

test("reconnect explains the held draft and never submits it automatically", () => {
  const input = vi.fn(() => true);
  const view = render(<MobileTerminalComposer connectionState="connecting" onInput={input} />);
  fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: "Keep this exact draft" } });
  expect(screen.getByText(/Connecting to the terminal. Your draft stays here/)).toBeVisible();
  expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  view.rerender(<MobileTerminalComposer connectionState="disconnected" onInput={input} />);
  expect(screen.getByText(/The terminal is not connected/)).toBeVisible();
  view.rerender(<MobileTerminalComposer connectionState="connected" onInput={input} />);
  expect(screen.queryByText(/The terminal is not connected/)).not.toBeInTheDocument();
  expect(screen.getByLabelText(/Message worker/)).toHaveValue("Keep this exact draft");
  expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
  expect(input).not.toHaveBeenCalled();
});

test("retains the draft and blocks controls while disconnected", () => {
  const onInput = vi.fn();
  render(<MobileTerminalComposer connectionState="disconnected" onInput={onInput} />);

  fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: "keep me" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));

  expect(onInput).not.toHaveBeenCalled();
  expect(screen.getByLabelText(/Message worker/)).toHaveValue("keep me");
  expect(screen.getByRole("button", { name: "Cycle mode" })).toBeDisabled();
  // ADD FILE IS NOT ONE OF THE BLOCKED CONTROLS, and this assertion used to
  // claim it was — passing only because this render supplies no onAttachment,
  // so the button was disabled for a reason that has nothing to do with the
  // connection. The claim and the cause were different things.
  expect(screen.getByRole("button", { name: "Add file" })).toBeDisabled();
});

/**
 * Uploading never needed the socket, and refusing to try is what the operator
 * felt as "works about half the time".
 *
 * Opening a phone's file picker BACKGROUNDS this tab, which drops the terminal
 * socket by design. The old guard then discarded the chosen file in silence, so
 * whether an attachment survived depended on whether the reconnect beat the
 * operator's thumb.
 */
test("a file chosen while the socket is down is still handed on, not dropped", async () => {
  const onAttachment = vi.fn().mockResolvedValue(undefined);
  const { container } = render(
    <MobileTerminalComposer connectionState="disconnected" onInput={vi.fn()} onAttachment={onAttachment} />,
  );
  const image = new File([new Uint8Array([1, 2, 3])], "screen.png", { type: "image/png" });

  expect(screen.getByRole("button", { name: "Add file" })).toBeEnabled();
  fireEvent.change(container.querySelector<HTMLInputElement>('input[type="file"]')!, {
    target: { files: [image] },
  });

  await vi.waitFor(() => expect(onAttachment).toHaveBeenCalledWith(image));
});

test("the button says the file is waiting on the connection rather than nothing", () => {
  render(
    <MobileTerminalComposer
      connectionState="disconnected"
      onInput={vi.fn()}
      onAttachment={vi.fn()}
      attachmentState="waiting"
    />,
  );
  expect(screen.getByRole("button", { name: "Waiting…" })).toBeTruthy();
});

test("offers a first-class mobile image picker without submitting the draft", async () => {
  const onAttachment = vi.fn().mockResolvedValue(undefined);
  const { container } = render(<MobileTerminalComposer connectionState="connected" onInput={vi.fn()} onAttachment={onAttachment} />);
  const image = new File([new Uint8Array([1, 2, 3])], "screen.png", { type: "image/png" });
  const input = container.querySelector<HTMLInputElement>('input[type="file"]');

  fireEvent.change(input!, { target: { files: [image] } });

  await vi.waitFor(() => expect(onAttachment).toHaveBeenCalledWith(image));
  expect(screen.getByRole("button", { name: "Add file" })).toBeEnabled();
});

test("offers a way to rebuild a terminal that has gone wrong", () => {
  // "We need to add that refresh button to clean out the terminal when things
  // get weird... Because right now I can't scroll in this worker."
  //
  // The desktop header has always carried this. A phone — where the view is
  // most likely to end up wrong, and where there is no other way to reach it —
  // had nothing. It repairs this screen's view and sends the worker nothing,
  // which is why it sits apart from the keys that do.
  const onRefresh = vi.fn();
  const onInput = vi.fn();
  render(
    <MobileTerminalComposer
      connectionState="connected"
      onInput={onInput}
      keysExpanded
      onRefresh={onRefresh}
    />,
  );

  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

  expect(onRefresh).toHaveBeenCalledOnce();
  expect(onInput).not.toHaveBeenCalled();
});

test("says nothing about refreshing when there is no way to do it", () => {
  render(<MobileTerminalComposer connectionState="connected" onInput={vi.fn()} keysExpanded />);

  expect(screen.queryByRole("button", { name: "Refresh" })).not.toBeInTheDocument();
});

test("Redraw survives with the keys panel closed", () => {
  // It used to live INSIDE the keys panel, so the one control that rescues a
  // broken view vanished whenever that panel was shut — on the same broken
  // screen. The operator asked for it beside Add file.
  const onRefresh = vi.fn();
  render(
    <MobileTerminalComposer
      connectionState="connected"
      onInput={vi.fn()}
      onRefresh={onRefresh}
    />,
  );

  // Shut the keys panel; the keys go, Refresh stays.
  fireEvent.click(screen.getByRole("button", { name: "Hide extra keys" }));
  expect(screen.queryByRole("button", { name: "Ctrl+C" })).toBeNull();

  const refresh = screen.getByRole("button", { name: "Refresh" });
  expect(refresh.closest(".terminal-key-actions")).toBeNull();
  fireEvent.click(refresh);
  expect(onRefresh).toHaveBeenCalledTimes(1);
});

/**
 * THE LAST SILENT BRANCH.
 *
 * After two fixes the operator still reported "nothing at all" on the failures:
 * no notice, no error, no change. Every path that reaches the handler with a
 * file now reports something, so silence meant the handler was reached WITHOUT
 * one — a change event carrying an empty list — or was never reached at all.
 * Both produced exactly nothing, which is unusable as a report.
 */
test("a picker that comes back with no file says so instead of nothing", () => {
  const onAttachment = vi.fn();
  const { container } = render(
    <MobileTerminalComposer connectionState="connected" onInput={vi.fn()} onAttachment={onAttachment} />,
  );

  fireEvent.change(container.querySelector<HTMLInputElement>('input[type="file"]')!, {
    target: { files: [] },
  });

  expect(screen.getByText(/No file arrived from the picker/)).toBeTruthy();
  expect(onAttachment).not.toHaveBeenCalled();
});

test("the notice clears when a file does arrive", async () => {
  const onAttachment = vi.fn().mockResolvedValue(undefined);
  const { container } = render(
    <MobileTerminalComposer connectionState="connected" onInput={vi.fn()} onAttachment={onAttachment} />,
  );
  const input = container.querySelector<HTMLInputElement>('input[type="file"]')!;

  fireEvent.change(input, { target: { files: [] } });
  expect(screen.getByText(/No file arrived/)).toBeTruthy();

  const image = new File([new Uint8Array([1, 2, 3])], "screen.png", { type: "image/png" });
  fireEvent.change(input, { target: { files: [image] } });

  await vi.waitFor(() => expect(onAttachment).toHaveBeenCalledWith(image));
  expect(screen.queryByText(/No file arrived/)).toBeNull();
});

test("an interrupted picker survives page recreation without retaining file data", () => {
  const view = render(<MobileTerminalComposer connectionState="connected" onInput={vi.fn()} onAttachment={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Add file" }));
  const stored = sessionStorage.getItem("swarm.mobile-picker.pending.v1");
  expect(stored).toMatch(/^\d+$/);
  view.unmount();
  render(<MobileTerminalComposer connectionState="connected" onInput={vi.fn()} onAttachment={vi.fn()} />);
  expect(screen.getByText(/No file arrived from the picker/)).toBeInTheDocument();
  expect(sessionStorage.getItem("swarm.mobile-picker.pending.v1")).toBeNull();
});

test("cancelling the native picker is quiet and clears its pending marker", () => {
  const view = render(<MobileTerminalComposer connectionState="connected" onInput={vi.fn()} onAttachment={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Add file" }));
  fireEvent(view.container.querySelector('input[type="file"]')!, new Event("cancel"));
  expect(sessionStorage.getItem("swarm.mobile-picker.pending.v1")).toBeNull();
  expect(screen.queryByText(/No file arrived/)).not.toBeInTheDocument();
});

test("picker return timeout is owned and cancelled on unmount", async () => {
  vi.useFakeTimers();
  const schedule = vi.spyOn(window, "setTimeout");
  const cancel = vi.spyOn(window, "clearTimeout");
  const view = render(<MobileTerminalComposer connectionState="connected" onInput={vi.fn()} onAttachment={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Add file" }));
  fireEvent(document, new Event("visibilitychange"));
  const index = schedule.mock.calls.findIndex((call) => call[1] === 1_500);
  expect(index).toBeGreaterThanOrEqual(0);
  const handle = schedule.mock.results[index].value;
  view.unmount();
  expect(cancel).toHaveBeenCalledWith(handle);
});

test("a blank Send is Enter, and never a paste or a source record", () => {
  const onInput = vi.fn(() => true);
  const onRecordSubmission = vi.fn();
  render(<MobileTerminalComposer sessionId="a" connectionState="connected" onInput={onInput} onRecordSubmission={onRecordSubmission} />);

  const send = screen.getByRole("button", { name: "Send" });
  expect(send).toBeEnabled();
  fireEvent.click(send);

  // ONE frame, the key itself. A blank Send confirms a provider's prompt; it
  // has no text to bracket-paste and nothing to record as an operator source.
  expect(onInput).toHaveBeenCalledExactlyOnceWith(MOBILE_TERMINAL_KEYS.enter);
  expect(onRecordSubmission).not.toHaveBeenCalled();
  expect(screen.getByLabelText(/Message worker/)).toHaveValue("");
});

test("a blank Send stays blocked whenever a typed Send would be", () => {
  const onInput = vi.fn(() => true);
  // SUBMIT THE FORM, do not click the button. A disabled button swallows the
  // click before the handler runs, so clicking here would pass with the
  // handler's guards deleted — it would be measuring the attribute, not the
  // rule. Submitting reaches the handler the way a stray Enter or an
  // autofilled form would.
  const submitForm = () => fireEvent.submit(screen.getByRole("button", { name: /Send/ }).closest("form")!);

  const view = render(<MobileTerminalComposer connectionState="connected" inputAvailable={false} onInput={onInput} />);
  expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  submitForm();
  expect(onInput).not.toHaveBeenCalled();

  view.rerender(<MobileTerminalComposer connectionState="connected" inputAvailable attachmentState="uploading" onInput={onInput} />);
  expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  submitForm();
  expect(onInput).not.toHaveBeenCalled();

  view.rerender(<MobileTerminalComposer connectionState="connected" inputAvailable attachmentState="error" onInput={onInput} />);
  submitForm();
  expect(onInput).not.toHaveBeenCalled();

  // And the same form, once nothing blocks it, does send the key — so the
  // three assertions above are about the guards and not about the route.
  view.rerender(<MobileTerminalComposer connectionState="connected" inputAvailable onInput={onInput} />);
  submitForm();
  expect(onInput).toHaveBeenCalledExactlyOnceWith(MOBILE_TERMINAL_KEYS.enter);
});

test("the arrow keys reach the terminal with the keys panel collapsed", () => {
  const onInput = vi.fn<(text: string) => boolean>(() => true);
  render(<MobileTerminalComposer connectionState="connected" keysExpanded={false} onInput={onInput} />);

  fireEvent.click(screen.getByRole("button", { name: "Arrow left" }));
  fireEvent.click(screen.getByRole("button", { name: "Arrow up" }));
  fireEvent.click(screen.getByRole("button", { name: "Arrow down" }));
  fireEvent.click(screen.getByRole("button", { name: "Arrow right" }));

  expect(onInput.mock.calls.map(([value]) => value)).toEqual([
    MOBILE_TERMINAL_KEYS.left,
    MOBILE_TERMINAL_KEYS.up,
    MOBILE_TERMINAL_KEYS.down,
    MOBILE_TERMINAL_KEYS.right,
  ]);
});

test("the extra keys are the ones a phone cannot type, and Enter is not among them", () => {
  render(<MobileTerminalComposer connectionState="connected" keysExpanded onInput={vi.fn()} />);

  const panel = screen.getByLabelText("Terminal keys");
  expect(within(panel).getAllByRole("button").map((b) => b.textContent)).toEqual([
    "Esc", "Rewind", "Tab", "Cycle mode", "Background", "Expand", "Clear", "Ctrl+C",
  ]);
  // Operator, 2026-09-13: "We don't need enter on the extra keys menu."
  // A blank Send is Enter, and the arrows moved out to the tools row.
  expect(within(panel).queryByRole("button", { name: "Enter" })).toBeNull();
  expect(within(panel).queryByRole("button", { name: "Arrow up" })).toBeNull();
});

test("Rewind is two Escapes, and Background and Expand carry Claude's own chords", () => {
  const onInput = vi.fn<(text: string) => boolean>(() => true);
  render(<MobileTerminalComposer connectionState="connected" keysExpanded onInput={onInput} />);

  fireEvent.click(screen.getByRole("button", { name: "Rewind" }));
  // "esc twice to go up a few messages and try again" -- there is no single
  // code for it, so two frames ARE the key. One Esc only cancels.
  expect(onInput.mock.calls.map(([value]) => value))
    .toEqual(Array.from({ length: CLAUDE_REWIND_PRESSES }, () => MOBILE_TERMINAL_KEYS.escape));
  expect(CLAUDE_REWIND_PRESSES).toBeGreaterThan(1);

  onInput.mockClear();
  fireEvent.click(screen.getByRole("button", { name: "Background" }));
  fireEvent.click(screen.getByRole("button", { name: "Expand" }));
  // Read out of Claude Code 2.1.270: ctrl+b "run in background", ctrl+o "to expand".
  expect(onInput.mock.calls.map(([value]) => value)).toEqual(["\u0002", "\u000f"]);
  expect(MOBILE_TERMINAL_KEYS.background).toBe("\u0002");
  expect(MOBILE_TERMINAL_KEYS.expand).toBe("\u000f");
});

test("Clear stages /clear in the box and never sends it by itself", () => {
  const onInput = vi.fn<(text: string) => boolean>(() => true);
  render(<MobileTerminalComposer connectionState="connected" keysExpanded onInput={onInput} />);

  fireEvent.click(screen.getByRole("button", { name: "Clear" }));

  // NOTHING DOWN THE WIRE. /clear discards the worker's conversation, so the
  // button loads it and the operator commits it -- one visible, abandonable tap.
  expect(onInput).not.toHaveBeenCalled();
  expect(screen.getByLabelText(/Message worker/)).toHaveValue("/clear");

  // And Send then carries it through the ordinary bracketed-paste path.
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(onInput).toHaveBeenCalledExactlyOnceWith("\u001b[200~/clear\u001b[201~");
});

test("Clear refuses to overwrite a draft rather than losing it silently", () => {
  const onInput = vi.fn<(text: string) => boolean>(() => true);
  render(<MobileTerminalComposer connectionState="connected" keysExpanded onInput={onInput} />);
  const box = screen.getByLabelText(/Message worker/);
  fireEvent.change(box, { target: { value: "a thought I dictated" } });

  fireEvent.click(screen.getByRole("button", { name: "Clear" }));

  expect(box).toHaveValue("a thought I dictated");
  expect(onInput).not.toHaveBeenCalled();
  expect(screen.getByText(/draft is still here, so \/clear was not staged/)).toBeVisible();
});
