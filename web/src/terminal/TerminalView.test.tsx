import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

const controller = vi.hoisted(() => ({
  attach: vi.fn(),
  detach: vi.fn(),
  sendInput: vi.fn(),
  stateListener: undefined as ((state: string) => void) | undefined,
  initialState: "connected",
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

afterEach(() => {
  cleanup();
  controller.sendInput.mockClear();
  controller.scrollToBottom.mockClear();
  controller.initialState = "connected";
  controller.stateListener = undefined;
  vi.unstubAllGlobals();
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
