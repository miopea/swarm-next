# September 9: post-engine-return dependency and calendar acceptance

## Calendar-gate follow-up on a501bc9b

The App/API-only guidance update preserved all twelve exact sessions and engine
PID 2271655. Its four CI jobs passed (`34315529668`). One additional fictional
task, `01a084ad-60cc-7fe1-82d1-77af96a3b7d2`, was admitted Blocked in the existing
workflow-fixture repository, assigned to Swarm Dogfood, with an exact earliest
start of 2026-09-09 06:00 UTC. Queen had to record the written date structurally,
then recheck and resume it after expiry. No new operator decision was required.

- Activity 9220: Queen used block reassessment to record `blocked_until=1788933600`,
  checked evidence and original assignment. Next owner became `blocked`.
- At 05:51:49 UTC the expected file did not exist and the worktree was clean.
- After expiry, the task remained Blocked and next owner became Queen. At
  06:01:55 UTC Edge showed it under Waiting on Queen with “Blocked · Queen
  reassessment needed”, not under scheduled work.
- Activity 9233, 06:03:21 UTC: Queen rechecked the date and moved it to Ready.
- Activity 9237, 06:03:52 UTC: the original worker picked it up, independently
  checked the clock, and marked it Active (31 seconds after resumption).
- Activities 9238/9239, 06:04:44 UTC: worker submitted Review and the system
  automatically settled documentation-only work. Due-to-completion was 284 seconds.

Commit `4c8defdee737413a30b7e316d02ff38e865dea01` changes only the new
`calendar-window-20260909.txt`. Independent `cmp` verified its exact contents:
`swarm-calendar-window-20260909:verified` followed by one newline. SHA-256 is
`e146f91229511fa254ec95d4d9301bac37f53d044cc6517577f8448c0a7c5310`.
The repository is clean. The bounded read-only observer exited successfully;
there was no manual unblock, direct prompt, synthetic settlement or deployment
during the due-time journey. No real-project terminal was read or typed into.

This establishes one actual scheduled-hold/resumption journey and routine
settlement, not clearance of the real backlog, universal date interpretation,
automatic engine admission or full performance acceptance. Expiry returns a
verification obligation; it does not independently authorize execution.

## Environment and scope

Live App/API and engine:
`1.6.0-dev-cf83c980bf17-20260909041327-2251164`.
Engine PID 2271655, API PID 2252714. The prior planned replacement returned all
twelve loaded workers into their exact provider conversations. Twenty-two
sleeping workers remained asleep. No release was cut.

This exercise used only the existing `workflow-fixture` and `contract-fixture`
repositories under `/home/bschleifer/projects/.swarm-next-dogfood`. One operator
task was created, assigned to Swarm Dogfood and made Ready. Its description
explicitly required Queen to create and link one prerequisite in the other demo
worker's lane. No manual repair, direct prompt, approval override, fake deployment
record or real-project terminal inspection was used after admission.

## Verified journey

Parent: `01a0847d-2108-7612-b0c0-8361a28aab53`.
Queen-created prerequisite: `01a08482-3215-7343-aa6b-fcd6574589fd`.

| Step | Durable evidence | Result |
| --- | --- | --- |
| Operator admits parent | Activity 9180, 1788929213 UTC epoch | Ready, assigned to Swarm Dogfood |
| Worker picks up | Activity 9182, 1788929240 | Active after 27 seconds |
| Worker verifies missing cross-worker input | Activity 9183, 1788929279; task message `01a0847e-5897-7391-9986-cb06eeae98be` | Blocked; no write outside its lane |
| Queen creates and assigns prerequisite | Activities 9184/9185, actor `019ff136-7a90-7631-bbc0-f95efd1df576` | Exactly one task in Contract worker's workspace |
| Queen records structural dependency | Activity 9186 and parent `prerequisites` response | Exact prerequisite ID, owner and reason stored |
| Contract worker picks up | Activities 9187/9188 | Ready to Active in 18 seconds |
| Prerequisite completes | Activities 9189/9190, 1788929621 | Review immediately auto-settled by system as documentation-only |
| Queen resumes parent | Activity 9191, 1788929900 | Blocked to Ready, original assignee preserved |
| Original worker resumes | Activity 9193, 1788929914 | Active after 14 seconds |
| Parent completes | Activities 9196/9197, 1788929990 | Review immediately auto-settled by system as documentation-only |

Ready-to-completed elapsed time was 777 seconds (12 minutes 57 seconds).
Prerequisite completion to Queen's parent resumption took 279 seconds. The
shared 300-second coordination-delivery cooldown contributed to this latency.
Queen's queued review was delivered at 1788929799 with one attempt; its displayed
pacing deadline cleared. Message-driven prerequisite creation proceeded before
that general review. This is successful autonomous execution, not proof of low
orchestration latency or complete resolution of the real Queen backlog.

## Artifact verification

- Contract commit `1be02035e07ff1f06f389a2945d692ab11a24d38` added only
  `engine-return-20260909.txt` relative to baseline `54c0927`.
- Workflow commit `bc76a64` added only `engine-return-receipt-20260909.txt`
  relative to baseline `a6c7dec`.
- Both worktrees were clean. Independent `cmp` verified the marker equals
  `swarm-engine-return-20260909:verified` plus one newline and the receipt equals
  the marker. Both SHA-256 hashes are
  `aa180d50881f226e71314b7ae0fe9ab722abc2af3328fc27a51d4e6bc67e531c`.
- Both tasks left the open board. Activity actor identities distinguish operator
  admission, Queen orchestration, each worker's execution and system settlement.

## Browser and resource evidence boundaries

The live mobile layout was measured at 390 by 844 CSS pixels. Body/document width
was 390 (no horizontal overflow); visible primary navigation targets were 44
pixels high. Queues showed grouped owners and dependency reasons. Needs You
showed the all-clear state with count zero. This was Edge responsive emulation,
not Android/iOS picker, keyboard or suspension acceptance.

A preceding separate desktop demo tab measured 0.308% main-thread task time
over 218 seconds. Ten switches between the two demos completed. Listener counts
rose and subsequently fell; no retained-listener leak is established. Native
diagnostics captured a 1701 ms snapshot sample: 25 ms state application and
1677 ms geometry/focus follow-up, including delayed animation frames. Large
presentation delays occurred with the desktop locked despite browser focus and
visibility reporting foreground. These measurements do not establish the cause
of the operator's intermittent terminal jumping or aged-browser sluggishness.
The test tab later no longer existed; its browser observation ended, not passed.

The independent one-hour Linux observation remains under
`/tmp/swarm-cf83-return-soak.kIt7SR`, PID 2274770. At 1388 seconds all twelve
sessions remained, with no dropped history bytes. Its final result was pending
when this acceptance record was written. No deployment interrupted that window.

## Executable-code and no-deployment settlement — September 9

Task `01a08585-edab-7a02-a10a-0f3c593eb854` used the existing Swarm Dogfood worker
`01a06eda-bdd1-7a82-928e-cffbee0be6c1` and its isolated workflow-fixture repository.
The operator-authorized admission requested a bounded retry lookup and built-in
Node tests, with no real-project changes, deployment, publication or customer send.
Assignment and Ready admission used normal API transitions; nothing injected a
terminal prompt or repaired task state afterward.

Durable activity records (UTC, September 9):

- 09:35:43 — Ready, operator admission (sequence 9357).
- 09:36:37 — Active, assigned demo worker (9360).
- 09:39:21 — Review, assigned demo worker (9362).
- 09:42:15 — Completed, Queen (9363).

Queen approved the explicit no-deployment exemption for executable code that is
used only by its own tests in the isolated fixture. She stated that her assessment
relied on recorded worker commit/test evidence and authorized scope, not an
independent execution she could not perform. The result did not pretend to be a
documentation-only task or invent deployment evidence.

Independent inspection after settlement verified a clean tree and commit
`18126b3199c7744620127cd54cd2d980857216d9` containing exactly two new files:
`retry-budget-20260909.mjs` and `retry-budget-20260909.test.mjs`. Source review
confirmed the requested integer/count/delay limits, empty and exhausted behavior,
RangeError contract and nonmutation. `node --test retry-budget-20260909.test.mjs`
passed all 12 tests. The worker additionally recorded a 21-test full-suite pass
and four mutation checks; those mutation checks were not independently repeated.

This is autonomous supported code/no-deployment settlement in 392 seconds from
Ready. It does not establish actual service deployment settlement, all Queen
recovery paths or long-running backlog clearance. No original worker or engine
was restarted, and intentionally sleeping workers remained asleep.

## Actual isolated deployment settlement — September 9

Task `01a085bb-4eeb-7141-b316-fbb277962a25` used the same idle Swarm Dogfood
worker and disposable repository. Normal creation, Ready and assignment were
the only controller mutations. Delivery succeeded; no terminal input, manual
completion, retry, reassignment or wake was used afterward.

Durable activity: Ready at 1788950040 (9392), worker Active at 1788950094 (9394),
worker Review at 1788950405 (9398), system Completed at 1788950475 (9400).
That is 435 seconds from Ready, with `closed_on_evidence=true` and
`closed_unverifiable=false`. Completion cited the actual `isolated-local-demo`
deployment. The worker attempted Awaiting Release and correctly received an
authority refusal; it stayed in Review and recorded deployment evidence instead.
This proves the Review-to-system-completion path, not a Queen-owned transition
through Awaiting Release.

Independent inspection confirmed clean commit
`ac60b9d79bffe6935de8752bcc019eb7fc0ac9ac`: seven new fixture source/test files,
no pre-existing file changes. The committed artifact and independently read
`deployment-fixture-20260909/deployed/status.json` both hash to
`920999e66e856c6c6efd3c278a945b0935801d71e43823835b645839ffe9a432`.
An independent bounded loopback server served the deployed 164 bytes with HTTP
200 and that same hash, then closed in finally. No public or permanent service
or release was created. The worker's 39 passing tests are reported evidence,
not an independently repeated full-suite result.

Do not promote the disposable deploy script into production tooling. Its stale
replacement uses two renames with a missing-path interval and lacks restoration
if the second rename fails. The observed first deployment and unchanged-byte
retry do not establish atomic replacement or crash recovery. This limitation
does not invalidate the independently verified deployed bytes, but prevents
claiming that the fixture's broader atomicity comments are proved.

## Same-task idle recovery — September 9

Task `01a08615-bf39-7c53-8827-4a461c8765cb` deliberately ended its first
provider turn after committing a checkpoint in the existing isolated Swarm
Dogfood repository. It stayed Active with an explicit remaining authorized step,
no question, no background job and no request to Queen. Normal task creation,
Ready and assignment were the only controller mutations.

Durable activity: Ready at 1788955967 (9432), Active at 1788955997 (9437),
checkpoint at 1788956055 (9438). The provider was subsequently observed Resting
with no background work. Recovery attention `01a08617-a124-7080-a632-9dde9b9796ab`
was recorded at 1788956090; Queen review was delivered at 1788956242. The worker
returned to activity, then Review and system Completed at 1788956414 (9441/9442).
Ready-to-completion took 447 seconds; this is not an instant-recovery claim.

Independent repository inspection verified the first checkpoint commit
`a33abed5d496056c55a42ed724a9e2f72936b9db` and completion commit
`1bef45817a160058e45ce17def9fceaaecc5a602`, changing only
`idle-recovery-20260909.md`, with both required headings and a clean tree.
Engine metadata retained session `01a0846e-3ee1-7af3-9a77-3d41e0a5acf6`
and selected/confirmed conversation `019ff8e1-4a2d-7a11-acff-6a10fb57af3e`.
No manual nudge, terminal input, restart, reassignment or completion was used.

The worker reported receipt `01a0861a-ee67-75a3-a4eb-48ab9fd2f4ff` and retained
provider context; those statements were not independently verified from terminal
content. Independent evidence establishes the resting gap, recovery obligation,
resumed activity, unchanged engine conversation, second commit and settlement.
Protected-input/background-work and failed-recovery escalation remain separate
acceptance cases. Do not rerun this passing fixture to substitute for them.

## Still open

Automatic loaded-engine admission, long-session performance, real mobile
attachment/handoff acceptance, the BFG Admin linked-task/reply journey, full UX
and accessibility acceptance, and real-backlog Queen recovery remain in the
overall maturity scope. This exercise closes only this concrete cross-worker
dependency/resumption and routine documentation-settlement journey.
