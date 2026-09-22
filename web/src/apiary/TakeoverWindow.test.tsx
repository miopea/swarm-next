import { render, screen, waitFor } from "@testing-library/react";
import { expect, test, vi, afterEach } from "vitest";

import TakeoverWindow, { type Surface } from "./TakeoverWindow";

const sockets: Array<{ protocols: string[]; sent: Uint8Array[]; listeners: Map<string, (e: unknown) => void> }> = [];

class FakeSocket {
  static readonly OPEN = 1;
  readyState = 1;
  binaryType = "blob";
  readonly sent: Uint8Array[] = [];
  readonly listeners = new Map<string, (e: unknown) => void>();
  constructor(readonly url: string, readonly protocols: string[]) {
    sockets.push({ protocols, sent: this.sent, listeners: this.listeners });
  }
  addEventListener(kind: string, handler: (e: unknown) => void) { this.listeners.set(kind, handler); }
  send(frame: Uint8Array) { this.sent.push(frame); }
  close() { /* nothing to tear down in the fake */ }
}

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  requestTakeoverControlGrant: vi.fn(),
}));
const { requestTakeoverControlGrant } = await import("../api");
const grant = vi.mocked(requestTakeoverControlGrant);
const ticket = { grant: "ticket-1", websocket_path: "/api/v1/apiary/takeovers/lease-1/control" };

afterEach(() => { sockets.length = 0; grant.mockReset(); });

function harness() {
  if (grant.mock.calls.length === 0 && grant.getMockImplementation() === undefined) grant.mockResolvedValue(ticket);
  const keys: ((data: string) => void)[] = [];
  const drawn: Surface = { write: vi.fn(), resize: vi.fn(), clear: vi.fn(), dispose: vi.fn() };
  vi.stubGlobal("WebSocket", FakeSocket);
  const view = render(<TakeoverWindow
    leaseId="lease-1"
    operatorToken="token"
    hiveName="Paul's Hive"
    onClose={vi.fn()}
    createSurface={(_host, onKey) => { keys.push(onKey); return drawn; }}
  />);
  return { keys, drawn, view };
}

/**
 * ⚠️ THE ONE THING THAT MAKES THIS TAKEOVER RATHER THAN WATCHING: it types.
 * The operator token must never reach the socket, so the grant travels as a
 * subprotocol exactly as the watch viewer does.
 */
test("keystrokes are sent, and the socket carries a grant rather than the token", async () => {
  const { keys } = harness();
  await waitFor(() => expect(sockets).toHaveLength(1));
  expect(sockets[0].protocols).toEqual(["swarm-takeover.ticket-1"]);
  expect(JSON.stringify(sockets[0].protocols)).not.toContain("token");

  keys[0]("ls\n");
  expect(sockets[0].sent).toHaveLength(1);
  expect(sockets[0].sent[0][0]).toBe(9);
  expect(new TextDecoder().decode(sockets[0].sent[0].slice(1))).toBe("ls\n");
});

test("the screen is drawn, and a snapshot replaces rather than appends", async () => {
  const { drawn } = harness();
  await waitFor(() => expect(sockets).toHaveLength(1));
  const message = sockets[0].listeners.get("message")!;

  const output = new Uint8Array(9 + 5);
  output[0] = 1;
  output.set(new TextEncoder().encode("hello"), 9);
  message({ data: output.buffer });
  expect(drawn.write).toHaveBeenCalledTimes(1);

  const snapshot = new Uint8Array(14 + 6);
  snapshot[0] = 2;
  const view = new DataView(snapshot.buffer);
  view.setUint16(9, 24);
  view.setUint16(11, 80);
  snapshot.set(new TextEncoder().encode("screen"), 14);
  message({ data: snapshot.buffer });
  expect(drawn.resize).toHaveBeenCalledWith(24, 80);
  expect(drawn.clear).toHaveBeenCalled();
});

/**
 * The likeliest reason a takeover ends is the other operator taking their
 * machine back. Calling that a connection error would blame the network for
 * somebody's decision about their own computer.
 */
test("an ended takeover says it ended, and says the Hive can end it", async () => {
  harness();
  await waitFor(() => expect(sockets).toHaveLength(1));
  sockets[0].listeners.get("close")!({});
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("The takeover ended."));
  expect(screen.getByText(/can take it back at any moment/)).toBeInTheDocument();
});

/**
 * ⚠️ A TAKEOVER IS NOT CONTROLLABLE UNTIL THE TARGET ACKNOWLEDGES, and the
 * target learns on its own federation pass. Asking once and giving up showed
 * "no longer active" for a takeover seconds old — a working feature reading as
 * a broken one.
 */
test("the window waits for the Hive to accept rather than declaring it dead", async () => {
  vi.useFakeTimers();
  grant.mockRejectedValueOnce(new Error("not active yet")).mockResolvedValue(ticket);
  vi.stubGlobal("WebSocket", FakeSocket);
  render(<TakeoverWindow
    leaseId="lease-1"
    operatorToken="token"
    hiveName="Paul's Hive"
    onClose={vi.fn()}
    createSurface={() => ({ write: vi.fn(), resize: vi.fn(), clear: vi.fn(), dispose: vi.fn() })}
  />);
  // Two flushes: the rejection is handled in a microtask, and the state it sets
  // renders on the next.
  await vi.advanceTimersByTimeAsync(0);
  await vi.advanceTimersByTimeAsync(0);
  expect(screen.getByRole("status")).toHaveTextContent("Waiting for this Hive to accept");
  await vi.advanceTimersByTimeAsync(2_000);
  await vi.waitFor(() => expect(sockets).toHaveLength(1));
  vi.useRealTimers();
});
