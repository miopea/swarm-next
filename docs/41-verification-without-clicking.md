# Verification without clicking

The design settled with the operator on 2026-08-30, for task `01a050c0`. It
replaces the framing that task was filed under, and the reason is in the first
section: the premise the task argued from turned out to be wrong.

## What the complaint turned out to be

The task was filed as *"either the system needs to work correctly or we need to
remove the verification confirmation"*, on figures that read as a leak: 49 of 355
completed tasks carrying nothing anyone verified, none of them sitting anywhere a
person would see.

Both halves of that dissolved on inspection.

**The count was wrong, and Queen corrected it against herself.** 31, not 49. The
query counted unapproved exemption claims without excluding tasks that *also*
carried a deployment record, so 18 properly closed, fully evidenced tasks were
counted as unverified because a stale claim sat beside real evidence. Those 18
are untidy, not unsafe. The 31 with no evidence at all were real, and were
written off through `record_task_unverifiable` the same evening — 31 written off,
0 unaccounted.

**The gate was never leaking.** `require_completion_evidence` refuses completion
unless `completion_evidence(task).closes_a_task()`, which is `Deployed` or
`ExemptionApproved` and deliberately **not** `ExemptionClaimed` — a worker
asserting its own work needs no evidence cannot also be the approval of that
claim. Reviewed work carrying a deployment already closes with no human at all
(`complete_reviewed_work_with_deployment`); only the remainder reaches
`reviewed_work_awaiting_judgment`.

So the mechanism is sound and what is left is **effort**. That rules out removing
the confirmation: you do not tear out a working gate to save taps. Operator's
answer, asked directly: *purely effort*.

## The two facts that shaped the design

Both were checked rather than assumed, and both closed off the obvious approach.

**Tasks hold no commits.** There is no commit field on a task, anywhere. So
"docs-only" is not derivable from anything Swarm knows — without new plumbing it
could only ever be *asserted* by the worker, which is the thing that rotted.

**`Completed` is the only terminal state.** Draft → Ready → Active → Blocked →
Review → Completed. Abandoned work has nowhere to go but Completed-with-an-
exemption, or Blocked forever. It was being made to answer a deployment question
that never applied to it.

## What was decided

### Work that never needed deployment evidence

Three categories, and they are handled differently on purpose:

| Category | How it is settled |
| --- | --- |
| Investigation — nothing was built | Derived: no commits attributed to the task |
| Docs-only changes | Derived: every attributed commit touches docs paths only |
| Abandoned or superseded | **Not settled at all** — it gets its own terminal state |

The third is the one worth noticing. Abandoned work is not completed work needing
an exemption, so it does not need a cheaper exemption — it needs a different
outcome. **This deletes a category of clicking rather than automating it**, and it
stops the completed count including things nobody finished.

### Commits become a recorded fact

Workers report the SHAs their task produced; Swarm reads the repository and
checks the commits exist, are reachable, and touch what was claimed. This is what
makes "nothing to deploy" **derived rather than asserted**, so it cannot rot the
way the 18 claims did.

A worker can still lie about *which* commits are its own. It cannot lie about
what those commits contain, and containment is the entire docs-only question.

**Attribution by time window was rejected.** A worker interleaves several tasks
in one session, and this repository's own history is full of exactly that. Time
attribution would misattribute constantly while looking objective — a check that
appears authoritative and is often wrong is worse than no check.

### Verify once, at report time, and store the verdict

Evidence is a **snapshot of what was true when it was checked**, not a live query.

This repository squash-merges and rebases, which destroys reported SHAs as a
matter of routine. Re-checking later would turn green evidence red weeks after
the fact, for work that was perfectly correct. A check that fails on correct
input teaches its reader to ignore it, and then it is not a check.

### Disagreement is the one thing that reaches a person

When a worker claims nothing to deploy and Swarm can see real code commits that
are not docs-only, the claim is **refused** and the task goes to the operator.

This is the case where a human genuinely adds something: the facts and the claim
contradict each other. Refusing here is what earns the automation everywhere
else. It should be rare enough never to feel like clicking — and if it is not
rare, that is a finding about the workers, which is worth knowing.

Accepting-with-a-flag was rejected: nothing would ever force the contradiction to
be resolved, which is precisely how the stale claims accumulated.

### Unverified work is visible where the operator already looks

A count on **Needs you**, with the list one tap away.

The 18 went unnoticed for one reason: zero of them sat in review, so there was
nowhere a person would encounter them. A number on a surface passed daily is what
stops that recurring. A settings panel would repeat the original failure, since
it has to be remembered.

## Deliberately out of scope

**Recording a deployment stays free text.** Verifying that a deployment is real
means health endpoints and SHA matching, and every project reports its deployed
version differently. That is a larger piece of work that deserves its own task
rather than riding along on this one. This task is about the operator's clicking,
and the clicking is on the exemption path.

**The 355 existing completed tasks are left alone.** The new rules apply going
forward. The 31 with no evidence are already written off and the rest closed on
real deployments; backfilling would mean inventing commit attribution for work
nobody can reconstruct. **Fabricated evidence is worse than an honest gap**, and a
half-populated field reads as a complete one.

## Acceptance

- An operator can tell, without clicking into anything, how much unverified work
  exists and why.
- Investigation and docs-only work closes with no human, on facts in tables
  rather than on a worker's word.
- Abandoned work reaches a terminal state that never asks about deployment.
- A claim contradicted by the commits is refused and reaches the operator.
- An ablation for each: removing the mechanism fails the test that asserts it.

## What this document does not settle

How a worker reports its SHAs — at transition, or as a separate call — and what
happens when the workspace is not a git repository at all. Both are
implementation questions that do not change any decision above.
