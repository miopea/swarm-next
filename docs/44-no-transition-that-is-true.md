# No transition that is true

A design for eight instances that look like eight bugs and are one, folded from
five deferred tickets on operator decision `01a0637e` — *"Fold the five into one
spec and work it"*, read at source, `answered_how=chose_an_offered_action`.

**The finding is that the fix is already in the code, applied once, to one
case.** `NextMoveOwner::derive` carries a single stored input called
`review_returned`, and the comment beside it states the general principle
exactly: *"The task did not move; the debt did."* Everything below is that
sentence, generalised.

## The shape

    THE LIFECYCLE SOMETIMES LEAVES AN ACTOR WITH NO TRANSITION THAT IS TRUE.

Sharpened by reading the code rather than the incidents:

**The state says WHERE the work is. Who owes the next move is *derived* from
that, and the derivation is guaranteed never to disagree with it.** So when the
true addressee does not match the state, an actor can only tell the truth about
one of them. They must either move the work somewhere it does not belong, or say
nothing and leave a note.

That guarantee is deliberate and it is documented. `crates/swarm-domain/src/tasks.rs:577`:

> Computed on read like `deployment_recorded`, **so it can never disagree with
> the state and assignment it is derived from.**

It is a good invariant for consistency and it is the direct cause of six of the
eight instances. The companion failure is why the damage stays invisible: **the
state is durable and the reason is not**, so the next reader supplies a
plausible reason and is confidently wrong rather than visibly uninformed.

### The enum cannot name the operator

`NextMoveOwner` is `Worker | Queen | Blocked | Release | Nobody`
(`tasks.rs:537`). There is no `Operator`.

Instance 2 below is a worker who had finished, whose next move was the
operator's, and who therefore had nothing true to say — not because the right
state was missing, but because **the vocabulary has no word for the person the
work was actually waiting on**. Every task on this board that is waiting on the
operator is currently recorded as waiting on Queen, or as Blocked.

## The eight, and two of them are not what the ticket says

Two entries in the filing turned out to be false beliefs about edges that exist.
That matters more than the corrections themselves, because *the same mistake is
instance 8* — and both were made by people fluent in this lifecycle.

| # | Instance | Verdict after reading the code |
| --- | --- | --- |
| 1 | `awaiting_release` refuses Blocked and Review | **Partly a false belief.** `awaiting_release -> active` exists (`tasks.rs:90`) and always has |
| 2 | Review refuses Blocked; nothing was true | Real, and **sharper than filed**: a worker in Review has exactly ONE legal move |
| 3 | A no-deployment claim cannot be withdrawn | Real. `superseded_at` is only ever set by a deployment (`task_outcomes.rs:836`) |
| 4 | The lifecycle description has no exit column | Real, **already fixed** in `e2c3adb` (`01a0635e`) |
| 5 | A delivered message could not say which session took it | Real, **already fixed** in `9690c33`, schema 121 |
| 6 | An undelivered message reads as no message | Real, and the fixable half is the sender's, not the recipient's |
| 7 | A worker cannot undo its own pickup | Real, **but not for the stated reason.** `Active -> Ready` exists for nobody |
| 8 | Queen believed Blocked work could not be assigned | **False belief.** `assign_task` refuses only Completed (`lib.rs:2583`) |

### The audit Queen asked for, after instance 1

Queen flagged that she is the common author of the list, that instance 1 was an
inference from two refused calls rather than a measurement, and that she did not
know the rest were clean. She was right to, and **a third entry did not survive
it — one this document had already repeated.**

**Instance 7 says `Active -> Ready` "is Queen's edge". There is no such edge.**
`can_transition_to` gives `Active -> Blocked | Review | Abandoned`
(`tasks.rs:80`), and nothing anywhere maps `Active -> Ready`. The real route is
`Active -> Blocked -> Ready`, and only the second hop is Queen's — because a
worker may target `Active | Blocked | Review` and nothing else, not because the
first hop is permission-gated.

That is instance 1's failure inverted: there, a missing edge was inferred from
two refusals; here, a **missing edge was described as a permission-gated one**.
Both mistake a fact about the graph for a fact about authority, in opposite
directions. It does not change what that worker needed and it does change the
sentence this document used to justify the remedy, which is why it is corrected
rather than absorbed.

**Instance 2 survived and got sharper.** Review's exits are
`Active | Ready | AwaitingRelease | Completed | Abandoned`. Intersect that with
what a worker may target — `Active | Blocked | Review` — and **a worker sitting
in Review has exactly one legal move, and it is `Active`.** The filing said no
reachable state was true, which reads as five options all false. It was one
option, and it was false. That is a stronger statement of the same defect.

**Instance 3 survived, measured.** `superseded_at` is written in exactly one
place, `let superseded_at = deployed.then_some(now)` (`task_outcomes.rs:836`).
There is no withdrawal route for anyone, including the claim's author.

**Instances 5 and 8 were already checked against the code**, and 6 is stated
below as a position rather than a finding precisely because nobody has measured
it. So the count stands at eight, and the tally of how they were established is:
**four measured against code, two fixed and verified, one unmeasured and flagged
as such, and three that carried a false premise** — 1, 7, and 8. Three of eight.

Instance 1's correction is worth stating plainly because a real acceptance is
standing on it: **`01a06316` was recoverable the whole time.** Queen tried
`awaiting_release -> blocked` and `awaiting_release -> review`, was refused
twice, and concluded the state could not be left. The worker who filed the
follow-up repeated that in writing — *"the one-way door stays"* — without
opening the file. `awaiting_release -> active -> review` was available at every
moment.

So three of the eight carried a false premise about the graph, in two
directions: **1 and 8 are an edge that exists, believed absent; 7 is an edge
that does not exist, believed present and permission-gated.** Every one was
written by someone fluent in this lifecycle, and none of the three needed
anything harder than opening `can_transition_to`.

That is what `01a0635e` fixes, by generating the exit table from that function
rather than describing it. It is not superseded by this document, and on this
evidence it is the highest-value item here — three of eight instances would not
have been written at all if the table had existed.

## The proposal

### 1. Store the addressee. Keep the derivation as its default.

`review_returned` is already this mechanism, built for one case. Generalise it:

| | Today | Proposed |
| --- | --- | --- |
| Who owes the next move | derived from state + assignment | derived, **unless** an actor has said otherwise |
| Overrides | one boolean, `review_returned` | a stored `(owner, reason)`, set by whoever hands the work on |
| Reason | nowhere | stored beside the owner, and required to set one |
| `NextMoveOwner` | `Worker Queen Blocked Release Nobody` | `+ Operator` |

**What it makes the machine do**, stated as behaviour rather than intent:

- `swarm_transition_task` gains a sibling that changes the addressee **without
  moving the task**. The state stays correct; the debt moves.
- A stored addressee **clears itself when the named owner next acts on the
  task**, exactly as `review_returned` does. Nobody has to remember to unset it.
- Any surface that lists work by `next_move_owner` — Queen's run brief,
  coordination attention, the control room — reads the stored value where one
  exists and the derived value where none does. No caller changes.
- The reason is **not optional**. A stored addressee with no reason is refused,
  because the whole failure this addresses is a durable state beside a lost
  reason. This is the one place the design spends a refusal.

The invariant that made this necessary survives in a weaker and more honest
form: the addressee can no longer *silently* disagree with the state, because
disagreeing requires a reason that a reader can see.

### 2. A claim its author disowns is superseded

Instance 3 is not an addressee problem and the addressee mechanism does not
reach it. It needs an edge that does not exist: `superseded_at` is set only when
a deployment arrives.

**What it makes the machine do:** the worker that filed a no-deployment claim
can supersede it while it is unapproved, setting `superseded_at` with a reason.
An approved claim stays immutable, which is already true and stays true. This is
a few lines and it is independent of everything else here.

### 3. The assign response says what it did and did not do

Instance 8 is not fixed by the exit table, because it is a belief about
*assignment*, not about transitions. Queen observed that assigning Blocked work
woke nobody and generalised it to "you cannot assign Blocked work" — reading a
sentence about DELIVERY as a sentence about LEGALITY.

**What it makes the machine do:** `swarm_assign_task` returns, on every call,
whether the assignment took effect *and* whether anything was woken, as separate
fields. Today the wake is implicit and its absence is silent, which is precisely
the gap the inference filled.

### 4. The sender is told when a message has not landed

Instance 6, and this is the half nobody has established, so it is stated as a
position with a falsifier rather than as a conclusion.

`swarm_message_worker` returns `queued` and never updates. The sender must poll
`swarm_read_task_history` and read `delivered_at`. Architecture checked the
board instead, found nothing, and filed a ticket about a decision that existed.

**Position: an undelivered message is correct behaviour and an *unremarked*
undelivered message is not.** Delivery waits for a resting prompt on purpose and
that should not change. What should change is that a message still undelivered
after a threshold appears in the sender's own attention surface, rather than
requiring them to know to look.

**What would falsify it:** if most undelivered messages land within a minute or
two, this is machinery for a case that resolves itself, and the honest answer is
to change the tool's reply from `queued` to something that says how to check.
**Nobody has measured the distribution of time-to-delivery.** That measurement
decides this item and it does not exist.

## What this does not cover, named

- **Instance 4** — the exit column. Already shipped in `e2c3adb`, and separate
  on purpose.
- **Instance 5** — the delivery session. Already shipped in `9690c33`.
- **Instance 7's underlying edge.** The addressee mechanism lets a worker say
  *"I picked this up in error, this is Queen's"* without moving the task, which
  is what that worker needed. It does **not** add `Active -> Ready`, which
  exists for nobody today. Nothing here argues it should — the existing route is
  `Active -> Blocked -> Ready`, and the worker's complaint was never that the
  route was missing. It was that Blocked overstates the obstacle, which is a
  complaint about what the state SAYS rather than about where it leads.
- **Re-delivery of a message whose session has gone.** `01a06340` decided that
  NO, deliberately, recorded before the code, with a falsifier: how often senders
  re-send a stranded message by hand anyway. **This spec does not reopen it**,
  and if anything reopens it, it should be that measurement.
- **Self-review** (`01a05ade-2778`). Untouched. The bottleneck was never measured
  with `3f85107` and `cdafc72` live, and that measurement should come before the
  invariant is questioned.

## Two things the superseded tickets got wrong, which the spec would have inherited

**Blocked does not lose the assignee.** `01a05ade-6fcf` reported it and the
transition does not touch assignment — the `UPDATE` at
`crates/swarm-persistence/src/lib.rs:2435` sets `state`, `blocked_until` and
`updated_at`, and nothing else. Whatever the operator saw, it was not the block
itself. Worth re-observing before anyone designs for it.

**Deliveries do not land mid-turn.** `01a05ad5` assumed *"a write lands whether
or not she is mid-turn"*. Every delivery already refuses unless the provider is
Resting — `delivery_baseline` returns `Deferred(ProviderBusy)` otherwise
(`coordination_delivery.rs:360`).

That correction leaves the operator's report intact and moves its cause:

> **Resting is not the same as finished.** The gate uses "the prompt is idle" as
> a proxy for "it is safe to interrupt". For a single-turn task those are the
> same; for a multi-turn review they are not, and a briefing arrives at the
> first pause rather than at the end of the thread.

This is the same failure as everything above, one level down: **a durable signal
standing in for one nobody records.** A turn boundary is observable and a task
boundary is not. It is out of scope here and it deserves its own ticket, because
the remedy — an agent saying "I am mid-thread" — is a new signal rather than a
new edge.

## What this costs

Storing the addressee weakens a real invariant. Today `next_move_owner` cannot
lie, because it cannot be set. Afterwards it can be wrong the way any written
field can be wrong — someone can hand the work to Queen and be mistaken.

That is the right trade and it should be made with the cost visible: **a wrong
addressee with a stated reason is better than a right addressee that is right by
construction and silent about a debt nobody can see.** The reason requirement is
what makes it recoverable; a reader who disagrees can see what they are
disagreeing with.

## What would change the answer

- If the time-to-delivery distribution turns out to be short, item 4 shrinks to
  a wording change on one tool reply.
- If instances of "no transition that is true" stop appearing once `01a0635e`
  ships, then three of the eight were the false-belief failure and the addressee
  mechanism is solving a smaller problem than this document claims. **Eight
  instances in two days is a lower bound found by accident with an unknown
  denominator** — the same shape as `01a0635f`, and it is not a rate.
