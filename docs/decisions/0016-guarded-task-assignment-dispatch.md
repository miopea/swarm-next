# ADR 0016: Guarded durable task assignment dispatch

Status: **Accepted**

## Context

A Queen or operator can durably assign work, but a worker cannot act until it
is briefed. Ad hoc terminal injection would recreate the context-breaking
broadcast behavior Swarm is replacing. It would also lose work across API
replacement or duplicate a brief after a crash-ambiguous terminal write.

Task state, stable worker ownership, process binding, and briefing delivery are
separate facts. The durable task remains authoritative even when its worker is
sleeping or terminal delivery is delayed or uncertain.

## Decision

Assigning a task records the stable worker immediately. If she is running, the
same transaction creates one bounded task-dispatch outbox row. If she is
sleeping, her next session binding creates the first dispatch atomically.

- Durable ownership targets a worker profile and survives process stop, crash,
  reboot, and provider conversation recovery.
- Delivery targets the current immutable worker session. A stopped session is
  detached without clearing worker ownership; restart rebinds the new process.
- A worker already briefed before restart is not briefed a second time. Her
  provider conversation recovery carries the context; uncertain delivery still
  requires operator review.
- A live operator engagement lease leaves the brief Queued. Merely viewing or
  resizing the terminal does not block delivery.
- Reassigning or stopping a session cancels its still-Queued brief in the same
  transaction that releases the process binding. A claimed brief is never
  silently retargeted, and only explicit reassignment changes stable ownership.
- The API claims at most 16 briefs per pass, with no more than 256 active queue
  rows and 1,024 retained final rows. It serializes task and decision delivery
  within one API instance.
- The one-line terminal payload contains task identity, title, priority,
  workspace, bounded description, and an MCP retrieval hint. Control characters
  are replaced before the single terminal submission. The PTY transport writes
  the prompt, waits up to ten seconds for its bounded task marker to remain at
  the same canonical output sequence for 300 milliseconds, and only then sends
  Enter. It observes provider output for up to ten seconds after each of at
  most three Enter attempts. Active output, an operator question, or a new
  resting prompt after the marker proves acceptance. A stalled, still-editing,
  or otherwise unverified submission becomes Uncertain.
- A terminal-host write acknowledgement is necessary but not sufficient for
  Delivered. A definitive host rejection retries at most three times;
  unexpected or transport outcomes are Uncertain immediately.
- API startup converts interrupted Dispatching rows to Uncertain and never
  automatically replays them. `swarm_list_tasks` remains the recovery source.
- HTTP assignment and the MCP request wrapper attempt delivery immediately.
  The existing supervisor later provides liveness when an engagement lease
  clears; correctness does not depend on its timer.
- Delivery uses the existing protocol-7 `Write` request, preserving the running
  terminal sidecar during API and web updates.

## Consequences

The maturity program exempts an assignment's initial briefing (generation zero)
from the per-terminal coordination cooldown. A preceding message must not leave
an otherwise ready worker idle for five minutes before receiving new work.
Returned-work generations retain pacing. This changes neither claim eligibility
(operator engagement, other Active work, ordering, session and provider policy)
nor terminal observation, unsent-draft protection and acceptance verification.
No uncertain delivery is replayed by this exemption.

Schema 129 adds a nonnegative integer briefing generation, initially zero for
existing assignments. Returning Review/Blocked work to Active increments it in
the same task transition transaction. Overflow fails that transaction rather
than wrapping. Claims carry the generation; completion, deferral and failure
must match it before changing the outbox. Task/session identity alone cannot
authorize a result from an earlier briefing of the same returned assignment.
Prompt-hold subjects carry assignment plus generation and are projected against
that exact pending row. No provider process or conversation is restarted by this
transition. Rollback across this schema requires a compatible database backup.

The daily-driver maturity policy also gates new task briefings during Night
Watch using the builder-owned provider promotion list. Experimental and unknown
providers retain their queued briefings without consuming attempts; filtering
precedes the batch limit. Queues reports this policy hold explicitly. Ending
Night Watch restores ordinary eligibility without restarting or interrupting
the current worker. This does not cancel previously submitted work or replace
the separate guards required for startup, messages, and decision delivery.

Queen assignment now wakes the correct quiet worker without introducing a
worker-to-worker message bus. Operators can keep steering a worker without an
automation injection breaking context. Queue pressure and ambiguous delivery
are explicit instead of dropping or duplicating work.

This slice does not make terminal output the task-completion authority. Workers
continue to report Blocked or Review through typed MCP transitions; Queen or the
operator approves completion.

## Validation

Missing-briefing repair inserts at most the remaining active queue capacity,
ordered by task position and identity. A regression fills that bound, proves
further repair is a no-op, then frees a slot and verifies the remaining work is
admitted without loss. The repair path shares the normal 256-row active bound.

- Persistence tests cover sleeping-worker ownership, restart rebinding,
  engagement deferral, active-session targeting, acknowledged completion,
  queued cancellation on reassignment and stop, and crash recovery to
  Uncertain.
- Schema 19 backfills stable ownership from each active legacy session
  assignment without changing task history.
- A v11-to-v12 migration test proves existing databases gain the outbox.
- API integration assigns through HTTP and observes the brief in a real
  host-owned PTY.
- Browser component tests render every delivery state.
- Rolling deployment validation must preserve protocol 7, the terminal-host PID,
  the Queen worker identity, and her active session.
