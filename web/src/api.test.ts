import { expect, test, vi } from "vitest";

import { recoverTransientRuntime, RuntimeRequestError } from "./api";

test("recovers a saved browser session after bounded gateway failures", async () => {
  const operation = vi.fn()
    .mockRejectedValueOnce(new RuntimeRequestError(502, "gateway switching"))
    .mockRejectedValueOnce(new TypeError("network unavailable"))
    .mockResolvedValue("restored");

  await expect(recoverTransientRuntime(operation, [0, 0])).resolves.toBe("restored");
  expect(operation).toHaveBeenCalledTimes(3);
});

test("does not retry invalid credentials", async () => {
  const operation = vi.fn().mockRejectedValue(new RuntimeRequestError(401, "unauthorized"));

  await expect(recoverTransientRuntime(operation, [0, 0])).rejects.toMatchObject({ status: 401 });
  expect(operation).toHaveBeenCalledOnce();
});

test("stops after the bounded recovery budget", async () => {
  const operation = vi.fn().mockRejectedValue(new RuntimeRequestError(503, "runtime unavailable"));

  await expect(recoverTransientRuntime(operation, [0, 0])).rejects.toMatchObject({ status: 503 });
  expect(operation).toHaveBeenCalledTimes(3);
});
