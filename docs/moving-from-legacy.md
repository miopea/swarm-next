# Moving from Swarm Legacy

If you used the Python Swarm, most of what you knew still applies: a Queen, a
roster of workers, durable tasks, Jira and email intake, terminals that stay
alive. What changed is mostly about **who is allowed to do what**, and about
Swarm saying what it does not know.

This is not a feature comparison. It is the handful of differences that will
surprise you.

## Nothing carries over automatically

Swarm starts with an empty Hive. Your Legacy tasks, workers and history are not
imported by installing it, and the two do not share a database — Legacy keeps
`config.yaml`, Swarm keeps `swarm.sqlite3`.

There is a migration path in Settings → Migration that previews what it would
bring across before changing anything. It is optional and you can start without
it. If you are trying Swarm for the first time, starting empty is the simpler
thing to judge it by.

The two can run side by side. They are separate processes with separate state.

## Workers no longer message each other

Legacy let any worker send a message to any other. It moved work along, and it
also produced arguments about who owned what — two agents negotiating
responsibility, in a channel nobody was reading.

Swarm removes it. Work flows one way: a worker reports outcomes to the task and
to Queen; Queen routes; you decide anything Queen cannot. A worker can file work
it discovered, and Queen is told it is waiting, but it cannot address another
worker and cannot assign anything — including to itself.

**What you lose:** workers coordinating directly on shared work.
**What you get:** responsibility that does not move without someone deciding it
should.

If two workers genuinely need to coordinate, that is a task and a routing
decision, not a conversation.

## Queen does less, on purpose

In Legacy, Queen was on the hot path for a great deal of mechanical work —
polling, transitions, housekeeping.

In Swarm, anything typed, reversible and policy-complete is done by a
**deterministic coordinator** that makes no model call at all: waking a worker
that has been assigned ready work, noticing work whose worker exited, noticing a
briefing that was delivered and never started.

Queen is reserved for ambiguity, prioritisation, cross-worker judgment, and
things that need you. She runs in bounded reviews rather than continuously, and
you can see every one.

**What you lose:** Queen acting on everything, immediately.
**What you get:** far fewer model calls, and behaviour you can predict without
asking what she was thinking.

## Confidence is not authority

Legacy scored proposals and acted above a threshold. Swarm does not have that
dial.

A decision is presented to you when evidence or policy cannot settle it — not
when a number falls below a bar. External effects stay blocked unless a durable
rule you approved covers that exact action, and neither Queen nor a worker can
create or widen that authority.

## Asking you is a queue, not an interruption

Legacy surfaced prompts where they happened, and stranded input was a real
category — something waiting in a terminal nobody was looking at.

Swarm has one queue. A worker that needs you files a decision and stops; it
appears in **Needs you** with what is being decided first and the reasoning
folded behind it. You answer with a button, or in your own words when none of
the offered answers is right, or decline with a reason.

Declining requires a reason, so "hold this" and "stop asking me" cannot be
recorded identically — which in Legacy they were.

## Swarm tells you when it does not know

The difference you will notice most.

When Swarm writes into a worker's terminal it watches the screen until it can
see the text, then sends Enter separately. If it cannot confirm the message
landed, it marks the worker and **does not send it again**. A briefing delivered
twice is worse than one you were told about.

You will see states like "could not confirm it landed" and "finished ·
unverified". They are not failures. They are Swarm declining to claim something
it has not established — a completed task is not a deployed one, and it will not
say otherwise.

## Approvals are mostly gone

Legacy spent effort clicking provider approval prompts safely. Modern providers
approve routine work themselves, so Swarm does not reimplement that. What
remains under Swarm's control is external effect — Jira writes, email replies,
deployments — and those are explicit.

## Email replies are reviewed, always

Legacy could draft and send. Swarm drafts; **you send**. The reply appears in
Needs you with the words in front of you. A ticket merged from several messages
by one person is answered once, on the thread they wrote in most recently,
rather than once per message.

## Things Legacy had that Swarm deliberately does not, yet

Named so you do not go looking:

- **Playbooks and standing pipelines.** Deferred until a real journey cannot be
  expressed as tasks plus coordination.
- **The learning miner.** Under investigation — the question is whether it
  improves decisions without creating policy nobody can see.
- **Speculative task preparation.** Deferred until wrong-recipient and
  cancellation cases are proven.
- **Fleet broadcast.** Removed, as above.

## What is genuinely better

- **The terminal engine is independent.** Updating the app does not touch your
  workers. Updating the engine does, and Swarm defers it while sessions are
  running rather than doing it under you.
- **Sleeping workers cost nothing** and wake when work arrives.
- **One database file**, which you can copy.
- **It runs as you**, under systemd user services, on localhost, with one token.

## If something is missing that you relied on

Say so. The disposition of every Legacy capability is recorded in
`docs/26-legacy-evolution-atlas.md` — kept, redesigned, deferred, or removed,
with the reasoning. "Deferred" means nobody has needed it yet, which is an
argument you can win with a real case.
