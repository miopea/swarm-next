import { afterEach, expect, test, vi } from "vitest";
import { authenticatedFetch } from "../api/request";
import { recordOperatorSubmission } from "./OperatorSubmission";

vi.mock("../api/request", () => ({ authenticatedFetch: vi.fn() }));
afterEach(() => vi.resetAllMocks());

test("records exact authored text with the owned signal and validates identity", async () => {
  const controller = new AbortController();
  vi.mocked(authenticatedFetch).mockImplementation(async (_token, _url, options) => {
    const body = JSON.parse(options!.body as string);
    expect(body.text).toBe(" Exact\n🐝 ");
    expect(options!.signal).toBe(controller.signal);
    expect(options!.method).toBe("POST");
    return new Response(JSON.stringify({ id: body.id, source: "operator_authored", provider_consumption: "unconfirmed" }));
  });
  await recordOperatorSubmission("token", "worker/session", " Exact\n🐝 ", controller.signal);
  expect(authenticatedFetch).toHaveBeenCalledTimes(1);
  expect(vi.mocked(authenticatedFetch).mock.calls[0][1]).toBe("/api/v1/terminal/sessions/worker%2Fsession/submissions");
});

test("mismatched acknowledgements and failed writes are not retried or treated as delivery", async () => {
  vi.mocked(authenticatedFetch).mockResolvedValue(new Response(JSON.stringify({ id: "wrong", source: "operator_authored", provider_consumption: "confirmed" })));
  await expect(recordOperatorSubmission("token", "session", "text", new AbortController().signal)).rejects.toThrow();
  expect(authenticatedFetch).toHaveBeenCalledTimes(1);
  vi.mocked(authenticatedFetch).mockRejectedValue(new Error("unavailable"));
  await expect(recordOperatorSubmission("token", "session", "text", new AbortController().signal)).rejects.toThrow();
  expect(authenticatedFetch).toHaveBeenCalledTimes(2);
});
