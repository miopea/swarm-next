import { afterEach, expect, test, vi } from "vitest";

import { fetchPresence, observePresence, setManualPresence } from "./presence";

const presence = { mode: "away" as const, manual_mode: null, source: "inactive_device" as const };

afterEach(() => vi.unstubAllGlobals());

test("owns the complete presence HTTP contract behind the shared transport", async () => {
  const fetch = vi.fn().mockImplementation(() => Promise.resolve(
    new Response(JSON.stringify(presence), { status: 200, headers: { "content-type": "application/json" } }),
  ));
  vi.stubGlobal("fetch", fetch);

  await expect(fetchPresence("operator-token")).resolves.toEqual(presence);
  await expect(setManualPresence("operator-token", "night_watch")).resolves.toEqual(presence);
  await expect(observePresence("operator-token", "phone/one", "mobile", "hidden")).resolves.toEqual(presence);

  expect(fetch).toHaveBeenNthCalledWith(1, "/api/v1/presence", expect.objectContaining({
    cache: "no-store",
    credentials: "same-origin",
  }));
  expect(fetch).toHaveBeenNthCalledWith(2, "/api/v1/presence", expect.objectContaining({
    method: "PUT",
    body: JSON.stringify({ manual_mode: "night_watch" }),
  }));
  expect(fetch).toHaveBeenNthCalledWith(3, "/api/v1/presence/devices/phone%2Fone", expect.objectContaining({
    method: "PUT",
    body: JSON.stringify({ device_class: "mobile", state: "hidden" }),
  }));
  for (const [, init] of fetch.mock.calls) {
    expect((init.headers as Headers).get("Authorization")).toBe("Bearer operator-token");
  }
});
