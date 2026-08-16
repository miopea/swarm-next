import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

const controller = vi.hoisted(() => ({
  attach: vi.fn(),
  detach: vi.fn(),
  sendInput: vi.fn(),
  subscribe: vi.fn((listener: (state: string) => void) => {
    listener("connected");
    return { dispose: vi.fn() };
  }),
}));

vi.mock("./TerminalWorkspace", () => ({
  terminalWorkspace: {
    authenticate: vi.fn(),
    controllerFor: vi.fn(() => controller),
  },
}));
vi.mock("./XtermSurface", () => ({ XtermSurface: class {} }));
vi.mock("./TerminalConnection", () => ({ TerminalConnection: class {} }));
vi.mock("./MobileTerminalComposer", () => ({ MobileTerminalComposer: () => null }));

import TerminalView from "./TerminalView";

afterEach(() => {
  cleanup();
  controller.sendInput.mockClear();
});

test("captures Ctrl-V text before the provider receives a terminal control character", () => {
  const parentKeyDown = vi.fn();
  const { container } = render(
    <div onKeyDown={parentKeyDown}>
      <TerminalView
        busy={false}
        onStop={vi.fn()}
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

test("keeps Queen automation visible beside her terminal without changing it", () => {
  const onOpenQueenSettings = vi.fn();
  render(
    <TerminalView
      busy={false}
      canStop={false}
      onOpenQueenSettings={onOpenQueenSettings}
      onStop={vi.fn()}
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
      session={{ session_id: "queen-session", running: true }}
    />,
  );

  fireEvent.click(screen.getByRole("button", { name: "Reviewing work" }));

  expect(onOpenQueenSettings).toHaveBeenCalledOnce();
  expect(screen.getByText("Always active")).toBeInTheDocument();
});
