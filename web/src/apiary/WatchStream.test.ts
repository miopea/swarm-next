import { expect, test, vi } from "vitest";

import { WatchStream, watchGrantFailure, type WatchStreamHandlers } from "./WatchStream";

class FakeWebSocket extends EventTarget {
  binaryType = "blob";
  readonly closed = vi.fn();
  constructor(readonly url: string, readonly protocols: string[]) { super(); }
  close(): void { this.closed(); }
  deliver(frame: Uint8Array): void {
    const buffer = frame.buffer.slice(frame.byteOffset, frame.byteOffset + frame.byteLength) as ArrayBuffer;
    this.dispatchEvent(Object.assign(new Event("message"), { data: buffer }));
  }
}

function harness(grantResponse?: Partial<Response>) {
  const handlers: WatchStreamHandlers = { onOutput: vi.fn(), onSnapshot: vi.fn(), onState: vi.fn() };
  const sockets: FakeWebSocket[] = [];
  const fetch = vi.fn().mockResolvedValue({
    ok: true,
    status: 200,
    json: async () => ({ grant: "ticket-1", websocket_path: "/api/v1/apiary/watches/w1/stream" }),
    ...grantResponse,
  });
  const stream = new WatchStream({
    watchId: "w1",
    operatorToken: "token",
    handlers,
    fetch: fetch as unknown as typeof window.fetch,
    locationOrigin: "http://127.0.0.1:5173",
    websocketFactory: (url, protocols) => {
      const socket = new FakeWebSocket(url, protocols);
      sockets.push(socket);
      return socket as unknown as WebSocket;
    },
  });
  return { handlers, sockets, fetch, stream };
}

function outputFrame(sequence: number, text: string): Uint8Array {
  const body = new TextEncoder().encode(text);
  const frame = new Uint8Array(9 + body.byteLength);
  frame[0] = 1;
  new DataView(frame.buffer).setBigUint64(1, BigInt(sequence));
  frame.set(body, 9);
  return frame;
}

/**
 * ⚠️ THE OPERATOR TOKEN MUST NEVER REACH THE SOCKET. A browser cannot send an
 * Authorization header on a WebSocket, so the only alternatives were a
 * short-lived single-use grant or putting a long-lived credential into a
 * subprotocol string that proxies and logs routinely record.
 */
test("the socket carries a single-use grant and never the operator token", async () => {
  const { stream, sockets, fetch, handlers } = harness();
  await stream.open();

  expect(fetch).toHaveBeenCalledWith(
    "http://127.0.0.1:5173/api/v1/apiary/watches/w1/grant",
    expect.objectContaining({ method: "POST", headers: { Authorization: "Bearer token" } }),
  );
  expect(sockets).toHaveLength(1);
  expect(sockets[0].url).toBe("ws://127.0.0.1:5173/api/v1/apiary/watches/w1/stream");
  expect(sockets[0].protocols).toEqual(["swarm-watch.ticket-1"]);
  expect(JSON.stringify(sockets[0].protocols)).not.toContain("token");
  expect(handlers.onState).toHaveBeenCalledWith("connecting");
});

test("output and snapshot frames are decoded and handed straight on", async () => {
  const { stream, sockets, handlers } = harness();
  await stream.open();
  sockets[0].dispatchEvent(new Event("open"));
  expect(handlers.onState).toHaveBeenCalledWith("live");

  sockets[0].deliver(outputFrame(7, "hello"));
  // Compared as bytes rather than by deep equality: a Uint8Array that crossed a
  // realm boundary is not `toEqual` another one even when every byte matches.
  const output = vi.mocked(handlers.onOutput).mock.calls[0][0];
  expect(new TextDecoder().decode(output)).toBe("hello");

  const body = new TextEncoder().encode("screen");
  const snapshot = new Uint8Array(14 + body.byteLength);
  snapshot[0] = 2;
  const view = new DataView(snapshot.buffer);
  view.setBigUint64(1, 12n);
  view.setUint16(9, 24);
  view.setUint16(11, 80);
  snapshot[13] = 1;
  snapshot.set(body, 14);
  sockets[0].deliver(snapshot);
  const received = vi.mocked(handlers.onSnapshot).mock.calls[0][0];
  expect({ rows: received.rows, columns: received.columns, truncated: received.truncated }).toEqual({
    rows: 24, columns: 80, truncated: true,
  });
  expect(new TextDecoder().decode(received.bytes)).toBe("screen");
});

/**
 * A watch can be ended by the person being watched, at any moment. Reporting
 * that as a connection failure would blame the network for someone's decision.
 */
test("a closed window says the window closed, not that something failed", async () => {
  const { stream, sockets, handlers } = harness();
  await stream.open();
  sockets[0].dispatchEvent(new Event("close"));
  expect(handlers.onState).toHaveBeenCalledWith("closed", "The window closed.");
});

/**
 * ⚠️ A REFUSAL IS RETRIED BEFORE IT IS BELIEVED. A watch is not live until the
 * watched Hive acknowledges it, and that Hive learns on its own federation
 * pass — so asking once and giving up reported "no longer live" for a watch a
 * second old. Fake timers here because the real wait is a minute.
 */
test("a refused grant is waited out and then explained", async () => {
  vi.useFakeTimers();
  const { stream, sockets, handlers } = harness({ ok: false, status: 403 });
  const opening = stream.open();
  await vi.advanceTimersByTimeAsync(0);
  await vi.advanceTimersByTimeAsync(0);
  expect(handlers.onState).toHaveBeenCalledWith("connecting", "Waiting for that Hive to accept…");
  await vi.advanceTimersByTimeAsync(61_000);
  await opening;
  expect(handlers.onState).toHaveBeenCalledWith("closed", "That Hive did not accept the watch.");
  expect(sockets).toHaveLength(0);
  vi.useRealTimers();
});

test("the failure words are written for a person, not a status code", () => {
  expect(watchGrantFailure(429)).toBe("Too many windows are open right now.");
  expect(watchGrantFailure(503)).toBe("This Hive is restarting; try again in a moment.");
  expect(watchGrantFailure(418)).toBe("The window could not be opened (418).");
});

/** A frame too short to carry its own header is dropped, not parsed. */
test("a malformed frame is ignored rather than read out of whatever follows", async () => {
  const { stream, sockets, handlers } = harness();
  await stream.open();
  sockets[0].deliver(new Uint8Array([1, 2, 3]));
  sockets[0].deliver(new Uint8Array([2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0]));
  expect(handlers.onOutput).not.toHaveBeenCalled();
  expect(handlers.onSnapshot).not.toHaveBeenCalled();
});
