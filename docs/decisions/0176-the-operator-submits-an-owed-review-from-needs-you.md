# 0176 — The operator submits an owed review from Needs you

- Status: Accepted
- Date: 2026-11-17
- Decided by: the operator, in the Swarm Next worker's terminal, on seeing the
  first owed-review card: "Why would it link to GH and ask me to write
  comments when I had the AI do the work?" and "I should just approve it, add
  the comment approve and merge". Asked how, they chose the worker's questions
  as an editable comment, and one press that approves and merges when GitHub
  allows it.
- Amends: ADR 0170 point 4 (the operator submits the review on GitHub; the
  worker does not post) and ADR 0175 points 4 and 5 (no button; Swarm posts
  nothing).

## Context

ADR 0175 put a review the operator owes in Needs you, with a link to GitHub.
The first one to appear showed the worker's notes and sent the operator to
GitHub to write the review themselves. The worker had already read the pull
request and decided what to ask. Writing it again by hand is the work the
operator had handed off.

The rule it replaces had one reason: a review posts under the operator's
GitHub name, so nothing should go out that they have not seen. A button keeps
that. Nothing posts until they press it, and they see the exact words first.

## Decision

1. **The worker proposes the review; it never posts one.** A new worker tool,
   `swarm_propose_github_review`, records on the worker's own review task:
   - the suggested verdict: approve or request changes;
   - the comment, written to the pull request's author;
   - the full commit the worker read.

   A later proposal replaces an earlier one. The draft's brief asks for it.
   The brief's text is added where Swarm files the draft, so the worker engine
   does not change.
2. **The card shows what will be posted.** It shows the suggested verdict and
   the comment in a box the operator can edit. The buttons are:
   - **Approve and merge**;
   - **Request changes**, which needs a comment.

   The suggested verdict's button comes first and is the primary one. The
   worker's notes fold beneath. They are the longer of the worker's last note
   and its handoff into Review, so a handoff that only points at the notes no
   longer replaces them.

   With no proposal recorded, the card shows the notes and the GitHub link, and
   says the worker recorded no review to submit.
3. **One press posts the review, then merges when GitHub allows it.**
   - The review posts under the machine's gh login, pinned to the commit the
     worker read.
   - Approve then merges only when all of these hold:
     - the pull request is not a draft;
     - its head is still the commit the worker read, so nothing unread is
       merged;
     - it has at least one check, and every check passed (the check gate's
       rule, ADR 0115);
     - GitHub says it can be merged.
   - The merge uses a merge commit when the repository allows one, else a
     squash, else a rebase.
   - Otherwise the review stands, nothing is merged, and the card says why.
     Request changes never merges.
   - The rule lives in the API beside the GitHub calls, not in the domain
     crate. A domain change rebuilds every worker's engine, and this rule
     concerns the API alone.
4. **The submission is recorded before GitHub is called.**
   - It moves through queued, posting, posted, then merging and merged.
     It can also end failed or uncertain.
   - One submission is in flight per ask, so a second press is refused.
   - A refusal from GitHub is shown in GitHub's words, and the operator can
     press again.
   - A call with an unknown outcome is never repeated blind. Swarm reads the
     pull request's reviews, and whether it merged, to settle it, as ADR 0173
     does for messages. A post found under the login, on the commit, with the
     verdict, is the one Swarm sent. None found means it never landed, and the
     operator may press again.
   - A restart marks a call that was under way as uncertain. Each five-minute
     GitHub pass settles what is uncertain, and tries a merge an approval left
     waiting, before it reads new asks.
5. **A posted review leaves Needs you at once.** GitHub stops requesting the
   review, and the ending pass ends the ask as before. In its place the card
   says what happened, for example "Approved and merged." or "Approved. Not
   merged: it is still a draft…", until the operator dismisses it. That
   result is kept in the page, not the database.
6. **Swarm still writes nothing to GitHub by itself.** Only the operator's
   press posts or merges. The worker's tool records a proposal and nothing
   else.
7. **Schema 230** adds the proposal and submission tables. The worker tool
   surface moves to revision 43.

## Consequences

- The operator reads the worker's comment, edits it if they want, and presses
  once. They no longer write a review by hand.
- Merging is the operator's press, so it can start the repository's own
  deploy. The card names the merge in the button.
- A review owed before this change has no proposal. It shows the notes and
  the GitHub link until its worker records one.
- A worker session that connected before the update cannot call the new tool
  until it reconnects. The revision is what says so.
- A merge whose answer was lost is settled by the next pass. The card that
  reported it said Swarm was checking; the settled answer shows on the pull
  request, not on a card, since the review had already left Needs you.
- The press waits for GitHub: up to five calls of twenty seconds each, though
  each normally answers in about a second.
- Not proven by the tests: a real review and merge on GitHub, which the tests
  replace with a fake. The first real press is that proof.
