# Domain model

Status: **Proposed**

## Core identities

### Operator

One authenticated person. An operator owns exactly one Hive. Presence is
tracked per operator and device; authorization is never inferred from presence.

### Hive

One operator's independently managed Swarm environment. It owns a personal
Queen, private workers, tasks, settings, execution nodes, and integration
identities. Apiary membership is optional and exclusive.

### Apiary

An optional federation of Hives with one Keeper. It owns membership, policy,
shared-project configuration, atomic task claims, cross-Hive routing, and
organization audit. It does not own Hive repositories or terminal processes.
An Apiary selects exactly one immutable shared-work backend: Jira or Native.

### Stewardship

A Keeper-granted scope over selected Hives, Jira projects, and capabilities.
Stewardship augments the operator's existing Queen. A Hive may have no primary
Steward and escalate directly to Keeper.

### Workspace

A configured environment in which tasks and workers operate. It identifies a
root, provider settings, security policy, and optional integration bindings.

### Worker

A durable operator-facing slot such as `alice`. A worker is configuration and
identity, not an operating-system process.

### Worker session

One immutable incarnation of a worker process. It has a unique ID, provider,
command, environment fingerprint, start/end times, and lifecycle state. A new
process always creates a new session even when it occupies the same worker.

### Terminal session

The server-owned terminal state associated with one worker session. It owns
the PTY, dimensions, canonical screen, cursor, alternate-screen state, bounded
scrollback, output sequence, and client attachment state.

### Task

A durable unit of intended work. A task has content, priority, dependencies,
source references, lifecycle state, and optional assignment. It does not own a
worker process.

A task has one home Hive. An Apiary-visible task may be unclaimed temporarily,
but execution begins only after an atomic claim or Keeper assignment establishes
one home Hive. Cross-Hive work is a handoff or linked contribution.

### Assignment

The time-bounded relationship between a task and worker session. Modeling it
separately preserves history and avoids contradictory fields on tasks and
workers.

### Task dispatch

The durable outbox record for briefing the worker-session named by an
assignment. Its Queued, Dispatching, Delivered, or Uncertain state describes
terminal delivery only; it never overrides task lifecycle or assignment truth.
Operator engagement defers a dispatch, and ambiguous delivery is not replayed
automatically.
### Activity event

An immutable record of a material domain transition. Events support audit,
diagnosis, UI resynchronization, and integration delivery; they are not an
excuse to make every subsystem eventually consistent.

### Decision request

A request for operator judgment with reason, risk, evidence, deadline, allowed
actions, and resolution. Queen proposals, approval escalations, and certain
integration conflicts converge here.

### Automation policy

An explicit rule controlling a permitted automatic transition. Policies are
versioned and auditable. Provider-native policy is referenced rather than
reimplemented where possible.

### Integration binding

Configuration and credentials connecting a workspace to an external system.
Integration adapters translate external events into application commands and
cannot directly mutate core tables.

Jira project bindings are owned by either one Hive or its Jira-backed Apiary.
Each Hive synchronizes with its operator Jira identity; Apiary coordination
distributes bindings and arbitrates claims without becoming the routine sync
actor. Native Apiary synchronization is a separate canonical adapter.

## Critical distinctions

- Worker name is not worker-session identity.
- Worker session is not terminal viewport.
- Task assignment is not task state.
- Provider activity is not inferred worker intent.
- Decision recommendation is not authorization.
- Activity events are not terminal byte streams.
- Browser connection is not authentication authority.
- Configuration source is not configuration precedence; imported configuration
  becomes one canonical stored representation.
- Apiary project ownership is not task home-Hive ownership.
- Apiary membership is not transferable task ownership.
- Structured oversight is not permission to stream or control a private
  terminal.

## Initial state-machine candidates

### Worker session

`starting -> running -> stopping -> stopped`

Exceptional exits: `starting -> failed`, `running -> failed`.

Provider observations such as working, waiting for input, rate-limited, or idle
are orthogonal activity attributes unless they control a real lifecycle
transition.

### Task

Proposed minimal model:

`draft -> ready -> active -> review -> completed`

Side paths: `ready|active -> blocked`, `blocked -> ready|active`, and
`review -> active` when changes are required. Removal is archival, not erasure.

The final state model requires a legacy transition audit and operator review.

### Terminal attachment

`detached -> attaching -> synchronizing -> live -> suspended -> resynchronizing`

Input is accepted only when the attachment is live and authorized for the
current worker-session ID.

