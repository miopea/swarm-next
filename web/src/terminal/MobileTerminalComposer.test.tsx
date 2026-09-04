import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import {
  composeTerminalSubmission,
  MAX_TERMINAL_DRAFT_LENGTH,
  MOBILE_TERMINAL_KEYS,
  MobileTerminalComposer,
} from "./MobileTerminalComposer";
import { terminalDraft } from "./TerminalDraft";

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.useRealTimers(); });
beforeEach(() => { terminalDraft.clear(); localStorage.clear(); sessionStorage.clear(); });

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
  fireEvent.click(screen.getByRole("button", { name: "Enter" }));
  fireEvent.click(screen.getByRole("button", { name: "Esc" }));
  fireEvent.click(screen.getByRole("button", { name: "Tab" }));
  fireEvent.click(screen.getByRole("button", { name: "Ctrl+C" }));
  fireEvent.click(screen.getByRole("button", { name: "Cycle mode" }));

  expect(onInput.mock.calls.map(([value]) => value)).toEqual([
    MOBILE_TERMINAL_KEYS.up,
    MOBILE_TERMINAL_KEYS.left,
    MOBILE_TERMINAL_KEYS.down,
    MOBILE_TERMINAL_KEYS.right,
    MOBILE_TERMINAL_KEYS.enter,
    MOBILE_TERMINAL_KEYS.escape,
    MOBILE_TERMINAL_KEYS.tab,
    MOBILE_TERMINAL_KEYS.interrupt,
    MOBILE_TERMINAL_KEYS.modeCycle,
  ]);
});

test("remembers when the operator collapses the mobile key pad", () => {
  const first = render(<MobileTerminalComposer connectionState="connected" onInput={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Hide keys" }));
  expect(screen.queryByRole("button", { name: "Arrow up" })).not.toBeInTheDocument();
  first.unmount();

  render(<MobileTerminalComposer connectionState="connected" onInput={vi.fn()} />);
  expect(screen.getByRole("button", { name: "Show keys" })).toHaveAttribute("aria-expanded", "false");
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

  fireEvent.click(screen.getByRole("button", { name: "Show keys" }));

  expect(onKeysExpandedChange).toHaveBeenCalledWith(true);
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
  fireEvent.click(screen.getByRole("button", { name: "Hide keys" }));
  expect(screen.queryByRole("button", { name: "Enter" })).toBeNull();

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
