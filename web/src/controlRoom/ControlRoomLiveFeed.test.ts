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

  expect(cursors).toEqual([0]);
  expect(states).toEqual(["connecting", "connected"]);
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
  expect(states).toEqual(["connecting", "retrying", "connected"]);
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

  expect(cursors).toEqual([0, 0]);
  expect(delay).toHaveBeenCalledWith(250, expect.any(AbortSignal));
});