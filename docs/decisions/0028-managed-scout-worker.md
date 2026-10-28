# ADR 0028: Managed Scout worker identity

Status: **Accepted**

## Context

Queen needs a safe way to delegate deliberate cross-repository preparation and
worktree setup. Treating the existing project-root worker as an ordinary roster
entry makes that capability easy to rename, remove, or confuse with a
repository owner. Giving it Queen authority would instead blur the clean
operator → Queen → worker hierarchy.

Existing dogfood installations may already have a durable `Project Root`
worker with a valuable provider conversation and task history. Replacing it
would discard exactly the continuity Swarm promises.

## Decision

Swarm records **Scout** as a managed worker system role separate from its
authorization role. Scout remains an ordinary worker for task and coordination
authority. The system role only protects its durable product identity and
presentation.

On startup, Swarm may promote an exact, active `Project Root` worker at the
configured projects root. Promotion:

- keeps the worker ID, provider conversation, terminal history, task history,
  description, and provider;
- changes its display name to `Scout` and marks the managed system role;
- leaves it sleeping by default rather than consuming provider memory;
- pins it directly after Queen in the roster; and
- prevents rename, removal, and ordinary drag ordering while continuing to
  allow provider, description, and always-active policy changes.

Promotion fails safely when the exact worker and workspace do not match. It
does not guess from similar names or create a duplicate. Fresh-install Scout
provisioning remains a separately tested installation concern.

## Consequences

- Existing operators keep continuity while gaining a recognizable protected
  cross-repository worker.
- Queen can delegate broader preparation without granting Scout Queen powers.
- Sleeping Scout adds no loaded provider process or terminal-memory cost.
- The managed marker is explicit in persistence and API contracts rather than
  inferred forever from a mutable name.

## Validation

### Queen routing evidence (daily-driver maturity)

The Queen-only MCP roster includes `scout_routing`: a single-query durable
snapshot of the managed Scout ID, open session binding, operator engagement
at the supplied observation time, and active-task ownership. A worker merely
named Scout does not qualify. Missing managed identity is explicit null.
The existing worker array remains unchanged.

This snapshot is neither a reservation nor a terminal-idle observation. It
explicitly reports terminal activity as unobserved and grants no dispatch
authority. Read failure returns an error rather than an optimistic idle state.
Delivery must still revalidate engagement and live terminal evidence, and
second-opinion admission protects ongoing tasks.

Queen's durable task-message boundary now treats message purpose as an
admission rule. The default `assigned_task` purpose requires the exact current
task assignee. The only exception, `scout_second_opinion`, requires the exact
protected managed Scout identity, an open session, no live operator engagement,
and no Active assignment in the same database transaction that records the
message. Swarm does not wake Scout solely for an opinion. A worker merely named
Scout does not qualify. Guarded delivery rechecks current work and engagement
before terminal input, so becoming occupied after admission holds the message
rather than interrupting Scout.

Persistence tests prove exact-only in-place promotion, stable identity and
conversation, sleeping default, pinned ordering, protected rename/removal, and
continued provider/description configuration. Admission tests prove ordinary
questions cannot be rerouted to peers and exercise every managed-Scout refusal
before the available path. API and browser tests expose and render the managed
role without treating it as Queen.

### Scout answers the second opinion it was asked (2026-10-27)

The send leg admitted Scout on work it does not own, and the answer leg did
not. `swarm_message_queen` accepted only the task's assignee, so on 2026-10-27
Queen asked Scout about an unassigned draft, the question was delivered, and
all three ways Scout tried to answer were refused with words about review
replies.

The answer leg now admits the managed Scout on a task where a message from
Queen to Scout has been delivered. A reply id, when given, must be that
message. The answer is a message to Queen and nothing more: it moves no review
marker and cannot answer for the assignee. Scout gains no notes, transitions
or other writes on that task. A question still waiting for delivery has not
been asked, and no other worker Queen has messaged is covered, a former
assignee included. Messages do not record their purpose, so the rule reads any
delivered Queen message to Scout on the task, which also covers Scout as a
former assignee; that answer still moves nothing. Every other non-assignee is
refused with words naming the rule, not review replies, and a task that does
not exist is refused the same way, so the refusal reveals nothing about which
tasks exist.

The alternative, refusing a second opinion at send time, would have removed
the capability this ADR accepted. Persistence tests cover the admitted answer,
the undelivered, former-assignee, bystander, wrong-reply-id and missing-task
refusals, and that the draft's state does not move.
