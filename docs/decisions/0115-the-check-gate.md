# 0115 — The check gate: work counts as done when its CI passes

- Status: Accepted
- Date: 2026-10-13
- Decided by: the operator, on 2026-10-13.
  - Decision 01a13773: "Check strictness first".
  - Decision 01a1378c, answered in a worker's terminal:
    - "Block closing (Recommended)";
    - "Allow, and say so (Recommended)" for CI that cannot be read;
    - "Its gh CLI login (Recommended)" for how a Hive reads CI.
- Builds on: ADR 0113. Check strictness is a setting a Keeper can require only
  once a member enforces it, and this is what enforces it.
- Task 01a1376b. The design note is on that task (note 20189).

## Context

"Check strictness" was in the operator's first policy set: "warnings allowed,
and which checks must pass before work counts as done". No Hive had anything
for it to lock. Nothing gated completion on checks, and Swarm could not read CI
at all; its only GitHub access was for issues. A task's recorded commits held a
SHA, a verdict and the paths changed, keyed by the local workspace, with no
remote and no CI state.

"Warnings allowed" is not visible in CI results. It belongs to each
repository's own CI configuration, such as `clippy -D warnings`, so it is not a
Swarm setting.

## Decision

### 1. Three levels, per Hive

`CheckGateLevel` is `off` (the default), `warn` or `require`.
- `warn` reads CI and shows it on the task, and never holds anything.
- `require` holds a task with recorded commits open while their CI fails or is
  still running.
- CI that cannot be read does not hold the task. The close goes through, and
  "Closed without a CI check: <why>" is written to its history.

A task with no recorded commits is never held: nothing could have run on it.
That also leaves Jira-mirrored tasks and paperwork alone.

### 2. What counts

The rules are pure domain functions (`classify_checks`, `combine_outcomes`,
`gate_verdict`):
- **Passed:** GitHub check runs that completed as `success`, `neutral` or
  `skipped`, and commit statuses that are `success`.
- **Failed:** any other completed run (`cancelled`, `timed_out`,
  `action_required`, `stale`, or one never seen before), or a status that is
  `failure` or `error`.
- **Pending:** anything not finished.

### 3. Which commit

CI usually runs only on the last commit of a push.
- The task's recorded commits are reduced to their tips: those no other
  recorded commit builds on.
- Each tip is judged by its own checks. If it has none, it is judged by the
  first of the next 20 commits on its remote-tracking branch that has any.
- A tip that is not on a remote branch is waiting for "the push to GitHub".
- A commit younger than 15 minutes with no checks is waiting for "CI to start",
  because GitHub creates check runs moments after a push.
- Only after that does "no checks anywhere" count as a repository with no CI,
  which is unreadable, not passing.

### 4. Reading CI: the machine's gh login, never inside a transaction

`ci_checks.rs` asks `gh api` for check runs and statuses, bounded by `timeout
20`. It strips forced-colour variables, because `CLICOLOR_FORCE` beats
`NO_COLOR` and paints the JSON.

A bounded, owned background pass runs every minute:
- it reads at most 10 tasks;
- only while the gate is on, and only for work in review or awaiting release
  with recorded commits;
- it stores one observation per task (`task_ci_observations`), keyed by a
  digest of the commits it was read for.

A pass is final until the task records more commits. Pending CI is read again
after one minute; failed or unreadable CI after ten, since a rerun can pass and
access can come back. The gate reads only the stored observation. A reading
taken before the task recorded another commit is stale, and the gate holds
until a new one arrives.

### 5. Where completion is gated

**`TaskStore::transition_task_guarded`, for target Completed.** This covers:
- the operator's board;
- Queen's `swarm_transition_task`;
- the coordinator's close-on-deployment;
- the coordinator's settle-without-deployment.

A manual close is refused with 409 `checks_not_passed` and the reason. The two
sweeps skip the task and leave it in place, as they do for unmet prerequisites.

**`record_deployment_delivering`'s Awaiting Release settle.** It writes the
state with its own SQL, so it asks the same question. This covers
`swarm_record_deployment` and the release-tag sweep. Held work stays in
Awaiting Release.

Entering Awaiting Release is not gated: it means accepted and unshipped. The
Jira sync's own state write is not gated, and needs no gate, because mirrored
tasks record no commits.

### 6. The Keeper and the member

The Keeper requires the level as `checks.gate` (ADR 0113). The member applies
it where the level is read, keeps its own choice underneath, and refuses a
local change with 409 `set_by_apiary`.

The GitHub access stays the member's own, because credentials are never
policy. When the gate is on and the member cannot read CI at all (gh missing or
signed out), its health report carries the reason. The Keeper's roster then
shows "Can't read CI: <why>", because otherwise the work it closes goes
unchecked without anyone seeing it.

## Consequences

- Schema 200 adds `check_gate_settings` and `task_ci_observations`.
- A Hive under `require` whose gh login lapses lets work close unchecked, and
  says so twice: on each task's history, and on its Keeper's roster.
- A flaky CI can hold finished work in review until a rerun passes. That is the
  operator's choice of "Block closing".
- Check runs from apps other than GitHub Actions count the same as Actions
  runs. A repository that wants one ignored has to stop running it.
