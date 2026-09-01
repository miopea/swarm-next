# Making the Queen work

The settled design from an operator interview on 2026-09-01, covering the five
deferred Queen tickets. It supersedes their individual framings where the
measurements below contradict them.

**Two of the five tickets rest on premises that are false.** They are corrected
here rather than built to.

The operator's instruction was to fix the architecture and let the stuck work
fall out of it: *"the fixes should solve it when we scope it and it is done."*
Nothing here is a patch to a particular stalled item.

## What is actually stuck, measured

Of 32 tasks in Review on the live board:

| waiting on | count |
| --- | --- |
| a Queen approval | 15 |
| a **worker** to record any evidence at all | 16 |
| the stranding bug fixed in `7b3acfc` | 1 |

So "Queen is the bottleneck" is true of under half of it. Seven of the 32 carry
recorded commits — work that is finished and unshipped, which today has no
honest way to say so.

Thirty tasks are Blocked; twelve carry no assignee.

## The organising idea: who owes the next move

Every waiting state answers one question — *who is holding this up* — and the
system could not answer it. That is the root of the queue clog, the anonymous
blocked pile, and the operator's attention surface filling with other actors'
backlogs.

Everything below is a consequence of making that question answerable.

## 1. A Swarm-owned message channel

Queen already asks workers questions, through Claude Code's own session channel.
Swarm did not build it, cannot see it, cannot record it, and one of the 39
active workers (the Codex one) cannot receive it at all.

**Decision: Swarm owns the channel, and an exchange is durable on the task.**
The question and its answer attach to the task and appear in its history. A
transient channel would leave coordination in two terminal scrollbacks, which
contradicts *what is not on the board did not happen*.

**Permissions: Queen↔worker in both directions. No worker-to-worker, ever.**
A worker may raise a thread to Queen unprompted, not only reply. Peer messaging
is refused outright rather than governed, because a worker's claim about
authority reaching another worker with no board record turns *anything a sender
can write, a sender can fabricate* from a discipline into an attack surface.

**Delivery uses the path that already exists.** `coordination_delivery` refuses
to write into a session that is not Resting. That check is the reason ordinary
workers are not interrupted mid-turn.

**Deliberately deferred: Codex cannot reply.** Delivery is a terminal write and
reaches any provider, but answering needs a tool call, and Codex sessions get no
MCP config (`01a05b20`). The operator's ruling is to ignore this for now and fix
Codex afterwards. A Codex worker can therefore be asked and cannot answer, and
that is a known limitation rather than an oversight.

## 2. The interruption is the same defect, not a second one

`01a05ad5` reports Queen losing her thread to prompts, repeatedly. The
investigation it asks for has one answer: **Swarm's own delivery already defers
while a session is busy.** So the interruptions are arriving through the
ungoverned channel, which bypasses that check entirely.

Queen is currently the only actor not protected by a mechanism that already
works.

**Decision: route her messages through the existing polite delivery.** The
interruption stops as a consequence of §1 rather than as its own build. No new
queue machinery — the operator was explicit: *"use what exists right now and
just not architect something that is worker/queen driven later."*

## 3. Evidence must be cited, and Queen orchestrates rather than verifies

At 04:25 on 2026-09-01 Queen approved a completion exemption that was false —
its work's PR is still open — without checking. The invariant's promise is *that
someone other than the author checked*, and it was satisfied while nobody
checked anything. Under load a second pair of eyes degrades into a rubber stamp,
and load is exactly when it is needed.

**Decision: approving must cite evidence, and Queen may hand the task back
demanding it.** *"The concept is the queen is the orchestrator."* She routes and
requires; she does not perform the verification labour.

**Machine settles what is derivable.** The store already refuses a
no-deployment claim over commits that reached a ref and touched code, and its
own comment states the philosophy: *refusing it here is what earns the
automation everywhere else.* Anything derivable — a merged SHA, a recorded
deployment, commits contradicting a claim — settles with no person involved.

**The escalation is machine → Queen → operator, and the last step is rare by
design.** In the operator's words: *"nothing waits on a person between the
machine or queen unless there is something they cannot resolve, which should be
EXTREMELY rare."*

The operator is reached only for:

- anything that ends live worker sessions
- a destructive deployment or a database migration
- a swap to production

Everything else minimises worker friction. Note that pushing, publishing and
ordinary irreversible-but-recoverable acts were **not** included.

## 4. Awaiting release, and it closes itself

A no-deployment exemption means *this ships nothing, ever*. An open PR means
*this ships later*. Conflating them is what produced the false approval above,
and there is no way today to say "finished, awaiting merge".

**Decision: a first-class awaiting-release state, which resolves itself.**
`CommitSettlement` already computes whether a task's commits reached a ref. Work
parks there and settles when the commits land and a deployment is recorded, with
no person involved.

This is the single largest drain on the backlog, it uses machinery that already
exists, and it is the honest description of most of the work produced today.

## 5. Sending work back: mark it, do not move it

The operator's question was how to return review work to the worker. Their Jira
uses a WAITING ON column; the recommendation is deliberately different, and the
reason is industry practice rather than taste.

GitHub's *changes requested* leaves the pull request open and records that the
ball is in the author's court. Gerrit's -1 leaves the change where it is. Kanban
guidance is explicit that backward column moves corrupt cycle time and disguise
rework as new work.

**Forward progress earns a state; rework earns a marker.**

**Decision: the task stays in Review and gains an owner-of-the-next-move.**
Queen hands it back with a named request; the next mover becomes the worker.

This also avoids a defect already observed. Returning a task to Ready is what
invalidated a valid no-deployment claim on `01a05d4f`, because Ready means
*unstarted* to everything that reads it. Returning it to Active would make
finished work look unfinished.

## 6. The queues tab

The operator's attention surface currently shows work that other actors own —
the unsettled-work card literally reads *"N pieces of finished work are waiting
on Queen"*, which is Queen's backlog rendered in the operator's area.

**Decision: a new top-level tab, alongside apiary, workers, tasks and settings,
grouped by who owes the next move.**

Read against today's board it would say: 15 on Queen, 16 on workers, 7 awaiting
release, 12 blocked and unowned.

Grouping by queue mechanism was rejected: one stall then appears in several
places and the view stops answering *why is nothing moving*.

## 7. Blocked keeps a narrower, harder meaning

**Decision: Blocked is for hard reasons, not for back-and-forth.** The operator:
*"Blocked becomes a harder reason than back and forth with worker/queen. For
instance, we have tasks that are blocked on other tasks, and that is a valid
blocked state."*

Conversation between Queen and a worker expresses itself through the
owner-of-next-move marker (§5). Blocked is reserved for work nothing in the Hive
can move.

**Task-on-task dependencies do not exist today.** `blocked_by` in
`task_dispatches` is dispatch ordering — whether an earlier task has been
dispatched — not a dependency between tasks. The Blocked state records no
structured reason at all; the reason lives in a transition note. Expressing
"blocked on another task" needs building.

## 8. The lifecycle correction — `01a05ae5`

**The premise is false.** Queen believed a Blocked ticket could not be assigned.
The store refuses assignment only for `Completed`
(`crates/swarm-persistence/src/lib.rs:2473`). Blocked work has always been
assignable, and her run brief already carries the correct procedure.

So this is a correct instruction that did not survive a busy run — not an
absent one.

**Decision: the fix lives in the refusal, at the moment of the act.** She learns
a rule when she hits it, not in a brief read an hour earlier; and where an action
is permitted, nothing should imply otherwise. Strengthening her standing
instructions was rejected on the evidence that the words already existed and did
not land.

§5's descriptive marker removes much of what she can get wrong anyway.

## 9. The assignee correction — `01a05ade-6fcf`

**The premise is false.** Blocking has never stripped an assignee. All 30
blocked tasks were checked: the 12 without an assignee have **no assignment
event anywhere in their history**. They were never assigned.

The operator raised it with Queen, who reassigned the affected work. The
residual value is confirmation rather than repair — *"worth investigation just
to make sure"* — that no path silently clears an assignee.

What was real is 12 blocked tasks nobody owns, which §6 makes visible and
attributable.

## What this update is not

**The completion invariant does not change.** Workers still cannot approve their
own work. It was not chosen at any point in the interview, and the alternative
selected — machine verification plus cited evidence — addresses the same
bottleneck without removing the guarantee that caught two false claims in a
single afternoon.

Whether Queen remains a bottleneck once she can see and is not being interrupted
is a question for measurement afterwards, not for this design.

## Open, and deliberately so

- **Codex cannot answer a message** until `01a05b20` lands.
- **Task-on-task dependencies** are named as valid but unmodelled (§7).
- **What "cited evidence" must contain** per claim class is unspecified. It
  should be derivable wherever possible, per §3.
- **Whether awaiting-release needs a separate proofing pass** was raised and not
  settled; it is only worth a state if machine verification and Queen's judgment
  are genuinely different acts.
