# ADR 0053: A session holding an older tool list

## Status

Accepted, 2026-08-24. Asked by task 01a03586 after `swarm_reload_app` shipped in
v0.8.4 and the worker that built it could not call it. The conclusion is that
this Hive cannot detect the condition, which that task allowed for explicitly —
"if the conclusion is that the protocol makes this unfixable and the right
answer is documentation, that is a legitimate outcome — record it with the
evidence rather than building something that only appears to help".

## The question

A tool is present in the server and absent from a running worker. Nothing says
so. Can this Hive detect it and report which tools a session is missing?

## What was measured

All four from the running 0.8.7 Hive, over the wire, using a worker's own
credential against `http://127.0.0.1:8766/mcp`.

**The premise was wrong.** A worker's tool schema is not fixed at connect. One
session enumerated its own surface twice in an afternoon: at the first
measurement `swarm_reload_app` was absent, at the second it was present, with no
worker restart in between. The server offered twelve tools to that credential
both times. So the gap closes on its own — eventually, silently, and after an
interval nobody chose.

**We advertise nothing.** `initialize` returns capabilities `{"tools": {}}`.
`ServerCapabilities::builder().enable_tools()` leaves `list_changed` as `None`;
declaring it needs `enable_tool_list_changed()`, which is not called. So no
client has ever been told this server's tool list can change.

**We could not send the notification if we declared it.** `initialize` returns
no `Mcp-Session-Id`, and `GET` on the endpoint answers `405 Method Not Allowed,
Allow: POST`. The transport is configured with `legacy_session_mode(false)` and
`json_response(true)`, and rmcp only serves the standing SSE stream under legacy
session mode or stateless replay. There is no server-to-client channel. A
`notifications/tools/list_changed` has nowhere to go.

**The server cannot see the condition.** It is stateless per request: it keeps
no record of what any session was served and never learns what a client
currently holds. Every `tools/list` it answers is current by construction. The
staleness lives entirely inside the client's model-facing schema, which this
process never observes.

## Decision

**Detect nothing. Declare nothing. Correct the comment and record this.**

Rejected: **declare `listChanged`.** The tool list genuinely is mutable inside
one process — `may_reload_this_hive` reads the caller's workspace from the
database, so moving a worker onto or off the development checkout adds or
removes `swarm_reload_app` for it, and Queen's block is role-dependent. The
capability is therefore applicable in principle. It is still refused, because
the transport cannot deliver the notification: declaring a capability we can
never exercise tells clients we will warn them and then never warns them, which
is worse than saying nothing.

Rejected: **infer staleness from session age.** The obvious heuristic — a
session that initialized before the current build may be behind — produces false
positives, and the measurement above is itself the counterexample: that session
predated the build and held the current list. Shipping it would report healthy
sessions as broken and teach people to ignore the signal.

Rejected, as the task instructed: **restarting workers automatically.** A
restart discards in-flight context.

## What follows from it

The comment on `CONFIG_SERVER_NAME` asserted the schema was fixed at connect.
That is the sentence somebody finds when a documented tool appears not to exist,
and it sent them to the wrong conclusion — that the session can never recover,
when in fact it recovers on its own. It now says what is true and points here.

The practical answer for a worker that suspects it is behind is a **pull, not a
push**: ask the server what it offers and compare. Any credential can do this
with one `tools/list` against the MCP endpoint. That is how the measurements
above were taken, and it is the only reliable check that exists today.

This belongs to the shape the task named — a failure that says nothing — with
one difference worth keeping. The others were silences we could close. This one
is a silence we can prove we cannot close from this side, and the honest record
of that is worth more than a detector that fires on the wrong sessions.

## What would change this

Any of three, none of them ours to decide alone: serving the standing SSE stream
(`legacy_session_mode` or stateless replay) so notifications have a channel;
a client that re-injects a changed MCP tool list into a running conversation on
a schedule it states; or the client reporting its own schema age, which is the
only place the answer actually lives.
