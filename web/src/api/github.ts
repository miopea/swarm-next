import { authenticatedFetch } from "./request";

export type GithubReviewVerdict = "approve" | "request_changes";

/** The review the worker suggests the operator submit (ADR 0176). */
export type GithubReviewProposal = {
  verdict: GithubReviewVerdict;
  /** Written to the pull request's author. The operator may edit it. */
  comment: string;
  /** The commit the worker read. The review is pinned to it. */
  head_sha: string;
  proposed_at: number;
};

export type GithubReviewSubmissionState =
  | "queued" | "posting" | "post_uncertain" | "failed"
  | "awaiting_merge" | "merging" | "merge_uncertain" | "merged" | "done";

/** Where the operator's last press on this review stands, while it is still owed. */
export type GithubReviewSubmission = {
  id: number;
  state: GithubReviewSubmissionState;
  /** Why it failed, or what Swarm is checking. In GitHub's words when it gave them. */
  outcome: string | null;
};

/**
 * A review GitHub still asks of the operator, on work whose notes are written
 * (ADR 0175). It clears itself when GitHub stops asking, or once the operator
 * submits it from the card (ADR 0176).
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
  /** Null when the worker recorded no review to submit. */
  proposal: GithubReviewProposal | null;
  submission: GithubReviewSubmission | null;
};

/** Where one press ended up, and why, in words for the operator. */
export type GithubReviewSubmitted = {
  state: GithubReviewSubmissionState;
  outcome: string | null;
};

export async function fetchGithubReviewsOwed(operatorToken: string, signal?: AbortSignal): Promise<GithubReviewOwed[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/github/reviews-owed", { signal });
  return response.json() as Promise<GithubReviewOwed[]>;
}

/**
 * Posts the review under this machine's GitHub login, then merges when GitHub
 * allows it and the verdict is approve (ADR 0176). Answers once GitHub has.
 */
export async function submitGithubReview(
  operatorToken: string,
  taskId: string,
  review: { verdict: GithubReviewVerdict; comment: string },
): Promise<GithubReviewSubmitted> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/integrations/github/reviews-owed/${encodeURIComponent(taskId)}/submit`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(review),
    },
  );
  return response.json() as Promise<GithubReviewSubmitted>;
}
