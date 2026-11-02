import { expect, test, vi } from "vitest";

import { ControlRoomLiveFeed, type LiveFeedState } from "./ControlRoomLiveFeed";

test("resumes from the durable cursor and invalidates snapshots", async () => {
  const cursors: number[] = [];
  const states: LiveFeedState[] = [];
  const invalidated = vi.fn();
  const feed = new ControlRoomLiveFeed(async (_token, after) => {
    cursors.push(after);
    return {
      events: [{ sequence: 7, hive_id: "hive-1", kind: "tasks_changed", occurred_at: 1 }],
      next_cursor: 7,
      reset_required: false,
    };
  });
  invalidated.mockImplementation(async () => feed.stop());

  feed.start("secret", invalidated, (state) => states.push(state));
  await vi.waitFor(() => expect(invalidated).toHaveBeenCalledOnce());

  expect(cursors).toEqual([0, 0]); // A task change is held once and read again from the same cursor.
  expect(states).toEqual(["connecting"]); // Stopped inside invalidation; no late Connected.
});

test("retries with capped owned backoff and refreshes after a cursor reset", async () => {
  const delay = vi.fn(async () => undefined);
  const states: LiveFeedState[] = [];
  const invalidated = vi.fn();
  let calls = 0;
  const feed = new ControlRoomLiveFeed(
    async () => {
      calls += 1;
      if (calls === 1) throw new Error("temporary network loss");
      return { events: [], next_cursor: 0, reset_required: true };
    },
    delay,
  );
  invalidated.mockImplementation(async () => feed.stop());

  feed.start("secret", invalidated, (state) => states.push(state));
  await vi.waitFor(() => expect(invalidated).toHaveBeenCalledOnce());

  expect(delay).toHaveBeenCalledWith(250, expect.any(AbortSignal));
  expect(states).toEqual(["connecting", "retrying"]);
});
test("does not acknowledge an event until snapshot invalidation succeeds", async () => {
  const cursors: number[] = [];
  const delay = vi.fn(async () => undefined);
  const invalidated = vi.fn();
  let invalidations = 0;
  const feed = new ControlRoomLiveFeed(
    async (_token, after) => {
      cursors.push(after);
      return {
        events: [{ sequence: 9, hive_id: "hive-1", kind: "workers_changed", occurred_at: 1 }],
        next_cursor: 9,
        reset_required: false,
      };
    },
    delay,
  );
  invalidated.mockImplementation(async () => {
    invalidations += 1;
    if (invalidations === 1) throw new Error("snapshot temporarily unavailable");
    feed.stop();
  });

  feed.start("secret", invalidated);
  await vi.waitFor(() => expect(invalidated).toHaveBeenCalledTimes(2));

  // Each attempt is held once and read again from the same cursor; none of
  // them moves past the event the failed refresh did not apply.
  expect(cursors).toEqual([0, 0, 0, 0]);
  expect(delay).toHaveBeenCalledWith(250, expect.any(AbortSignal));
});
test("reconnecting cancels old invalidation and prevents a late Connected state", async () => {
  let oldSignal: AbortSignal | undefined;
  let finish!: () => void;
  const gate = new Promise<void>((resolve) => { finish = resolve; });
  const states: LiveFeedState[] = [];
  const feed = new ControlRoomLiveFeed(async () => ({ events: [], next_cursor: 0, reset_required: true }));
  feed.start("operator", async (_page, signal) => { oldSignal = signal; await gate; }, (state) => states.push(state));
  await vi.waitFor(() => expect(oldSignal).toBeDefined());
  feed.start("operator", async () => { feed.stop(); }, (state) => states.push(state));
  expect(oldSignal!.aborted).toBe(true);
  finish();
  await gate;
  await vi.waitFor(() => expect(states).toEqual(["connecting", "connecting"]));
  feed.stop();
});

test("settles ordinary worker and task bursts before rebuilding the snapshot", async () => {
  let release!: () => void;
  const settle = new Promise<void>((resolve) => { release = resolve; });
  const delay = vi.fn(() => settle);
  const invalidated = vi.fn();
  const feed = new ControlRoomLiveFeed(async () => ({
    events: [
      { sequence: 7, hive_id: "hive-1", kind: "workers_changed", occurred_at: 1 },
      { sequence: 8, hive_id: "hive-1", kind: "tasks_changed", occurred_at: 1 },
    ],
    next_cursor: 8,
    reset_required: false,
  }), delay);
  invalidated.mockImplementation(async () => feed.stop());

  feed.start("secret", invalidated);
  await vi.waitFor(() => expect(delay).toHaveBeenCalledWith(250, expect.any(AbortSignal)));
  expect(invalidated).not.toHaveBeenCalled();
  release();
  await vi.waitFor(() => expect(invalidated).toHaveBeenCalledOnce());
});

test("changes that land while a burst is held cost one refresh, read from the same cursor", async () => {
  const cursors: number[] = [];
  const delay = vi.fn(async () => undefined);
  const invalidated = vi.fn();
  const worker = (sequence: number) => ({ sequence, hive_id: "hive-1", kind: "workers_changed" as const, occurred_at: 1 });
  const feed = new ControlRoomLiveFeed(async (_token, after) => {
    cursors.push(after);
    // The first read sees one change; three more land during the hold.
    return cursors.length === 1
      ? { events: [worker(7)], next_cursor: 7, reset_required: false }
      : { events: [worker(7), { ...worker(8), kind: "tasks_changed" as const }, worker(9), worker(10)], next_cursor: 10, reset_required: false };
  }, delay);
  invalidated.mockImplementation(async () => feed.stop());

  feed.start("secret", invalidated);
  await vi.waitFor(() => expect(invalidated).toHaveBeenCalledOnce());

  expect(cursors).toEqual([0, 0]);
  expect(invalidated.mock.calls[0][0].events.map((event: { sequence: number }) => event.sequence)).toEqual([7, 8, 9, 10]);
});

test("worker and task refreshes keep a minimum gap, and a decision is not held by it", async () => {
  let clock = 0;
  const delay = vi.fn(async (milliseconds: number) => { clock += milliseconds; });
  const pages = [
    { events: [{ sequence: 1, hive_id: "hive-1", kind: "workers_changed" as const, occurred_at: 1 }], next_cursor: 1, reset_required: false },
    { events: [{ sequence: 2, hive_id: "hive-1", kind: "tasks_changed" as const, occurred_at: 1 }], next_cursor: 2, reset_required: false },
    { events: [{ sequence: 3, hive_id: "hive-1", kind: "decisions_changed" as const, occurred_at: 1 }], next_cursor: 3, reset_required: false },
  ];
  const cursors: number[] = [];
  const refreshedAt: number[] = [];
  const feed = new ControlRoomLiveFeed(async (_token, after) => {
    cursors.push(after);
    // The next change arrives 300 ms after each refresh began.
    if (refreshedAt.length > 0 && clock < refreshedAt.at(-1)! + 300) clock = refreshedAt.at(-1)! + 300;
    return pages.find((page) => page.next_cursor > after)!;
  }, delay, 35_000, () => clock);
  const invalidated = vi.fn(async () => {
    refreshedAt.push(clock);
    if (refreshedAt.length === 3) feed.stop();
  });

  feed.start("secret", invalidated);
  await vi.waitFor(() => expect(invalidated).toHaveBeenCalledTimes(3));

  // The first burst settles for the window; the second waits out the rest of
  // the gap since the first refresh; the decision goes at once.
  expect(delay.mock.calls.map(([milliseconds]) => milliseconds)).toEqual([250, 700]);
  expect(refreshedAt).toEqual([250, 1_250, 1_550]);
  expect(cursors).toEqual([0, 0, 1, 1, 2]);
});

test.each(["decisions_changed", "runtime_changed", "sessions_changed", "presence_changed", "notifications_changed"] as const)(
  "%s remains an immediate invalidation",
  async (kind) => {
    const delay = vi.fn(async () => undefined);
    const invalidated = vi.fn();
    const feed = new ControlRoomLiveFeed(async () => ({
      events: [{ sequence: 7, hive_id: "hive-1", kind, occurred_at: 1 }],
      next_cursor: 7,
      reset_required: false,
    }), delay);
    invalidated.mockImplementation(async () => feed.stop());

    feed.start("secret", invalidated);
    await vi.waitFor(() => expect(invalidated).toHaveBeenCalledOnce());
    expect(delay).not.toHaveBeenCalled();
  },
);

test("a poll that never answers cannot wedge the feed", async () => {
  // Reported from a phone: the roster kept showing every worker resting while
  // they were plainly working, kicking them changed nothing, and the pill still
  // read Connected. A backgrounded mobile tab can leave an in-flight fetch that
  // never settles — it does not resolve and it does not reject — so the loop
  // waited on it forever while the last state it published was "connected".
  //
  // The server holds a poll for at most twenty seconds, so anything past the
  // ceiling is a hang rather than a slow answer.
  const states: LiveFeedState[] = [];
  const delay = vi.fn(async () => undefined);
  const invalidated = vi.fn();
  let attempt = 0;
  const feed = new ControlRoomLiveFeed(
    async (_token, _after, signal) => {
      attempt += 1;
      if (attempt === 1) {
        // Never settles on its own, exactly like the suspended fetch.
        return new Promise((_resolve, reject) => {
          signal.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")));
        });
      }
      return { events: [], next_cursor: 0, reset_required: true };
    },
    delay,
    10,
  );
  invalidated.mockImplementation(async () => feed.stop());

  feed.start("secret", invalidated, (state) => states.push(state));
  await vi.waitFor(() => expect(invalidated).toHaveBeenCalledOnce());

  // It gave up on the hung poll and got a real answer on the next one, rather
  // than sitting on "connected" forever.
  expect(attempt).toBe(2);
  expect(states).toEqual(["connecting", "retrying"]);
});
