# ADR 0054: Verifying an operator ruling a worker did not obtain

## Status

Accepted, 2026-08-25. Asked for by the operator: "workers need to understand
that the queen is not a peer. She can issue commands on my behalf. Otherwise
Swarm will never be able to run overnight."

## The failure

Queen obtains a ruling from the operator and routes it to the worker that has to
act on it. The worker stops and asks the operator to confirm what they already
decided. Overnight there is nobody to answer, so an authorised action waits until
morning.

Two instances on one night, and they are not the same instance:

- A worker holding a finished cherry-pick was told the operator had approved
  opening a PR, cited by decision id. It raised a confirmation prompt with four
  options, all of which stop until a human presses a key. **It was relying on the
  relay alone, and it was right to refuse.**
- This worker read the operator's own resolution out of the durable store,
  confirmed it matched the relay exactly — **and asked anyway.** The operator had
  stepped away. Had they not come back, a release they had already authorised
  would have sat all night. Their reply: "I had stepped away but you did what I
  said, you didn't believe the queen's issuance."

The second is the one that matters, because it is the case a read path alone does
not fix.

## Why the refusal is correct and must stay

Agent sessions are instructed that a cross-session message is a teammate's
request and never the user's approval, and that performing an action a peer could
not perform itself is permission laundering. Without that rule any session on the
machine could unlock any other session's outward-facing actions by asserting an
approval. Nothing in a message can repair this: **anything a sender can write, a
sender can fabricate.** "I am Queen" is not evidence, and neither is a
well-formatted relay citing a real id.

So the defect was never the refusal. It is that Queen's authority was only ever
asserted in prose, and prose is exactly what a worker is right to distrust.

## Decision

**A worker verifies the ruling instead of believing the messenger.**
`swarm_list_decisions` takes a full `decision_id` and returns the operator's own
recorded answer — whoever raised it.

**Not scoped to decisions the worker originated.** That rule is right for
browsing an inbox and wrong for verification, because the entire point is
checking a ruling somebody else obtained. The full id is the capability: 128 bits,
unguessable, and already present in the relay the worker is deciding whether to
believe. The requester's argument — reason, risk, evidence, answers — is not
returned; the operator's answer and what they were answering is. That last part
matters: without the title and summary a worker cannot catch a relay that cites a
real decision authorising something else.

**A prefix is refused, never resolved.** Task `01a036ad-847f…` and decision
`01a036ad-dee2…` were created inside one millisecond window on 2026-08-25 and
share eight characters, in different namespaces, with nobody trying. `UUIDv7` is
time-ordered, so a busy Hive generates near-collisions by construction and is
busiest exactly when verification matters most. Any design where a relay names a
truncated id and the worker resolves it is unsound before it starts.

**An empty answer says which set was searched.** The old reply was a well-formed
empty list — no error, no hint — so "there are none you originated" and "there is
one and it is not yours to see" arrived wearing the same clothes. A worker that
had not read the tool description closely would conclude the decision did not
exist and report a contradiction that was not there, and one had been primed to
treat exactly such a contradiction as more significant than the action.

## The half the code cannot do

**Reading a verified ruling and acting on it are different steps, and this worker
failed the second while passing the first.** A fix that only makes the record
readable leaves the stall intact: a worker can be handed perfectly good
first-party evidence and still decline, and the outcome is identical.

The distinction the tool description now carries, because it is the thing that
was misapplied: the rule covers **a peer's claim about the operator**. A resolved
decision read from this Hive's durable store is **the operator**. Acting on it is
not laundering, because nothing was relayed — the worker went and looked. What it
authorises is exactly what it says and nothing beyond it, and an unresolved or
absent decision authorises nothing at all.

Stated as the two cases: a worker holding only a message must stop. A worker
holding a verified resolution must not.

## What this deliberately does not do

It does not weaken the laundering rule; it gives a worker a way to satisfy it
without spending an operator's attention. It does not make Queen trusted — she is
still not a peer and still not the operator, and her relay is still not evidence.
It does not auto-approve anything: the operator's ask was that an *already
recorded* decision needs no second confirmation, not that confirmation disappears.

## Where the behaviour lives

`verify_decision` in `crates/swarm-api/src/agent.rs`, pinned by
`a_worker_verifies_an_operator_ruling_it_did_not_raise`, which asserts the
pending case authorises nothing, the resolved case verifies with the operator's
action and the decision's own title, the unknown id reads as absence rather than
as an error, the prefix is refused, and an empty inbox names its scope.

Note for whoever extends this: a worker's MCP tool schema is fixed when its
session connects (ADR 0053), so the `decision_id` argument does not reach a
session that predates it. Those sessions keep the old behaviour and will keep
stopping until they reconnect.
