# ADR 0060: Ops Console tickets are scoped external commands

Status: implementation in progress under the operator's approved Ops Console
phase 3 scope. Runtime enablement and scoped credentials are not configured yet.

Ops Console owns customer requests and original conversations. Swarm owns
implementation tasks, activity and deployment evidence. Several requests may
refer to one implementation task; their customer threads remain distinct.

The integration receives its own identity and an explicit app-to-workspace map.
It can submit inert draft tasks and read progress for its own tickets. It cannot
assign work, start workers, write terminal input, change task state, or borrow a
Queen/worker/browser-operator credential. MCP remains an adapter over application
services using the existing Rust MCP transport.

Each ticket carries integration identity, source app, console request ID and
conversation ID. The unique external key is integration/app/request. Swarm stores
that key and a digest of the normalized command in the same transaction that
creates the draft, task activity and control-room event. Identical retries return
the same task; changed content under an existing key is a conflict. No race or
lost response may create a second task.

Scope resolution precedes creation. Workspace comes from the authenticated
integration's approved mapping, never from a caller-supplied filesystem path.
Inputs, mapping count and progress pages are bounded. Revocation denies reads
and writes. Integration activity is attributed to its own identity.

The initial additive schema stores provenance in `ops_console_tickets`. The
existing activity model records `actor_kind=system` and the unambiguous
`ops-console:<integration_id>` actor ID. This preserves existing activity rows
and CHECK constraints without manufacturing a worker profile. The dedicated
integration identity and app scope are separate from all worker authorization.

Authorized commands expose only immutable accessors after domain validation.
An IMMEDIATE transaction serializes external-key lookup and creation across
independent database connections. Removed tasks retain their source keys; replay
returns the original receipt and never silently recreates removed work. Read
access refuses tasks moved outside the current approved workspace mapping.

Progress uses Swarm's task state, durable activity and deployment records. Closed
is not shipped; recorded deployment, evidence-based closure and unverifiable
closure remain separate facts. Reading progress cannot mutate task state.

The console uses a durable outbox with frozen reviewed commands, bounded retries
and recovery of leases after restart. Customer update generation remains separate
and sending always requires operator approval.

Validation must cover scope refusal, malformed commands, simultaneous identical
submissions, changed retries, transaction rollback, restart/replay, revoked
credentials, bounded progress and the distinction between closure and deployment.
An API-only release must preserve the running terminal host and active sessions.
