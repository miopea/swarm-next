# September 9: post-engine-return dependency acceptance

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

## Still open

Automatic loaded-engine admission, long-session performance, real mobile
attachment/handoff acceptance, the BFG Admin linked-task/reply journey, full UX
and accessibility acceptance, and real-backlog Queen recovery remain in the
overall maturity scope. This exercise closes only this concrete cross-worker
dependency/resumption and routine documentation-settlement journey.
