import { afterEach, expect, test, vi } from "vitest";

import { fetchGithubReviewsOwed } from "../api";

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
  }];
  const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify(owed), { status: 200, headers: { "Content-Type": "application/json" } }));
  vi.stubGlobal("fetch", fetch);

  await expect(fetchGithubReviewsOwed("operator")).resolves.toEqual(owed);
  expect(fetch).toHaveBeenCalledWith("/api/v1/integrations/github/reviews-owed", expect.any(Object));
  const init = fetch.mock.calls[0][1] as RequestInit;
  expect(new Headers(init.headers).get("Authorization")).toBe("Bearer operator");
});
