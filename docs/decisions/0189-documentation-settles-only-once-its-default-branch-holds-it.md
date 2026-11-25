# 0189 — Documentation settles only once its default branch holds it

- Status: Proposed. It changes which Review work the coordinator closes by
  itself, so it takes effect only on the operator's own decision.
- Date: 2026-11-24
- Amends: doc 41, "Docs-only changes" (derived from the commits' paths), and
  the coordinator's no-deployment settlement that ADR 0066 and ADR 0150 guard.

## Context

The coordinator closes Review work whose recorded commits touch documentation
only. It approves the no-deployment exemption as `coordinator` and completes
the task in the same pass, usually within a second of the handoff.

It decided "documentation only" from two facts per commit: the paths it
touches, and the verdict `Present`. `Present` means that some ref reaches the
commit (`git for-each-ref --contains`). A pull request's branch is a ref.

So documentation still waiting for its merge read exactly like documentation
on main. A worker handed off a pull request that deleted a stale file, and the
task closed in the same second. The merge gate (ADR 0179) then told the worker
to ask the operator for the merge with a command card, and filing the card was
refused: a command can be allowed only on an open task. The pull request had
no task left to carry it.

Read on one Hive on 2026-11-24, from its store, the local repositories and
GitHub's record of each commit, the coordinator had closed 139 tasks as
documentation only. 8 were completed while their pull request was open, and the
pull request was still open that day. 4 more closed before a squash merge that
came later.

The same 139 read under this rule, from each default branch's reflog at the
moment each commit was recorded:

- 97 would have settled when recorded, as they did.
- 41 would have stayed in Review:
  - 30 until their commits reached the default branch and were recorded
    again. For 6 of them, the local ref was still behind GitHub's that day.
  - 7 with their pull request still open.
  - 4 that were squash-merged.
- 1 could not be read, because its reflog does not go back that far. Its pull
  request is still open.

## Decision

1. **A recorded commit says whether its repository's default branch held it.**
   When commits are recorded, Swarm asks this in the task's own workspace, as
   it already asks the other facts: once, and stored. The answer is yes, no,
   or nothing established. A commit that workspace does not have, including
   one found in another workspace, is nothing established.
2. **The default branch is, in order:**
   - the branch `origin/HEAD` names, where a clone set it and that branch
     still exists. After the remote renames its default and a prune, it names
     a ref that is gone;
   - else origin's `main`, else origin's `master`. A checkout whose remote was
     added after `git init` has no `origin/HEAD`, and 8 of 52 checkouts on
     this Hive had none;
   - in a repository with no remote at all, its own `main`, else `master`.
     It has nowhere to merge to, so that is where its work lands;
   - otherwise none, and nothing is established about any commit.
3. **Documentation settles only when the default branch held every commit.**
   Otherwise the report reads `DocumentationOffDefaultBranch`: nothing to
   deploy, and not finished. The coordinator leaves the task in Review, claims
   nothing on its behalf, and Queen's list shows it as "Docs unmerged". The
   store refuses the coordinator's approval of it, as it refuses code, so the
   sweep is not the only thing holding the line.
4. **Code is unchanged.** A commit that touches code is `BuiltCode` on any
   branch, so a no-deployment claim it contradicts is still refused.
5. **A row recorded before this asked nothing**, so it settles nothing. The
   column is added as NULL for every existing row.

## How the work closes

- **A merge commit or a fast-forward.** The worker records its commits again
  from an up-to-date checkout. The same commits now reach the default branch,
  the row is replaced, and the next pass closes the task.
- **A squash merge.** The branch's commits never reach the default branch, and
  a recorded commit cannot be withdrawn. The worker records no deployment, and
  Queen approves it, as for any claim the facts do not settle. The worker's
  tool description says this.
- **The merge card.** The task is still open, so the card can be filed on it.

## Consequences

- Documentation that went through a pull request costs Queen one more look
  when it is squash-merged. Before this, it cost nobody anything and left the
  pull request behind.
- Documentation recorded before it was pushed no longer settles until it is
  recorded again. Before this, a commit only on a local branch counted.
- A repository whose `origin/HEAD` is unset or names a branch that is gone,
  and whose default branch is neither `main` nor `master`, settles no
  documentation by itself.
- The rule reads the workspace's own refs, which are only as current as its
  last fetch. A worker records again after fetching, or the merged work still
  reads as unmerged.
- A shallow checkout cannot see past its boundary. A commit beneath it reads
  as not held, so its documentation waits for Queen rather than settling; it
  never reads the other way. 1 of 33 checkouts on this Hive was shallow.
- The 8 completed tasks with open pull requests stay completed: Completed is
  final. Routing those pull requests is Queen's, and nothing here does it.
- `swarm-domain` changes, so a release carrying this has a new worker engine
  build. The rule runs in the API, and takes effect when the API does.

## Where it lives

- The rule: `commit_settlement` in `swarm-domain`, which every reader of a
  commit report already shares.
- The fact: `verify_reported_commits` in `swarm-api` asks git, once per report
  for the default branch and once per commit.
- The record: `task_commits.on_default_branch`, schema 235.
- The row: kind `docs_off_default_branch` on the coordinator's list of
  unsettled review, and its chip in the web app.

## Evidence

- Tests seen failing before the rule existed:
  - the policy, for a commit on a branch, a commit never asked, and code on a
    branch;
  - the sweep leaving such work in Review with nothing claimed, then closing it
    once the commit is recorded on the default branch;
  - real repositories: a clone with `origin/HEAD`, a pushed branch, an
    unpushed commit, `origin/HEAD` removed, origin's `main` removed, and a
    repository with no remote on `main` and on `master`;
  - the chip on the web card.
- Tests added after the rule, and shown to bite by breaking the code:
  - `origin/HEAD` naming a branch other than `main`; `origin/HEAD` naming a
    branch that is gone; origin's `master` alone; a repository with no remote
    whose `main` and `master` disagree;
  - a squash-merged task: its claim is accepted, the coordinator neither
    approves it nor can be made to, and Queen's approval is evidence that
    closes it;
  - the coordinator's list showing `docs_off_default_branch`, not settling.
- 24 ablations, one per piece, each made its test fail, and the source was
  restored byte for byte. The pieces:
  - the rule;
  - the sweep, the claim, and the store's refusal of the coordinator;
  - the column, and its write, update and read;
  - each choice of default branch, and their order;
  - the git question;
  - the row, dropped or called settling;
  - the chip.
- git 2.43.0.

## Not proven

- Not run on a Hive, and no real pull request has been taken through it.
- The 139 were dated by each checkout's own reflog, which says when that
  checkout saw a commit, not when GitHub merged it. 12 of them, all in one
  shallow checkout, read as held when recorded and not held today, because
  git cannot walk past the shallow boundary.
- How often squash merges will now reach Queen was not measured.
