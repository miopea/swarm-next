import { afterEach, expect, test, vi } from "vitest";
import { fetchQueenRunHistory } from "./queenHistory";

afterEach(() => vi.unstubAllGlobals());

test("history uses private bounded transport with cancellation and no cache", async () => {
  const history = { retention_days: 30, max_retained: 4096, retained_count: 0, records: [] };
  const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify(history)));
  vi.stubGlobal("fetch", fetch);
  const signal = new AbortController().signal;
  await expect(fetchQueenRunHistory("fixture-token", signal)).resolves.toEqual(history);
  expect(fetch).toHaveBeenCalledWith("/api/v1/runtime/queen-history?limit=100", expect.objectContaining({
    signal, cache: "no-store", credentials: "same-origin",
  }));
  expect(fetch.mock.calls[0][1].headers.get("Authorization")).toBe("Bearer fixture-token");
});

test("unavailable history rejects instead of reporting zero finishes", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({ message: "unavailable" }), { status: 503 })));
  await expect(fetchQueenRunHistory("fixture-token", new AbortController().signal)).rejects.toThrow("503");
});
