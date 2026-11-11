# 0170 — GitHub's asks of the operator arrive as drafts

- Status: Accepted
- Date: 2026-11-11
- Decided by: the operator, in the Swarm Next worker's terminal on 2026-11-11.
  A teammate asked for their review on GitHub, and Swarm did not notice. They
  asked for Swarm to poll GitHub the way it polls Jira, on the repositories
  they can reach. Offered three scopes, they chose "Mine, on every repo". Asked
  what a review request should become, they chose a draft task for Queen.
- Extends: ADR 0115 (the check gate reads GitHub through the machine's gh
  login) and ADR 0126 (intake records where work came from).

## Context

Swarm already read GitHub in four ways: device sign-in, filing feedback, CI
for the check gate, and release discovery. It also took every open issue from
one opted-in repository as drafts. That intake is set in the environment, and
nothing reconciles it once an issue closes.

Nothing noticed work GitHub addressed to the operator personally. A review
request reached Swarm only if the operator imported GitHub's notification email
by hand.

The machine's gh login already reaches the operator's private repositories. On
this Hive it saw 60 repositories, with 104 open issues and pull requests between
them, two of which asked for the operator's review.

## Decision

1. **What arrives.** Every five minutes, and once at start, a pass makes two
   searches:
   - open pull requests where the operator's own review is requested
     (`user-review-requested:@me`);
   - open issues assigned to the operator (`assignee:@me`).

   Both cover every repository the login reaches. Team review requests are not
   included, because "mine" was the ruling. Taking every open issue stays
   opt-in for each repository, as before.
2. **The credential is the machine's gh login**, as for the check gate. Swarm
   stores no token for this. A Hive with no gh login reads nothing and says
   nothing.
3. **Each ask becomes one draft**, filed by the system with an intake record
   (ADR 0126). The record holds the link, the author as the requester, and a
   suggested worker. Titles read `Review owner/name#N: title` or
   `owner/name#N: title`.
   - If exactly one routable worker's checkout has that repository as its
     origin, the draft is filed in that checkout and that worker is suggested.
   - If several do, the draft names them all and suggests nobody, because
     choosing between them is routing, and routing is Queen's.
   - If none does, the draft holds `github://owner/name` until it is assigned.
4. **A review draft is a brief for review notes.** It asks the worker to read
   the pull request and write notes for the operator: what it changes, what
   could go wrong, and what to ask. The operator submits the review. The
   worker does not approve, comment or request changes on GitHub. The author's
   own text is fenced and labelled as their words, not instructions.
5. **One link per ask, keyed by GitHub's own URL**, built from the repository,
   kind and number. A table records the link (schema 225).
   - The link stops a second pass from filing the ask again.
   - The link outlives the ask's ending. A review requested again after the
     operator reviewed files a new draft if the old task is closed. If the old
     task is still open, the request is written into that task's history.
   - A draft Queen closed while GitHub still asks stays closed.
   - An issue that the whole-repository intake already filed is linked to
     that task, not filed twice, whichever intake reached it first.
6. **Only a direct read ends an ask.** Each pass reads directly at most ten
   linked asks that no longer appear in the searches, longest unchecked first.
   It starts no direct read once the pass has run a minute, and every gh call
   is capped at twenty seconds, so shutdown never waits long on it.
   - Missing from a search is not evidence by itself: search can lag.
   - A search GitHub marks incomplete, or one with more results than a page,
     ends nothing.
   - A 404 on a direct read is not an ending either, so a repository the
     login lost leaves its asks as they were.
7. **What an ending does** depends on the task's state:
   - a draft closes as abandoned, with the reason in its history: merged,
     closed, review no longer requested, or unassigned;
   - routed work (ready and later) is told in its history, and its state is
     left alone;
   - closed or removed work is left as it is.

   Ready counts as routed. It has a worker who may be starting it, so a close
   decided from a state read a moment earlier could land on work that has just
   gone active. A draft has nobody, and the close checks inside its own
   statement that the task is still a draft.
8. **Failure is bounded and reported when it matters.**
   - A failed pass waits 5 minutes, then 10, doubling to at most an hour. The
     first pass that succeeds resets the wait.
   - Each pass records how its read went. Once reads have worked on this Hive,
     a later failure shows on `/health` as a degraded subsystem with gh's
     reason: "GitHub review requests and assigned issues — They have stopped
     arriving: …". A Hive that never read GitHub stays quiet.
9. **One gh call path.** The check gate's `gh api` call moved into a shared
   helper, which names timeouts, missing gh, signed-out, 404 and rate limits
   once. The check gate's messages are unchanged.

## Consequences

- A review request reaches Queen's drafts within five minutes. When the
  operator reviews it, or the request is withdrawn, the draft closes by itself.
- On this Hive both pending review requests are on one repository, which two
  workers have checked out. Both drafts arrive naming both workers, and Queen
  picks one.
- Routed work learns about an ending only from its history. Nothing is sent to
  the worker. A worker preparing review notes will see the pull request's
  state when it reads it.
- Every Hive whose machine has a gh login gets this after updating, with no
  switch to turn it off. Whether a member Hive wants one is an open question
  (`docs/09-open-questions.md`).
- An ask whose repository stays unreadable is read again each pass in turn,
  ten at a time. Its draft stays as it was until the login can see it again or
  someone closes it.
- Not proven by the tests: a real ending observed on this Hive. The live test
  reads real GitHub, files the two real review requests into a board of its
  own, and confirms a second pass files nothing. No ending has been seen on a
  running Hive yet.
