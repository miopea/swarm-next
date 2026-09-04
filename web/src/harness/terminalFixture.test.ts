import { expect, test } from "vitest";
import { FixtureWebSocket } from "./terminalFixture";
import { hiveFixture } from "./hiveFixture";

test("synthetic output preserves bytes and advances the snapshot sequence", async () => {
  const socket = new FixtureWebSocket("/fixture", ["swarm-terminal.v4"], false);
  const frames: Uint8Array[] = [];
  socket.addEventListener("message", (event) => {
    const data = (event as MessageEvent).data;
    if (typeof data !== "string") frames.push(new Uint8Array(data));
  });
  await Promise.resolve();
  socket.send(JSON.stringify({ type: "resume", rows: 24, columns: 36 }));
  await Promise.resolve();
  const initial = new DataView(frames[0].buffer).getBigUint64(1);
  const bytes = new Uint8Array([27, 91, 50, 75, 240, 159, 144, 157]);
  socket.emitOutput(bytes);
  socket.emitOutput(bytes);
  expect(frames.slice(1).map((frame) => new DataView(frame.buffer).getBigUint64(1))).toEqual([initial + 1n, initial + 2n]);
  expect(Array.from(frames[1].subarray(9))).toEqual(Array.from(bytes));
  expect(() => socket.emitOutput(new Uint8Array(65_537))).toThrow("bound");
  socket.close();
  expect(() => socket.emitOutput(bytes)).toThrow("not attached");
});

test("the visual harness advertises the controlled browser protocol", () => {
  expect(hiveFixture("/api/v1/terminal/sessions/fixture/attach-grants")).toMatchObject({ protocol: "swarm-terminal.v4" });
});

test("passive fixture refuses stale claims, then publishes an explicit handoff", async () => {
  const socket = new FixtureWebSocket("/fixture", ["swarm-terminal.v4"], false);
  const messages: Record<string, unknown>[] = [];
  const frames: ArrayBuffer[] = [];
  socket.addEventListener("message", (event) => {
    const data = (event as MessageEvent).data;
    if (typeof data === "string") messages.push(JSON.parse(data));
    else frames.push(data);
  });
  await Promise.resolve();
  socket.send(JSON.stringify({ type: "resume", rows: 24, columns: 80 }));
  await Promise.resolve();
  expect(messages.at(-1)).toMatchObject({ type: "state", control: { owned: false, generation: "1" } });
  socket.send(JSON.stringify({ type: "claim", observed_generation: null, rows: 30, columns: 40 }));
  await Promise.resolve();
  expect(messages.at(-1)).toMatchObject({ type: "control", control: { owned: false } });
  expect(frames).toHaveLength(1);
  socket.send(JSON.stringify({ type: "claim", observed_generation: "1", rows: 30, columns: 40 }));
  await Promise.resolve();
  expect(messages.at(-1)).toMatchObject({ type: "control", control: { owned: true, generation: "2" } });
  expect(new DataView(frames.at(-1)!).getUint16(11)).toBe(40);
  socket.send(JSON.stringify({ type: "probe", request_id: "return" }));
  await Promise.resolve();
  expect(messages.at(-1)).toEqual({ type: "alive", request_id: "return" });
  expect(frames).toHaveLength(2);
  socket.close();
});

test("closing before scheduled open cannot revive a fixture socket", async () => {
  const socket = new FixtureWebSocket("/fixture");
  socket.close();
  await Promise.resolve();
  expect(socket.readyState).toBe(FixtureWebSocket.CLOSED);
});
