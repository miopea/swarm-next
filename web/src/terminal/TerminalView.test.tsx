import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

const controller = vi.hoisted(() => ({
  attach: vi.fn(),
  detach: vi.fn(),
  sendInput: vi.fn(() => true),
  stateListener: undefined as ((state: string) => void) | undefined,
  initialState: "connected",
  controlListener: undefined as ((control: string) => void) | undefined,
  subscribeControl: vi.fn((listener: (control: string) => void) => {
    controller.controlListener = listener;
    listener("owned");
    return { dispose: vi.fn() };
  }),
  resumeHere: vi.fn(() => true),
  subscribe: vi.fn((listener: (state: string) => void) => {
    controller.stateListener = listener;
    listener(controller.initialState);
    return { dispose: vi.fn() };
  }),
  scrollListener: undefined as ((atBottom: boolean) => void) | undefined,
  subscribeScroll: vi.fn((listener: (atBottom: boolean) => void) => {
    controller.scrollListener = listener;
    listener(true);
    return { dispose: vi.fn() };
  }),
  scrollToBottom: vi.fn(),
  findListener: undefined as (() => void) | undefined,
  subscribeFind: vi.fn((listener: () => void) => {
    controller.findListener = listener;
    return { dispose: vi.fn() };
  }),
  find: vi.fn(() => true),
  requestFocus: vi.fn(),
}));

vi.mock("./TerminalWorkspace", () => ({
  terminalWorkspace: {
    authenticate: vi.fn(),
    controllerFor: vi.fn(() => controller),
  },
}));
vi.mock("./XtermSurface", () => ({ XtermSurface: class {} }));
vi.mock("./TerminalConnection", () => ({ TerminalConnection: class {} }));
// The composer renders nothing, but its PROPS are the seam this view is wired
// through — without capturing them, onAttachment is unreachable and the
// picker's path is untested while every test still passes.
const composerProps = vi.hoisted(() => ({ current: undefined as undefined | { onAttachment?: (file: File) => Promise<void> } }));
vi.mock("./MobileTerminalComposer", () => ({
  MobileTerminalComposer: (props: { onAttachment?: (file: File) => Promise<void> }) => {
    composerProps.current = props;
    return null;
  },
}));
const upload = vi.hoisted(() => vi.fn());
vi.mock("./TerminalAttachments", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./TerminalAttachments")>()),
  uploadTerminalAttachment: upload,
}));

import TerminalView from "./TerminalView";

test("a passive terminal offers Resume Here and keeps its toolbar visible on mobile", () => {
  render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} />);
  act(() => controller.controlListener!("elsewhere"));
  const button = screen.getByRole("button", { name: "Resume Here" });
  expect(button.closest(".terminal-toolbar")).not.toHaveClass("terminal-toolbar-quiet");
  fireEvent.click(button);
  expect(controller.resumeHere).toHaveBeenCalled();
  act(() => controller.controlListener!("owned"));
  expect(screen.queryByRole("button", { name: "Resume Here" })).not.toBeInTheDocument();
});

test("a terminal requiring recovery offers the existing view reload action", () => {
  const refresh = vi.fn();
  render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} onRefresh={refresh} />);
  expect(screen.queryByRole("button", { name: "Reload terminal view" })).toBeNull();
  act(() => controller.stateListener!("recovery_required"));
  fireEvent.click(screen.getByRole("button", { name: "Reload terminal view" }));
  expect(refresh).toHaveBeenCalledOnce();
});

afterEach(() => {
  cleanup();
  controller.sendInput.mockReset().mockReturnValue(true);
  controller.scrollToBottom.mockClear();
  controller.initialState = "connected";
  controller.stateListener = undefined;
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

test("Ctrl+F opens a find bar that searches the terminal and its scrollback", async () => {
  render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} />);
  expect(screen.queryByRole("search")).not.toBeInTheDocument();

  // The surface takes Ctrl+F when the terminal has focus, and hands it back.
  act(() => controller.findListener!());

  const bar = await screen.findByRole("search", { name: "Search this terminal" });
  const input = within(bar).getByLabelText("Find in terminal");
  fireEvent.change(input, { target: { value: "deploy run" } });
  fireEvent.keyDown(input, { key: "Enter" });
  expect(controller.find).toHaveBeenCalledWith("deploy run", "next");

  fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
  expect(controller.find).toHaveBeenCalledWith("deploy run", "previous");
});

test("a search with no match says so, and Escape returns focus to the terminal", async () => {
  render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} />);
  act(() => controller.findListener!());
  const input = await screen.findByLabelText("Find in terminal");

  vi.mocked(controller.find).mockReturnValueOnce(false);
  fireEvent.change(input, { target: { value: "nothing here" } });
  fireEvent.keyDown(input, { key: "Enter" });
  expect(await screen.findByText("No match")).toBeInTheDocument();

  fireEvent.keyDown(input, { key: "Escape" });
  expect(screen.queryByRole("search")).not.toBeInTheDocument();
  expect(controller.requestFocus).toHaveBeenCalledWith(true);
});

test("captures Ctrl-V text before the provider receives a terminal control character", () => {
  const parentKeyDown = vi.fn();
  const { container } = render(
    <div onKeyDown={parentKeyDown}>
      <TerminalView
        busy={false}
        operatorToken="browser-session-cookie"
        session={{ session_id: "session-1", running: true }}
      />
    </div>,
  );
  const panel = container.querySelector(".terminal-panel");
  expect(panel).not.toBeNull();

  fireEvent.keyDown(panel!, { key: "v", ctrlKey: true });
  fireEvent.paste(panel!, {
    clipboardData: {
      items: [],
      getData: (type: string) => type === "text/plain" ? "one\r\ntwo" : "",
    },
  });

  expect(parentKeyDown).not.toHaveBeenCalled();
  expect(controller.sendInput).toHaveBeenCalledOnce();
  expect(controller.sendInput).toHaveBeenCalledWith("\u001b[200~one\ntwo\u001b[201~");
});

test("offers a jump to latest action only while viewing scrollback", () => {
  render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} />);
  expect(screen.queryByRole("button", { name: "Jump to latest ↓" })).not.toBeInTheDocument();
  act(() => controller.scrollListener?.(false));
  fireEvent.click(screen.getByRole("button", { name: "Jump to latest ↓" }));
  expect(controller.scrollToBottom).toHaveBeenCalledOnce();
});

test("keeps Queen automation visible beside her terminal without changing it", () => {
  const onOpenQueenSettings = vi.fn();
  render(
    <TerminalView
      busy={false}
      canStop={false}
      onOpenQueenSettings={onOpenQueenSettings}
      operatorToken="browser-session-cookie"
      queenAutomation={{
        enabled: true,
        state: "running",
        run_id: "run-1",
        trigger: "actionable_work",
        actionable_count: 3,
        attempts: 1,
        requested_at: 1,
        delivered_at: 2,
        finished_at: null,
        outcome: null,
        waiting_reason: null,
      }}
      queenAutonomy="coordinate"
      session={{ session_id: "queen-session", running: true }}
    />,
  );

  fireEvent.click(screen.getByRole("button", { name: "Reviewing work" }));

  expect(onOpenQueenSettings).toHaveBeenCalledOnce();
  expect(screen.getByRole("button", { name: "Coordinate the Hive" })).toHaveAttribute("title", expect.stringContaining("Scout"));
  expect(screen.getByText("Always active")).toBeInTheDocument();
});

test("keeps the internal terminal id behind session details", () => {
  render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "private-session-id", running: true }} />);
  expect(screen.getByText("Session details")).toBeInTheDocument();
  expect(screen.getByText("private-session-id").closest("details")).not.toHaveAttribute("open");
  expect(screen.getByText("Swarm terminal session")).toBeInTheDocument();
  expect(screen.getByText(/Claude or Codex conversation is separate/)).toBeInTheDocument();
  expect(screen.queryByText(/Continuation fallback/)).not.toBeInTheDocument();
});

test("shows continuation provenance without claiming restoration and clears it for another session", () => {
  const { rerender } = render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{
    session_id: "recovery-session", running: true,
    recovery_attempt: { recovery_id: "recovery-1", number: 2, step: { kind: "continue" } },
  }} />);
  expect(screen.getByText("Continuation fallback · see Session details")).toBeInTheDocument();
  expect(screen.getByText(/Swarm has not verified which conversation was restored/).closest("details")).not.toHaveAttribute("open");
  expect(screen.getByText("Recovery attempt 2 · recovery-1")).toBeInTheDocument();
  rerender(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "normal-session", running: true }} />);
  expect(screen.queryByText(/Continuation fallback/)).not.toBeInTheDocument();
  expect(screen.queryByText(/Recovery attempt 2/)).not.toBeInTheDocument();
});

test("settled recovery replaces startup uncertainty and does not follow another terminal", () => {
  const { rerender } = render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{
    session_id: "recovery-session", running: true,
    recovery_attempt: { recovery_id: "recovery-1", number: 2, step: { kind: "continue" } },
    recovery_outcome: { state: "restored", conversation: "restored-context", via_continue: true },
  }} />);
  expect(screen.getByText(/Provider context was restored at startup/)).toBeInTheDocument();
  expect(screen.getByText("restored-context").closest("details")).not.toHaveAttribute("open");
  expect(screen.queryByText(/Swarm has not verified/)).not.toBeInTheDocument();
  expect(screen.queryByText(/Continuation fallback/)).not.toBeInTheDocument();
  rerender(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "another", running: true }} />);
  expect(screen.queryByText(/Conversation recovery result/)).not.toBeInTheDocument();
});

test("manual and fresh outcomes never claim restored context", () => {
  const { rerender } = render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{
    session_id: "manual", running: true, recovery_outcome: { state: "manual", reason: "unexpected_conversation" },
  }} />);
  expect(screen.getByText("Check conversation · see Session details")).toBeVisible();
  expect(screen.getByText(/The saved default was not changed/)).toBeInTheDocument();
  rerender(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{
    session_id: "fresh", running: true, recovery_outcome: { state: "fresh", conversation: "new-context" },
  }} />);
  expect(screen.getByText("Fresh conversation · previous context not restored")).toBeVisible();
  expect(screen.queryByText(/Provider context was restored/)).not.toBeInTheDocument();
});

test("confirmed later selection clears old recovery attention without removing its history", () => {
  const base = { session_id: "recovered", running: true, recovery_outcome: { state: "manual" as const, reason: "unexpected_conversation" as const } };
  const { rerender } = render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={base} />);
  expect(screen.getByText("Check conversation · see Session details")).toBeVisible();
  rerender(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ ...base, confirmed_selection: { revision: 2, conversation: "chosen-context" } }} />);
  expect(screen.queryByText("Check conversation · see Session details")).not.toBeInTheDocument();
  expect(screen.getByText("Current conversation confirmed")).toBeInTheDocument();
  expect(screen.getByText(/Startup result: manual/)).toBeInTheDocument();
  expect(screen.getByText("chosen-context").closest("details")).not.toHaveAttribute("open");
  rerender(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ ...base, recovery_outcome: { state: "fresh", conversation: "fresh-context" }, confirmed_selection: { revision: 3, conversation: "chosen-context" } }} />);
  expect(screen.queryByText("Fresh conversation · previous context not restored")).not.toBeInTheDocument();
  rerender(<TerminalView busy={false} operatorToken="browser-session-cookie" session={base} />);
  expect(screen.getByText("Check conversation · see Session details")).toBeVisible();
});

test("copies the internal terminal id only when the operator asks", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "private-session-id", running: true }} />);

  fireEvent.click(screen.getByRole("button", { name: "Copy session ID" }));

  expect(writeText).toHaveBeenCalledWith("private-session-id");
  expect(await screen.findByRole("button", { name: "Copied" })).toBeInTheDocument();
});

test("keeps the destructive sleep control off the terminal bar", () => {
  // Raised as prime real estate spent on something rarely used, where the cost
  // of reaching for it by mistake is a stopped worker. Sleep lives in the
  // worker-list menu, which is where an earlier ruling put it.
  render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} />);

  expect(screen.queryByRole("button", { name: "Put worker to sleep" })).not.toBeInTheDocument();
});


/**
 * A FILE UPLOADED WHILE THE SOCKET IS DOWN IS DELIVERED, NOT LOST AND NOT LIED
 * ABOUT.
 *
 * Two silent drops met here. Uploading is plain HTTP and never needed the
 * socket, but the mobile picker refused to start without one — the operator
 * reported that as "works about half the time", because opening a phone's file
 * picker backgrounds the tab and drops the connection. And underneath,
 * `TerminalConnection#send` discards anything sent while the socket is closed,
 * silently and by design, so pasting anyway would have reported success for a
 * paste that never happened. That is worse than the silence: it is a false
 * "File added".
 *
 * The confirmation NAMES THE FILE. "File added" beside a terminal someone just
 * dropped something into is nearly contentless, and the element carrying it had
 * no CSS rule at all — it rendered as unstyled small text in a busy toolbar, so
 * an attachment that worked looked identical to one that did nothing. The
 * operator, on dropping an mp4: "Just any sort of confirmation would be good."
 */
test("an attachment uploaded while disconnected waits, then lands when the socket returns", async () => {
  upload.mockResolvedValue("/tmp/attachments/screen.png");
  controller.initialState = "disconnected";
  const { container } = render(
    <TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} />,
  );

  const file = new File([new Uint8Array([1, 2, 3])], "screen.png", { type: "image/png" });
  const surface = container.querySelector(".terminal-surface") ?? container.firstElementChild!;
  fireEvent.drop(surface, { dataTransfer: { files: [file], items: [{ kind: "file", type: "image/png" }], types: ["Files"] } });

  await screen.findByText(/waiting for the connection/i);
  expect(controller.sendInput).not.toHaveBeenCalled();

  act(() => controller.stateListener?.("connected"));

  await screen.findByText(/Added screen\.png/i);
  expect(controller.sendInput).toHaveBeenCalledWith(expect.stringContaining("/tmp/attachments/screen.png"));
});

test("a connection lost during an upload does not report a discarded reference as added", async () => {
  let finish!: (path: string) => void;
  upload.mockImplementationOnce(() => new Promise<string>((resolve) => { finish = resolve; }));
  render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} />);
  let uploadDone!: Promise<void>;
  act(() => { uploadDone = composerProps.current!.onAttachment!(new File(["image"], "screen.png", { type: "image/png" })); });
  act(() => controller.stateListener?.("disconnected"));
  await act(async () => { finish("/tmp/attachments/screen.png"); await uploadDone; });
  expect(controller.sendInput).not.toHaveBeenCalled();
  expect(screen.getByText(/waiting for the connection/i)).toBeInTheDocument();
  act(() => controller.stateListener?.("connected"));
  expect(controller.sendInput).toHaveBeenCalledTimes(1);
});

test("socket refusal wins over a stale connected UI", async () => {
  upload.mockResolvedValueOnce("/tmp/attachments/screen.png");
  controller.sendInput.mockReturnValue(false);
  render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} />);
  await act(async () => { await composerProps.current!.onAttachment!(new File(["image"], "screen.png", { type: "image/png" })); });
  expect(screen.getByText(/waiting for the connection/i)).toBeInTheDocument();
  expect(screen.queryByText(/Added screen\.png/i)).not.toBeInTheDocument();
  act(() => controller.stateListener?.("disconnected"));
  controller.sendInput.mockReturnValue(true);
  act(() => controller.stateListener?.("connected"));
  expect(screen.getByText(/Added screen\.png/i)).toBeInTheDocument();
});

test("an uploaded attachment names ownership as the wait and inserts once after Resume Here", async () => {
  upload.mockClear().mockResolvedValueOnce("/tmp/attachments/screen.png");
  controller.sendInput.mockReturnValue(false);
  render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} />);
  act(() => controller.controlListener!("elsewhere"));
  await act(async () => { await composerProps.current!.onAttachment!(new File(["image"], "screen.png", { type: "image/png" })); });
  expect(screen.getByText(/uploaded · Resume Here to add it/)).toBeInTheDocument();
  expect(screen.queryByText(/waiting for the connection/i)).not.toBeInTheDocument();
  expect(screen.queryByText(/Added screen\.png/i)).not.toBeInTheDocument();
  controller.sendInput.mockClear().mockReturnValue(true);
  fireEvent.click(screen.getByRole("button", { name: "Resume Here" }));
  // Requesting control is not confirmation; no paste before ownership arrives.
  expect(controller.sendInput).not.toHaveBeenCalled();
  act(() => controller.controlListener!("owned"));
  expect(screen.getByText(/Added screen\.png/i)).toBeInTheDocument();
  expect(controller.sendInput).toHaveBeenCalledTimes(1);
  act(() => controller.controlListener!("elsewhere"));
  act(() => controller.controlListener!("owned"));
  expect(controller.sendInput).toHaveBeenCalledTimes(1);
  expect(upload).toHaveBeenCalledTimes(1);
});

test("finishing an upload after leaving the view cannot inject into its terminal", async () => {
  let finish!: (path: string) => void;
  upload.mockImplementationOnce(() => new Promise<string>((resolve) => { finish = resolve; }));
  const view = render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} />);
  let uploadDone!: Promise<void>;
  act(() => { uploadDone = composerProps.current!.onAttachment!(new File(["image"], "screen.png", { type: "image/png" })); });
  view.unmount();
  await act(async () => { finish("/tmp/attachments/screen.png"); await uploadDone; });
  expect(controller.sendInput).not.toHaveBeenCalled();
});

test("a failed attachment retries the retained file without reopening the picker", async () => {
  upload.mockClear().mockRejectedValueOnce(new Error("offline")).mockResolvedValueOnce("/tmp/attachments/screen.png");
  render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} />);
  const file = new File(["image"], "screen.png", { type: "image/png" });
  await act(async () => { await composerProps.current!.onAttachment!(file); });
  expect(screen.getByText(/Selected file/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Retry attachment" }));
  await screen.findByText(/Added screen\.png/);
  expect(upload.mock.calls[0][2]).toBe(file);
  expect(upload.mock.calls[1][2]).toBe(file);
  expect(controller.sendInput).toHaveBeenCalledTimes(1);
  expect(screen.queryByText(/Selected file/)).not.toBeInTheDocument();
});

test("overlapping file selections cannot create concurrent uploads", async () => {
  let finish!: (path: string) => void;
  upload.mockClear().mockImplementationOnce(() => new Promise<string>((resolve) => { finish = resolve; }));
  render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} />);
  let first!: Promise<void>;
  act(() => { first = composerProps.current!.onAttachment!(new File(["one"], "one.png", { type: "image/png" })); });
  await act(async () => { await composerProps.current!.onAttachment!(new File(["two"], "two.png", { type: "image/png" })); });
  expect(upload).toHaveBeenCalledTimes(1);
  expect(screen.getByText(/Adding one.png/)).toBeInTheDocument();
  await act(async () => { finish("/tmp/attachments/one.png"); await first; });
});

test("cancelling upload aborts it and a late response cannot insert the file", async () => {
  let finish!: (path: string) => void;
  upload.mockClear().mockImplementationOnce(() => new Promise<string>((resolve) => { finish = resolve; }));
  render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} />);
  let pending!: Promise<void>;
  act(() => { pending = composerProps.current!.onAttachment!(new File(["image"], "screen.png", { type: "image/png" })); });
  const signal = upload.mock.calls[0][3] as AbortSignal;
  fireEvent.click(screen.getByRole("button", { name: "Cancel attachment" }));
  expect(signal.aborted).toBe(true);
  await act(async () => { finish("/tmp/attachments/screen.png"); await pending; });
  expect(controller.sendInput).not.toHaveBeenCalled();
  expect(screen.queryByText(/Added screen/)).not.toBeInTheDocument();
});

test("removing a waiting reference prevents reconnect from inserting it", async () => {
  upload.mockResolvedValueOnce("/tmp/attachments/screen.png");
  controller.initialState = "disconnected";
  render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} />);
  await act(async () => { await composerProps.current!.onAttachment!(new File(["image"], "screen.png", { type: "image/png" })); });
  fireEvent.click(screen.getByRole("button", { name: "Remove attachment" }));
  act(() => controller.stateListener?.("connected"));
  expect(controller.sendInput).not.toHaveBeenCalled();
});

test("a waiting attachment cannot cross into another session on view reuse", async () => {
  upload.mockResolvedValueOnce("/tmp/attachments/original.png");
  controller.initialState = "disconnected";
  const view = render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} />);
  await act(async () => { await composerProps.current!.onAttachment!(new File(["image"], "original.png", { type: "image/png" })); });
  controller.initialState = "connected";
  view.rerender(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-2", running: true }} />);
  expect(controller.sendInput).not.toHaveBeenCalled();
  expect(screen.queryByText(/original.png/)).not.toBeInTheDocument();
});

test("an upload deadline is visible and retains the file for retry", async () => {
  vi.useFakeTimers();
  upload.mockImplementationOnce((_token, _session, _file, signal: AbortSignal) => new Promise((_resolve, reject) => {
    signal.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true });
  }));
  render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} />);
  act(() => { void composerProps.current!.onAttachment!(new File(["image"], "screen.png", { type: "image/png" })); });
  await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
  expect(screen.getByText(/Upload timed out/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Retry attachment" })).toBeInTheDocument();
  expect(controller.sendInput).not.toHaveBeenCalled();
});


/**
 * The picker path is judged by the same rule a drop is.
 *
 * It was not. A dropped file was size-checked and refused by name; a file
 * picked from a phone went straight to the upload and ran until the server's
 * transport limit killed it, producing the bare failure `refuseSize` exists to
 * prevent. A phone video is the ordinary way to meet that.
 */
test("a file picked from the phone that is too large is refused before any upload", async () => {
  upload.mockClear();
  render(<TerminalView busy={false} operatorToken="browser-session-cookie" session={{ session_id: "session-1", running: true }} />);

  const huge = new File([new Uint8Array([1])], "clip.mov", { type: "video/quicktime" });
  Object.defineProperty(huge, "size", { value: 400 * 1024 * 1024 });
  await act(async () => {
    await composerProps.current?.onAttachment?.(huge);
  });

  expect(await screen.findByText(/clip\.mov/)).toBeTruthy();
  expect(screen.getByText(/the limit is/)).toBeTruthy();
  expect(upload).not.toHaveBeenCalled();
});
