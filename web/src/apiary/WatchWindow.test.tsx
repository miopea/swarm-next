import { render, screen, waitFor } from "@testing-library/react";
import { expect, test, vi, afterEach } from "vitest";

import WatchWindow, { type WatchSurface } from "./WatchWindow";

const opened: Array<{ handlers: unknown; close: ReturnType<typeof vi.fn> }> = [];

vi.mock("./WatchStream", () => ({
  WatchStream: class {
    close = vi.fn();
    constructor(readonly options: { handlers: unknown }) {
      opened.push({ handlers: options.handlers, close: this.close });
    }
    open() { return Promise.resolve(); }
  },
}));

afterEach(() => { opened.length = 0; });

function surface() {
  return { write: vi.fn(), resize: vi.fn(), clear: vi.fn(), dispose: vi.fn() } satisfies WatchSurface;
}
type Handlers = {
  onOutput(bytes: Uint8Array): void;
  onSnapshot(snapshot: { rows: number; columns: number; truncated: boolean; bytes: Uint8Array }): void;
  onState(state: string, detail?: string): void;
};

test("frames are drawn, and a snapshot resizes and replaces rather than appending", async () => {
  const drawn = surface();
  render(<WatchWindow watchId="w1" operatorToken="token" hiveName="Paul's Hive" onClose={vi.fn()} createSurface={() => drawn} />);
  await waitFor(() => expect(opened).toHaveLength(1));
  const handlers = opened[0].handlers as Handlers;

  handlers.onOutput(new TextEncoder().encode("ls\n"));
  expect(drawn.write).toHaveBeenCalledTimes(1);

  handlers.onSnapshot({ rows: 24, columns: 80, truncated: false, bytes: new TextEncoder().encode("screen") });
  expect(drawn.resize).toHaveBeenCalledWith(24, 80);
  // Cleared before writing: a snapshot IS the screen, so appending it under
  // whatever was there would draw a terminal that never existed.
  expect(drawn.clear).toHaveBeenCalled();
  expect(drawn.write).toHaveBeenCalledTimes(2);
});

/**
 * ⚠️ WATCHING IS LOOKING. Typing into someone else's machine is takeover, which
 * ADR 0036 governs separately and gates behind a reasoned, audited, exclusive
 * lease. This window must offer no way to send anything.
 */
test("the window offers no way to type into the watched Hive", async () => {
  render(<WatchWindow watchId="w1" operatorToken="token" hiveName="Paul's Hive" onClose={vi.fn()} onTakeOver={vi.fn()} createSurface={surface} />);
  await waitFor(() => expect(opened).toHaveLength(1));
  expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  // ⚠️ THE CLAIM IS THAT NOTHING SENDS, NOT THAT ONE BUTTON EXISTS. This used
  // to assert the button list was exactly ["Close window"], so adding the tools
  // the operator asked for failed it for no reason that matters. What must stay
  // true is that every control here either ends the watch or escalates to a
  // takeover, which ADR 0036 governs separately — none of them writes to the
  // watched Hive.
  expect(screen.queryAllByRole("button").map((button) => button.textContent))
    .toEqual(["Take over", "Stop watching"]);
});

/** Escalation is offered only when there is something to escalate to. */
test("without an escalation the window offers only stopping", async () => {
  render(<WatchWindow watchId="w1" operatorToken="token" hiveName="Paul's Hive" onClose={vi.fn()} createSurface={surface} />);
  await waitFor(() => expect(opened).toHaveLength(1));
  expect(screen.queryAllByRole("button").map((button) => button.textContent))
    .toEqual(["Stop watching"]);
});

test("the window says what it is and that the watched Hive knows", async () => {
  render(<WatchWindow watchId="w1" operatorToken="token" hiveName="Paul's Hive" onClose={vi.fn()} createSurface={surface} />);
  await waitFor(() => expect(opened).toHaveLength(1));
  const handlers = opened[0].handlers as Handlers;

  handlers.onState("live");
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Live"));
  expect(screen.getByText(/Nothing here is recorded, and Paul's Hive is showing that you are watching/)).toBeInTheDocument();

  handlers.onState("closed", "The window closed.");
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("The window closed."));
});

/** Closing must release the socket AND the surface, or a dismissed window keeps
 * receiving someone's terminal in the background. */
test("closing releases the stream and the surface", async () => {
  const drawn = surface();
  const view = render(<WatchWindow watchId="w1" operatorToken="token" hiveName="Paul's Hive" onClose={vi.fn()} createSurface={() => drawn} />);
  await waitFor(() => expect(opened).toHaveLength(1));
  view.unmount();
  expect(opened[0].close).toHaveBeenCalled();
  expect(drawn.dispose).toHaveBeenCalled();
});
