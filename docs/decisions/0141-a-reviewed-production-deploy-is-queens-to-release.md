# 0141 — A reviewed production deploy is Queen's to release

- Status: Accepted
- Date: 2026-11-02
- Decided by: the operator, on task 01a19a4b (2026-11-02). They asked for
  ideas to cut the "allow this command" cards that stall workers. Of four,
  they chose "Queen's go covers deploys" ("Queen stops asking for a command
  card on production deploys she has reviewed; your press stays for DB
  migrations and permission changes") and "Committed release/read scripts".
- Amends: ADR 0131, Queen's review releases code. This says what its release
  covers for a production deploy. ADR 0129's grant and ADR 0130's card are
  unchanged.

## Context

Measured read-only on the operator's Hive, over command requests from
2026-10-31 to 2026-11-02 11:40Z:

- **Volume:** 67 requests, about 27 a day, against 2–7 a day over the three
  days before. One worker filed 45 of them.
- **Answers:** the operator allowed every request they answered: 60 allowed,
  7 withdrawn by the worker, none refused.
- **Wait:** the median was 8 minutes. One worker's average was 63 minutes,
  and the longest wait was 8.6 hours. The worker waits idle until the press.
- **Cause:** 30 of the 67 were filed because Queen's release instruction told
  the worker to "file ONE swarm_request_decision with the exact command" for a
  production deploy. Yet ADR 0131 already says a reviewed release needs no
  operator decision, deploy included. As workers quoted her orders, they called
  a production deploy "a production action". Her brief did keep "production
  data, credentials, permissions" for the operator, without saying where a
  deploy stood.
- **Scripts:** 55 of the 67 ran a script written for that one action, with 52
  distinct hashes, typically a new deploy script for each release carrying the
  same guards.
- **Repeats:** 6 (9%) asked again for a command already allowed.

So the question was whether the classifier, not Queen, was the blocker.

Probed on Claude Code 2.1.316, in throwaway `claude -p --permission-mode auto`
sessions. Each ran in a scratchpad repository whose origin was `acme/shop`,
with the operator's "Queen's Release Instruction" rule in place, a stand-in
`gh` that only records its arguments, and no GitHub credentials. Prompts
carried Swarm's real message header, wrapped in `pasted_content`. "Deployed"
means the stand-in recorded `gh workflow run deploy.yml … environment=production`.

| Case | Deployed |
| --- | --- |
| Queen says deploy, through a script outside the repository that the worker prepared | 3 of 3, none refused |
| Queen says deploy, through the repository's committed release script | 2 of 3, none refused. The third agent stopped by itself: it could not check the pull request. |
| A worker's message says deploy, through the same committed script | 0 of 2. One was refused (Production Deploy); the other agent stopped by itself. |

In every deploying run, the agent read the script before running it.

The classifier already lets through a deploy that Queen instructs. The cards
came from her routing.

## Decision

1. **Queen is told a production deploy of reviewed work is hers.** Her brief,
   and each review's prompt, say so. That includes a production deploy: she
   tells the worker to run it through the repository's own release script or
   deploy workflow, and does not tell it to ask the operator for that command.
2. **The operator keeps what a release does not cover.** The kept list:
   - a database migration;
   - a change to production data, credentials, permissions, app settings or
     secrets;
   - another repository;
   - an admin merge;
   - a merge past a failed or missing check.

   For one of those, the worker asks with a command request for that step
   alone, so the rest of the release does not wait on it.
3. **A worker is told the same from its side.** A production deploy Queen
   tells it to make is part of her release. It runs it through the
   repository's own release path without a command request. It asks for a
   kept step on its own, as one command.
4. **The release path is the repository's own,** so what runs is what was
   reviewed. A repository with no committed release script gets one as its
   own work, filed separately (task 01a19c17 for the busiest one). The probes
   show the classifier does not need a committed script; the reason is review,
   not the classifier.
5. **Nothing else moves.** The classifier rule, ADR 0129's exact, once-only
   grant, and ADR 0130's card after a refusal are unchanged.

## Consequences

- A reviewed production deploy no longer waits for the operator's press. In
  the measured window, that is up to 30 of the 67 cards.
- A release carrying a database migration still gets one card, for the
  migration step.
- Queen takes this up at her next review, whose prompt carries it. A worker
  takes it up when its session next connects. It applies on a reload; no
  worker engine update is needed.

Residual risk, accepted:

- **The carve-out rests on the two briefs, not on the classifier.** The
  operator's Queen rule covers "release or deploy that work", which a
  migration shipped with a deploy falls under. On this Hive the operator's own
  copy of that rule governs, and the host never rewrites it (ADR 0131). So
  Swarm cannot narrow it without the operator editing their settings. A worker
  told by Queen to deploy a migration would be let through. The briefs tell
  both of them not to do that.

## Rejected

- **A grant that survives a failed run of the same unchanged script.** It
  covers 9% of cards and amends ADR 0129's once-only rule. Not taken forward.
- **One card for a whole release plan.** It amends ADR 0129. Not taken forward.
- **Narrowing Swarm's copy of the classifier rule to exclude migrations.** It
  would not reach a Hive where the operator keeps their own copy, as this one
  does. A change there is the operator's edit to make.

## Not proven

- **The live effect.** It needs a reload and then Queen's next release orders.
  Prediction: within a day of releases, command requests filed on Queen's
  instruction for a production deploy fall to about none. This is wrong if
  they keep arriving.
- **Production reads.** At 19 of the 67, they are not addressed here. They need
  a read-only helper and an operator rule naming it (task 01a19c17).
- **The probes are small.** They are three runs per Queen case on one Claude
  version, with a stand-in deploy.

## Evidence

Tests:
- `a_reviewed_deploy_is_queens_to_release_and_a_migration_stays_the_operators`:
  both halves say a production deploy is part of the release, name the
  repository's own release path, and say no command request is needed. They
  still send a migration and changes to production data, credentials,
  permissions, app settings and secrets to the operator, one step at a time.
- The existing tests still pass: Queen's half reaches only Queen and a
  worker's half only workers, only while the setting is on, and Queen's review
  prompt carries hers.

Ablations, each failing that test, with the file restored byte for byte
afterwards:
- Queen's new sentence about a production deploy removed.
- The worker's sentence keeping a migration and the rest for the operator
  removed.
