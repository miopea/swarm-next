import { authenticatedFetch } from "./request";

/**
 * A review GitHub still asks of the operator, on work whose notes are written
 * (ADR 0175). It clears itself when GitHub stops asking.
 */
export type GithubReviewOwed = {
  task_id: string;
  /** GitHub's link to the pull request. */
  url: string;
  /** `owner/name#N`. */
  reference: string;
  title: string;
  /** Who asked, as GitHub named them. */
  requested_by: string | null;
  /** The worker that wrote the notes. */
  worker_name: string | null;
  /** The notes: the worker's handoff into Review. */
  notes: string | null;
  owed_since: number;
};

export async function fetchGithubReviewsOwed(operatorToken: string, signal?: AbortSignal): Promise<GithubReviewOwed[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/github/reviews-owed", { signal });
  return response.json() as Promise<GithubReviewOwed[]>;
}
