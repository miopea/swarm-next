import { afterEach, expect, test, vi } from "vitest";

import { TerminalConnection, type TerminalConnectionHandlers } from "./TerminalConnection";

class FakeWebSocket extends EventTarget {
  static readonly OPEN = WebSocket.OPEN;
  binaryType = "blob";
  readyState: number = WebSocket.CONNECTING;
  readonly sent: string[] = [];
  close = vi.fn(() => {
    this.readyState = WebSocket.CLOSED;
  });

  send(payload: string): void {
    this.sent.push(payload);
  }

  open(): void {
    this.readyState = WebSocket.OPEN;
    this.dispatchEvent(new Event("open"));
  }

  message(data: string | ArrayBuffer): void {
    this.dispatchEvent(new MessageEvent("message", { data }));
  }

  disconnect(): void {
    this.readyState = WebSocket.CLOSED;
    this.dispatchEvent(new CloseEvent("close"));
  }
}

function outputFrame(sequence: bigint, text: string): ArrayBuffer {
  const bytes = new TextEncoder().encode(text);
  const frame = new Uint8Array(bytes.length + 9);
  frame[0] = 1;
  new DataView(frame.buffer).setBigUint64(1, sequence);
  frame.set(bytes, 9);
  return frame.buffer;
}

function harness(retryDelaysMs: readonly number[] = [1]) {
  const sockets: FakeWebSocket[] = [];
  const fetch = vi.fn().mockResolvedValue({
    ok: true,
    json: async () => ({
      grant: `grant-${sockets.length}`,
      protocol: "swarm-terminal.v1",
      websocket_path: "/api/v1/terminal/sessions/session-1/attach",
      expires_in_ms: 30_000,
    }),
  });
  const handlers: TerminalConnectionHandlers = {
    onOutput: vi.fn(),
    onState: vi.fn(),
    onRunningChange: vi.fn(),
  };
  const connection = new TerminalConnection({
    sessionId: "session-1",
    operatorToken: "secret",
    fetch,
    locationOrigin: "http://127.0.0.1:5173",
    retryDelaysMs,
    websocketFactory: (_url, protocols) => {
      expect(protocols[0]).toBe("swarm-terminal.v1");
      expect(protocols[1]).toMatch(/^swarm-grant\./);
      const socket = new FakeWebSocket();
      sockets.push(socket);
      return socket as unknown as WebSocket;
    },
  });
  return { connection, fetch, handlers, sockets };
}

afterEach(() => {
  vi.useRealTimers();
});

test("requests a no-store grant and resumes from the last applied sequence", async () => {
  const { connection, fetch, handlers, sockets } = harness();
  connection.start(handlers);
  await vi.waitFor(() => expect(sockets).toHaveLength(1));
  expect(fetch).toHaveBeenCalledWith(
    "/api/v1/terminal/sessions/session-1/attach-grants",
    expect.objectContaining({ method: "POST", cache: "no-store" }),
  );
  sockets[0].open();
  expect(JSON.parse(sockets[0].sent[0])).toEqual({ type: "resume", after_sequence: 0 });
  sockets[0].message(outputFrame(1n, "one"));
  sockets[0].message(outputFrame(1n, "duplicate"));
  expect(connection.sequence).toBe(1);
  expect(handlers.onOutput).toHaveBeenCalledTimes(1);
  expect(Array.from(vi.mocked(handlers.onOutput).mock.calls[0][0])).toEqual([111, 110, 101]);
});

test("detects sequence gaps and does not reconnect without a snapshot", async () => {
  vi.useFakeTimers();
  const { connection, handlers, sockets } = harness([1]);
  connection.start(handlers);
  await vi.advanceTimersByTimeAsync(0);
  sockets[0].open();
  sockets[0].message(outputFrame(2n, "gap"));
  sockets[0].disconnect();
  await vi.advanceTimersByTimeAsync(10);
  expect(sockets).toHaveLength(1);
  expect(handlers.onState).toHaveBeenCalledWith("recovery_required", expect.stringContaining("expected 1"));
});

test("unexpected disconnect obtains a fresh grant and resumes without duplicating output", async () => {
  vi.useFakeTimers();
  const { connection, fetch, handlers, sockets } = harness([1]);
  connection.start(handlers);
  await vi.advanceTimersByTimeAsync(0);
  sockets[0].open();
  sockets[0].message(outputFrame(1n, "one"));
  sockets[0].disconnect();
  await vi.advanceTimersByTimeAsync(1);
  await vi.advanceTimersByTimeAsync(0);
  expect(fetch).toHaveBeenCalledTimes(2);
  expect(sockets).toHaveLength(2);
  sockets[1].open();
  expect(JSON.parse(sockets[1].sent[0])).toEqual({ type: "resume", after_sequence: 1 });
  sockets[1].message(outputFrame(2n, "two"));
  expect(handlers.onOutput).toHaveBeenCalledTimes(2);
});

test("explicit disposal cancels reconnect ownership", async () => {
  vi.useFakeTimers();
  const { connection, sockets } = harness([1]);
  connection.start({ onOutput: vi.fn(), onState: vi.fn(), onRunningChange: vi.fn() });
  await vi.advanceTimersByTimeAsync(0);
  sockets[0].open();
  sockets[0].disconnect();
  connection.dispose();
  await vi.advanceTimersByTimeAsync(10);
  expect(sockets).toHaveLength(1);
});

test("an open-close loop cannot reset the bounded reconnect budget", async () => {
  vi.useFakeTimers();
  const { connection, handlers, sockets } = harness([1, 1]);
  connection.start(handlers);
  await vi.advanceTimersByTimeAsync(0);
  for (let index = 0; index < 3; index += 1) {
    sockets[index].open();
    sockets[index].disconnect();
    await vi.advanceTimersByTimeAsync(1);
    await vi.advanceTimersByTimeAsync(0);
  }
  expect(sockets).toHaveLength(3);
  expect(handlers.onState).toHaveBeenCalledWith("error", expect.stringContaining("reconnect limit reached"));
});
