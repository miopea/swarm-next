# 0109 — A worker that lost its conversation tries the ones that exist

- Status: Accepted
- Date: 2026-09-22

## Context

A worker whose pinned conversation Claude no longer holds cannot start. Reported
from the field on 2026-09-22 and still failing after three attempts to fix it.

The sequence is: Swarm starts the worker with `--resume <pinned>`; the terminal
host probes Claude and is told there is no such conversation; the host rewrites
the start to `--continue`; Claude answers "No conversation found to continue";
the process dies; and the worker goes back to sleep.

`--continue` is the problem, and it is the problem in two separate ways.

**It asks Claude to choose, and Claude may decline.** `--continue` means "the
most recent conversation in this directory". When Claude's own view of that
directory is empty — pruned, moved, a different config directory — there is
nothing to continue and the start fails. Swarm then has no second idea.

**It never yields an id.** Even when `--continue` succeeds, Swarm learns nothing
it can pin, so the worker stays adrift and the same rewrite happens next time.
A worker that recovers by `--continue` is a worker that will need recovering
again.

Meanwhile Swarm already knows how to enumerate what Claude actually has:
`conversation_freshness` reads Claude's transcript store to judge whether a
worker's pin is stale. That machinery answers the question `--continue` was
being asked to guess at.

⚠️ AND IT MUST BE ASKED IN THE RIGHT PLACE. Enumerating from swarm-api is the
bug this codebase already fixed once: swarm-api has its own `CLAUDE_CONFIG_DIR`
while Claude is spawned by the terminal host and inherits THAT service's, so a
scan from the API can report a conversation gone that Claude would open, and a
`New` start then reuses a pinned id Claude still holds and is refused.

## Decision

When the pinned conversation cannot be opened, the terminal host looks for the
conversations that DO exist for that workspace, newest first, and resumes the
first one Claude confirms it can open. Only when none can be opened does the
existing ladder continue to a fresh conversation.

Three tiers, in the operator's words: try to open it; fall back to resuming from
the list of what is really there; fall back to a new session.

The enumeration runs **in the terminal host**, in the environment Claude runs
in, and each candidate is confirmed with the same read-only probe already used
for the pin — `claude --resume <id> --print ""`. Existence of a transcript file
is not taken as evidence that Claude will open it; that inference is what made a
directory scan untrustworthy in the first place.

The search is bounded. At most a few candidates are probed, newest first,
because each probe spawns Claude and a worker that takes a minute to start is
its own failure. A workspace with a hundred stale transcripts must not turn one
failed start into a hundred subprocesses.

The recovery record says `Exact` against the id actually tried. The host
substitutes a better conversation BEFORE the ladder begins rather than inventing
a new rung, so what is journalled is what was attempted, and a recovery that
later succeeds names a real conversation that can be pinned.

## Consequences

- A worker whose pin went stale keeps its history instead of starting over,
  whenever any of its recent conversations is still openable.
- The worker stops drifting: the conversation that worked is a concrete id,
  which is what `--continue` could never supply.
- A fresh start remains the floor and is unchanged, so the worst case is what
  happens today minus the dead end.
- Starting a worker can now cost a few extra probes in the failure case only.
  The success path is untouched: a pin that opens is used immediately and
  nothing is enumerated.
- The path-encoding for Claude's transcript directory moves to `swarm-domain`,
  because two crates now need it and a second copy that forgets the `.` variant
  returns an empty listing rather than failing — which every caller reads as
  "no transcripts".

## Alternatives rejected

**Keep `--continue` and retry it.** It fails because Claude has nothing to
continue; retrying asks the same question again.

**Enumerate in swarm-api and pass the id to the host.** The two processes can
have different `CLAUDE_CONFIG_DIR`s. This is the exact shape of the bug the
`New` arm was introduced to fix.

**Trust the transcript files without probing.** A file on disk is not a promise
that Claude will open it. The probe already exists and costs about a second.

**Probe every transcript.** Unbounded subprocess spawning on a failure path, on
a machine that is already not starting a worker.
