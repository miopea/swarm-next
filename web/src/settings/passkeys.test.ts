import { expect, test, vi, beforeEach } from "vitest";

import { passkeysSupported, signInWithPasskey } from "./passkeys";

beforeEach(() => {
  vi.restoreAllMocks();
});

test("reports whether this browser can use passkeys at all", () => {
  const original = "PublicKeyCredential" in window;
  expect(passkeysSupported()).toBe(original);
});

/**
 * The sign-in path is deliberately unauthenticated — it is the door — and a
 * Hive with no passkey registered must say so rather than failing opaquely at
 * the browser prompt.
 */
test("says plainly when no passkey is registered for this address", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => new Response("", { status: 404 })));
  await expect(signInWithPasskey()).rejects.toThrow(/No passkey is registered for this address/);
});

test.each([
  [429, /Wait a few minutes, or sign in with your token/],
  [503, /temporarily unavailable.*sign in with your token/],
])("explains passkey start refusal %s without retrying or opening a device prompt", async (status, message) => {
  const request = vi.fn(async () => new Response("", { status }));
  const get = vi.fn();
  vi.stubGlobal("fetch", request);
  vi.stubGlobal("navigator", { credentials: { get } });
  await expect(signInWithPasskey()).rejects.toThrow(message);
  expect(request).toHaveBeenCalledTimes(1);
  expect(get).not.toHaveBeenCalled();
});

test("does not swallow a rejected passkey", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) =>
      String(url).includes("/start")
        ? new Response(
            JSON.stringify({
              challenge_id: "c1",
              options: { publicKey: { challenge: "AAAA", allowCredentials: [] } },
            }),
            { status: 200, headers: { "content-type": "application/json" } },
          )
        : new Response("", { status: 401 }),
    ),
  );
  vi.stubGlobal("navigator", {
    ...navigator,
    credentials: {
      get: vi.fn(async () => ({
        id: "abc",
        type: "public-key",
        rawId: new Uint8Array([1, 2, 3]).buffer,
        response: {
          clientDataJSON: new Uint8Array([1]).buffer,
          authenticatorData: new Uint8Array([2]).buffer,
          signature: new Uint8Array([3]).buffer,
          userHandle: null,
        },
      })),
    },
  });
  await expect(signInWithPasskey()).rejects.toThrow(/was not accepted/);
});
