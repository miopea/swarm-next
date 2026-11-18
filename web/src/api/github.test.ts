import { afterEach, expect, test, vi } from "vitest";

import { fetchGithubReviewsOwed, submitGithubReview } from "../api";

afterEach(() => vi.unstubAllGlobals());

test("reads the reviews the operator owes, with their token", async () => {
  const owed = [{
    task_id: "task/one",
    url: "https://github.com/acme/petal/pull/42",
    reference: "acme/petal#42",
    title: "Review acme/petal#42: Make the petals bloom",
    requested_by: "sorrel",
    worker_name: "Petal",
    notes: "Ask why the stem is renamed.",
    owed_since: 1_792_700_000,
    proposal: null,
    submission: null,
  }];
  const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify(owed), { status: 200, headers: { "Content-Type": "application/json" } }));
  vi.stubGlobal("fetch", fetch);

  await expect(fetchGithubReviewsOwed("operator")).resolves.toEqual(owed);
  expect(fetch).toHaveBeenCalledWith("/api/v1/integrations/github/reviews-owed", expect.any(Object));
  const init = fetch.mock.calls[0][1] as RequestInit;
  expect(new Headers(init.headers).get("Authorization")).toBe("Bearer operator");
});

test("submits the operator's verdict and edited words, and returns where it ended up", async () => {
  const answer = { state: "merged", outcome: "Approved and merged." };
  const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify(answer), { status: 200, headers: { "Content-Type": "application/json" } }));
  vi.stubGlobal("fetch", fetch);

  await expect(submitGithubReview("operator", "task/one", { verdict: "approve", comment: "Looks right." })).resolves.toEqual(answer);
  expect(fetch).toHaveBeenCalledWith("/api/v1/integrations/github/reviews-owed/task%2Fone/submit", expect.any(Object));
  const init = fetch.mock.calls[0][1] as RequestInit;
  expect(init.method).toBe("POST");
  expect(JSON.parse(init.body as string)).toEqual({ verdict: "approve", comment: "Looks right." });
  expect(new Headers(init.headers).get("Authorization")).toBe("Bearer operator");
});

test("a refused press carries the server's reason", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(
    JSON.stringify({ code: "github_review_in_flight", message: "This review is already being submitted" }),
    { status: 409 },
  )));
  await expect(submitGithubReview("operator", "task-1", { verdict: "approve", comment: "" }))
    .rejects.toThrow("This review is already being submitted");
});
