import { afterEach, expect, test, vi } from "vitest";
import { submitSupport, type SupportSubmission } from "./support";

afterEach(() => vi.unstubAllGlobals());
const input: SupportSubmission = {
  submission_key: "11111111-1111-4111-8111-111111111111", kind: "bug_report",
  email: "fictional@example.invalid", name: null, subject: "Fictional report", body: "Reviewed words",
};

test.each(["pending", "delivering", "uncertain", "failed", "confirmed", "rate_limited"])(
  "an exact replay acknowledges the saved %s report without sending another", async (state) => {
    const saved = { submission_key: input.submission_key, created_at: 1, delivery: {
      state, attempts: 1, retry_not_before: state === "rate_limited" ? 120 : null,
    } };
    const fetcher = vi.fn().mockResolvedValue(new Response(JSON.stringify(saved)));
    vi.stubGlobal("fetch", fetcher);
    expect(await submitSupport("fixture", input)).toEqual(saved);
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(JSON.parse(fetcher.mock.calls[0][1].body)).toEqual(input);
  },
);

test.each([
  { submission_key: "different-key", delivery: { state: "pending" } },
  { submission_key: input.submission_key, delivery: { state: "invented" } },
  { submission_key: input.submission_key },
])("does not confirm unrelated or invalid save evidence", async (response) => {
  const fetcher = vi.fn().mockResolvedValue(new Response(JSON.stringify(response)));
  vi.stubGlobal("fetch", fetcher);
  await expect(submitSupport("fixture", input)).rejects.toThrow("Support save could not be confirmed");
  expect(fetcher).toHaveBeenCalledTimes(1);
});
