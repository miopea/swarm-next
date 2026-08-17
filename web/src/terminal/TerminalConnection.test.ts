import { afterEach, expect, test, vi } from "vitest";

import { BROWSER_SESSION_AUTH } from "../api";
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

function outputBytesFrame(sequence: bigint, byteLength: number): ArrayBuffer {
  const frame = new Uint8Array(byteLength + 9);
  frame[0] = 1;
  new DataView(frame.buffer).setBigUint64(1, sequence);
  return frame.buffer;
}

function snapshotFrame(
  sequence: bigint,
  rows: number,
  columns: number,
  text: string,
  truncated = false,
): ArrayBuffer {
  const bytes = new TextEncoder().encode(text);
  const frame = new Uint8Array(bytes.length + 14);
  frame[0] = 2;
  const view = new DataView(frame.buffer);
  view.setBigUint64(1, sequence);
  view.setUint16(9, rows);
  view.setUint16(11, columns);
  frame[13] = truncated ? 1 : 0;
  frame.set(bytes, 14);
  return frame.buffer;
}

function harness(
  retryDelaysMs: readonly number[] = [1],
  confirmationTimeoutMs = 3_000,
  operatorToken = "secret",
  useDefaultRetries = false,
) {
  const sockets: FakeWebSocket[] = [];
  const fetch = vi.fn().mockResolvedValue({
    ok: true,
    json: async () => ({
      grant: `grant-${sockets.length}`,
      protocol: "swarm-terminal.v3",
      websocket_path: "/api/v1/terminal/sessions/session-1/attach",
      expires_in_ms: 30_000,
    }),
  });
  const handlers: TerminalConnectionHandlers = {
    onOutput: vi.fn(),
    onSnapshot: vi.fn(),
    onState: vi.fn(),
    onRunningChange: vi.fn(),
  };
  const connection = new TerminalConnection({
    sessionId: "session-1",
    operatorToken,
    fetch,
    locationOrigin: "http://127.0.0.1:5173",
    ...(useDefaultRetries ? {} : { retryDelaysMs }),
    confirmationTimeoutMs,
    deviceId: "019fedfc-1c30-70e1-a5e2-9a3c94268093",
    websocketFactory: (_url, protocols) => {
      expect(protocols[0]).toBe("swarm-terminal.v3");
      expect(protocols[1]).toMatch(/^swarm-grant\./);
      const socket = new FakeWebSocket();
      sockets.push(socket);
      return socket as unknown as WebSocket;
    },
  });
  connection.resize(24, 80);
  return { connection, fetch, handlers, sockets };
}

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

test("requests a no-store grant and applies a snapshot before sequenced deltas", async () => {
  const { connection, fetch, handlers, sockets } = harness();
  connection.start(handlers);
  await vi.waitFor(() => expect(sockets).toHaveLength(1));
  expect(fetch).toHaveBeenCalledWith(
    "/api/v1/terminal/sessions/session-1/attach-grants",
    expect.objectContaining({ method: "POST", cache: "no-store" }),
  );
  sockets[0].open();
  expect(JSON.parse(sockets[0].sent[0])).toEqual({
    type: "resume",
    after_sequence: null,
    rows: 24,
    columns: 80,
    device_id: "019fedfc-1c30-70e1-a5e2-9a3c94268093",
    claim_geometry: true,
  });
  sockets[0].message(snapshotFrame(0n, 24, 80, "screen"));
  sockets[0].message(outputFrame(1n, "one"));
  sockets[0].message(outputFrame(1n, "duplicate"));
  await vi.waitFor(() => expect(connection.sequence).toBe(1));
  expect(handlers.onSnapshot).toHaveBeenCalledWith(
    expect.objectContaining({ sequence: 0, rows: 24, columns: 80, truncated: false }),
  );
  expect(handlers.onOutput).toHaveBeenCalledTimes(1);
  expect(Array.from(vi.mocked(handlers.onOutput).mock.calls[0][0])).toEqual([111, 110, 101]);
});

test("visible viewport resizes reclaim geometry while hidden observers stay passive", async () => {
  const { connection, handlers, sockets } = harness();
  connection.start(handlers);
  await vi.waitFor(() => expect(sockets).toHaveLength(1));
  sockets[0].open();

  connection.resize(38, 154);
  expect(JSON.parse(sockets[0].sent.at(-1) ?? "null")).toEqual({
    type: "resize",
    rows: 38,
    columns: 154,
    claim_geometry: true,
  });

  vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
  connection.resize(20, 72);
  expect(JSON.parse(sockets[0].sent.at(-1) ?? "null")).toEqual({
    type: "resize",
    rows: 20,
    columns: 72,
    claim_geometry: false,
  });
});

test("uses the trusted browser cookie for terminal attach grants", async () => {
  const { connection, fetch, handlers, sockets } = harness([1], 3_000, BROWSER_SESSION_AUTH);
  connection.start(handlers);
  await vi.waitFor(() => expect(sockets).toHaveLength(1));

  const init = fetch.mock.calls[0]?.[1] as RequestInit;
  expect(init.credentials).toBe("same-origin");
  expect(new Headers(init.headers).get("Authorization")).toBeNull();
  connection.dispose();
});

test("does not report connected until the canonical renderer is ready", async () => {
  let releaseSnapshot: (() => void) | undefined;
  const rendered = new Promise<void>((resolve) => {
    releaseSnapshot = resolve;
  });
  const { connection, handlers, sockets } = harness();
  handlers.onSnapshot = vi.fn(() => rendered);
  connection.start(handlers);
  await vi.waitFor(() => expect(sockets).toHaveLength(1));

  sockets[0].open();
  sockets[0].message(JSON.stringify({ type: "state", running: true }));
  expect(vi.mocked(handlers.onState).mock.calls.some(([state]) => state === "connected")).toBe(false);
  sockets[0].message(snapshotFrame(0n, 24, 80, "screen"));
  await vi.waitFor(() => expect(handlers.onSnapshot).toHaveBeenCalledTimes(1));
  expect(vi.mocked(handlers.onState).mock.calls.some(([state]) => state === "connected")).toBe(false);

  releaseSnapshot?.();
  await vi.waitFor(() => expect(handlers.onState).toHaveBeenCalledWith("connected", undefined));
});

test("a completed provider session closes without a reconnect loop", async () => {
  vi.useFakeTimers();
  const { connection, handlers, sockets } = harness([1]);
  connection.start(handlers);
  await vi.advanceTimersByTimeAsync(0);
  sockets[0].open();
  sockets[0].message(snapshotFrame(0n, 24, 80, "Resume this session"));
  await vi.advanceTimersByTimeAsync(0);
  sockets[0].message(JSON.stringify({ type: "state", running: false }));
  sockets[0].disconnect();
  await vi.advanceTimersByTimeAsync(10);

  expect(handlers.onRunningChange).toHaveBeenCalledWith(false);
  expect(sockets).toHaveLength(1);
});

test("detects sequence gaps and reconnects from a fresh snapshot", async () => {
  vi.useFakeTimers();
  const { connection, handlers, sockets } = harness([1]);
  connection.start(handlers);
  await vi.advanceTimersByTimeAsync(0);
  sockets[0].open();
  sockets[0].message(snapshotFrame(0n, 24, 80, "screen"));
  await vi.advanceTimersByTimeAsync(0);
  sockets[0].message(outputFrame(2n, "gap"));
  await vi.advanceTimersByTimeAsync(0);
  sockets[0].disconnect();
  await vi.advanceTimersByTimeAsync(1);
  await vi.advanceTimersByTimeAsync(0);
  expect(sockets).toHaveLength(2);
  sockets[1].open();
  expect(JSON.parse(sockets[1].sent[0])).toEqual({
    type: "resume",
    after_sequence: null,
    rows: 24,
    columns: 80,
    device_id: "019fedfc-1c30-70e1-a5e2-9a3c94268093",
    claim_geometry: true,
  });
  expect(handlers.onState).toHaveBeenCalledWith(
    "disconnected",
    expect.stringContaining("requesting a fresh snapshot"),
  );
});

test("unexpected disconnect obtains a fresh grant and resumes without duplicating output", async () => {
  vi.useFakeTimers();
  const { connection, fetch, handlers, sockets } = harness([1]);
  connection.start(handlers);
  await vi.advanceTimersByTimeAsync(0);
  sockets[0].open();
  sockets[0].message(snapshotFrame(0n, 24, 80, "screen"));
  await vi.advanceTimersByTimeAsync(0);
  sockets[0].message(outputFrame(1n, "one"));
  await vi.advanceTimersByTimeAsync(0);
  sockets[0].disconnect();
  await vi.advanceTimersByTimeAsync(1);
  await vi.advanceTimersByTimeAsync(0);
  expect(fetch).toHaveBeenCalledTimes(2);
  expect(sockets).toHaveLength(2);
  sockets[1].open();
  expect(JSON.parse(sockets[1].sent[0])).toEqual({
    type: "resume",
    after_sequence: 1,
    rows: 24,
    columns: 80,
    device_id: "019fedfc-1c30-70e1-a5e2-9a3c94268093",
    claim_geometry: true,
  });
  sockets[1].message(outputFrame(2n, "two"));
  await vi.waitFor(() => expect(connection.sequence).toBe(2));
  expect(handlers.onOutput).toHaveBeenCalledTimes(2);
});

test("explicit disposal cancels reconnect ownership", async () => {
  vi.useFakeTimers();
  const { connection, sockets } = harness([1]);
  connection.start({
    onOutput: vi.fn(),
    onSnapshot: vi.fn(),
    onState: vi.fn(),
    onRunningChange: vi.fn(),
  });
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

test("an unconfirmed socket is abandoned inside the bounded retry budget", async () => {
  vi.useFakeTimers();
  const { connection, handlers, sockets } = harness([1], 1);
  connection.start(handlers);
  await vi.advanceTimersByTimeAsync(0);
  sockets[0].open();

  await vi.advanceTimersByTimeAsync(1);
  expect(sockets[0].close).toHaveBeenCalledWith(4013, "terminal confirmation timed out");
  expect(handlers.onState).toHaveBeenCalledWith(
    "disconnected",
    "terminal connection received no confirmation",
  );

  await vi.advanceTimersByTimeAsync(1);
  await vi.advanceTimersByTimeAsync(0);
  expect(sockets).toHaveLength(2);
});

test("the default reconnect budget spans a bounded rolling API handoff", async () => {
  vi.useFakeTimers();
  const { connection, fetch, handlers } = harness([], 3_000, "secret", true);
  fetch.mockResolvedValue({ ok: false } as Response);
  connection.start(handlers);

  await vi.advanceTimersByTimeAsync(8_000);
  expect(fetch.mock.calls.length).toBeGreaterThanOrEqual(7);
  expect(handlers.onState).not.toHaveBeenCalledWith(
    "error",
    expect.stringContaining("reconnect limit reached"),
  );

  await vi.advanceTimersByTimeAsync(8_000);
  expect(fetch).toHaveBeenCalledTimes(8);
  expect(handlers.onState).toHaveBeenCalledWith(
    "error",
    expect.stringContaining("reconnect limit reached"),
  );
});

test("protocol failures use a browser-valid application close code", async () => {
  const { connection, handlers, sockets } = harness();
  connection.start(handlers);
  await vi.waitFor(() => expect(sockets).toHaveLength(1));
  sockets[0].open();

  sockets[0].message("not-json");

  expect(sockets[0].close).toHaveBeenCalledWith(4008, "terminal WebSocket returned invalid JSON");
  expect(handlers.onState).toHaveBeenCalledWith(
    "recovery_required",
    "terminal WebSocket returned invalid JSON",
  );
});

test("state messages without canonical bytes cannot reset the reconnect budget", async () => {
  vi.useFakeTimers();
  const { connection, handlers, sockets } = harness([1], 1);
  connection.start(handlers);
  await vi.advanceTimersByTimeAsync(0);
  sockets[0].open();
  sockets[0].message(JSON.stringify({ type: "state", running: true }));

  await vi.advanceTimersByTimeAsync(1);
  await vi.advanceTimersByTimeAsync(1);
  await vi.advanceTimersByTimeAsync(0);
  expect(sockets).toHaveLength(2);
  sockets[1].open();
  sockets[1].message(JSON.stringify({ type: "state", running: true }));

  await vi.advanceTimersByTimeAsync(1);
  expect(handlers.onState).toHaveBeenCalledWith(
    "error",
    expect.stringContaining("reconnect limit reached"),
  );
});

test("an unconfirmed attach grant is aborted inside the bounded retry budget", async () => {
  vi.useFakeTimers();
  const fetch = vi.fn((_input: RequestInfo | URL, init?: RequestInit) =>
    new Promise<Response>((_resolve, reject) => {
      init?.signal?.addEventListener(
        "abort",
        () => reject(new DOMException("Aborted", "AbortError")),
        { once: true },
      );
    }),
  );
  const handlers: TerminalConnectionHandlers = {
    onOutput: vi.fn(),
    onSnapshot: vi.fn(),
    onState: vi.fn(),
    onRunningChange: vi.fn(),
  };
  const connection = new TerminalConnection({
    sessionId: "session-1",
    operatorToken: "secret",
    fetch: fetch as typeof window.fetch,
    retryDelaysMs: [1],
    confirmationTimeoutMs: 1,
    websocketFactory: vi.fn(),
  });
  connection.resize(24, 80);
  connection.start(handlers);
  await vi.advanceTimersByTimeAsync(0);
  expect(fetch).toHaveBeenCalledTimes(1);

  await vi.advanceTimersByTimeAsync(1);
  expect(handlers.onState).toHaveBeenCalledWith(
    "disconnected",
    "terminal attach grant received no response",
  );

  await vi.advanceTimersByTimeAsync(1);
  await vi.advanceTimersByTimeAsync(0);
  expect(fetch).toHaveBeenCalledTimes(2);
  connection.dispose();
});

test("renderer backlog is bounded and forces canonical recovery", async () => {
  let releaseOutput: (() => void) | undefined;
  const blockedOutput = new Promise<void>((resolve) => {
    releaseOutput = resolve;
  });
  const { connection, handlers, sockets } = harness();
  handlers.onOutput = vi.fn(() => blockedOutput);
  connection.start(handlers);
  await vi.waitFor(() => expect(sockets).toHaveLength(1));
  sockets[0].open();
  sockets[0].message(snapshotFrame(0n, 24, 80, "screen"));
  await vi.waitFor(() => expect(handlers.onSnapshot).toHaveBeenCalledTimes(1));

  for (let sequence = 1; sequence <= 385; sequence += 1) {
    sockets[0].message(outputBytesFrame(BigInt(sequence), 8_192));
  }

  expect(sockets[0].close).toHaveBeenCalledWith(4013, "fresh terminal snapshot required");
  expect(handlers.onState).toHaveBeenCalledWith(
    "disconnected",
    expect.stringContaining("bounded queue"),
  );
  releaseOutput?.();
});

test("resume cursor advances only after the renderer applies output", async () => {
  let releaseOutput: (() => void) | undefined;
  const rendered = new Promise<void>((resolve) => {
    releaseOutput = resolve;
  });
  const { connection, handlers, sockets } = harness();
  handlers.onOutput = vi.fn(() => rendered);
  connection.start(handlers);
  await vi.waitFor(() => expect(sockets).toHaveLength(1));
  sockets[0].open();
  sockets[0].message(snapshotFrame(0n, 24, 80, "screen"));
  await vi.waitFor(() => expect(handlers.onSnapshot).toHaveBeenCalledTimes(1));
  sockets[0].message(outputFrame(1n, "pending"));
  await vi.waitFor(() => expect(handlers.onOutput).toHaveBeenCalledTimes(1));
  expect(connection.sequence).toBe(0);

  releaseOutput?.();
  await vi.waitFor(() => expect(connection.sequence).toBe(1));
});

test("a memory-safety snapshot reset remains visible to the operator", async () => {
  const { connection, handlers, sockets } = harness();
  connection.start(handlers);
  await vi.waitFor(() => expect(sockets).toHaveLength(1));
  sockets[0].open();
  sockets[0].message(snapshotFrame(12n, 24, 80, "bounded", true));

  await vi.waitFor(() => expect(connection.sequence).toBe(12));
  expect(handlers.onState).toHaveBeenCalledWith(
    "connected",
    expect.stringContaining("canonical memory bound"),
  );
});
