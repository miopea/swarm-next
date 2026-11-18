# 0175 — A GitHub review the operator owes stays in Needs you

- Status: Accepted
- Date: 2026-11-17
- Decided by: the Swarm Next worker, on a task Queen routed. The task left the
  shape to the worker. It offered two: keep the task open, or settle it into
  something the operator sees as theirs until GitHub stops asking.
- Amends: ADR 0170 (GitHub's asks of the operator arrive as drafts).
- Amended by: ADR 0176. Points 4 and 5 no longer hold: the operator submits
  the worker's proposed review from the card, and the press can merge.

## Context

ADR 0170 turns a pull request that asks for the operator's review into a draft.
Queen routes it, and a worker writes review notes. The operator submits the
review on GitHub. Its Consequences said: "When the operator reviews it, or the
request is withdrawn, the draft closes by itself."

That is not what happened. The worker moves the notes to Review with no
commits. The coordinator settles no-commit work at once ("nothing to deploy"),
so the task completed the moment the notes were written. On this Hive, all
three review requests went that way:

- two pull requests were merged without a review;
- the third was still waiting for the operator's review a day later, on a
  completed task.

The board's only sign that a review was owed was an open task. Once the task
closed, nothing showed it.

Holding the task open in Review was rejected. Review is Queen's queue, so she
would be asked to judge work that is finished and waits on the operator. She
could also close it on the same no-commit grounds, and the failure would come
back one step later.

## Decision

1. **The task settles as before.** The worker's job is the notes, and once
   written they are done. Nothing about settling changes.
2. **The review is owed separately.** A review is owed while all of these hold:
   - GitHub still asks for it: the link is a review request and has not
     ended;
   - the notes are written: the task is in Review, Awaiting release or
     Completed;
   - the task has not been removed.

   A draft, or work still being done, is not yet the operator's to act on. A
   task Queen abandoned stays closed, as ADR 0170 point 5 says, and is not
   owed.
3. **Each owed review is one Needs you item.** It names the pull request, who
   asked, the worker who wrote the notes, and the notes themselves (the
   worker's handoff into Review). It links to the pull request on GitHub and
   to the task. It is counted in the badge and sent as a push, like every
   other Needs you item.
4. **It has no button.** It clears itself when GitHub stops asking: the
   operator reviews, the request is withdrawn, or the pull request is merged
   or closed. The ending pass of ADR 0170 records that within about five
   minutes. That pass already marks an ask ended whatever state its task is
   in, so a review that ends after its task closed clears the same way.
5. **Swarm still posts nothing on GitHub.** The operator submits the review.
6. **Schema 229** widens the kinds a Needs you push may carry to include
   `github_review`. It adds no table.

## Consequences

- A review the operator owes stays visible until GitHub says it is done,
  wherever its task is.
- A review request already completed and still waiting when a Hive updates
  shows in Needs you at once. It is not pushed: it is older than the
  operator's last look at Needs you, and a push is only for what arrived
  since. A review owed after the update is pushed as usual.
- ADR 0170 point 7 is unchanged. An ending still writes nothing into a closed
  task's history; the Needs you item is what changes.
- If the login loses access to a repository, its asks stay standing (ADR 0170
  point 6), so their items stay too until access returns or the request ends.
  There is no dismiss button: GitHub decides when a review is no longer owed,
  and the operator can withdraw their own request there.
- Assigned issues are not covered. An assigned issue asks the operator to do
  work, not to give a verdict, and the worker that takes it does the work. If
  issues need the same treatment, that is a separate decision.
- Not proven by the tests: a real review submitted on GitHub, and the item
  clearing on a running Hive.
