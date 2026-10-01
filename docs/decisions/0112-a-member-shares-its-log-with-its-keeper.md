# 0112 — A member shares its log with its Keeper

- Status: Accepted
- Date: 2026-09-29
- Decided by: the operator, on 2026-09-29: "the keeper should have access to
  logs on the connected hives", and, asked whether a member may turn it off,
  "Always on".

## Context

Three releases in a row fixed Apiary faults that the operator found by hand. Each
time, the member's own log held the explanation:
- a sync step refused and never retried;
- a watch whose sender never started;
- keystrokes refused with "requires generation-bound control".

That log was on the member's machine, and the refusal was only at debug level.
The Keeper, which is where anyone diagnosing the Apiary sits, saw nothing. The
operator's words: "your error checking and logging system isn't what it needs to
be. I shouldn't have to do all your leg work."

## Decision

1. **Every member keeps a bounded in-memory copy of its own log.** A tracing layer
   with its own fixed filter feeds it. It keeps Swarm's lines at INFO and above,
   plus DEBUG from the Apiary connection's own modules (the doorbell, and the
   watch and takeover senders). It holds 1,000 lines, and each message is cut at
   1 KiB. It does not follow `RUST_LOG`, so what a Keeper can read does not
   depend on how a member was started.
2. **A member sends new lines to its Keeper on every sync pass**, before the
   pacing and refusal gates. The member stuck in a backoff is the one whose log
   matters. A batch carries up to 200 lines in order, a per-process `boot_id`,
   and a count of lines lost before they could be sent. The Keeper answers 204. A
   failed send is logged only at DEBUG, which the shared log does not keep, so
   the send cannot feed itself.
3. **The Keeper keeps a bounded copy per Hive and writes every line to its own
   journal.** The journal is under target `swarm_api::member_log`, with the Hive
   id, the member's module and the member's timestamp. The journal is the
   durable copy. The in-memory view empties on a Keeper restart.
4. **The roster shows it.** Every row on the Keeper's Hive list has a Logs
   button: a member's shared log, or the Keeper's own for its own row. The view
   is newest first and can be filtered to warnings and errors. It says when the
   Keeper last heard from that Hive and how many lines were lost.
5. **Always on, and said so.** A member cannot turn it off. Its Apiary page and
   Settings state that it shares its log, and what that includes.

## What is shared, and what cannot be

Swarm's own log lines: messages and their fields, as the journal shows them.
They can include worker names, repository paths and error text. They never
include terminal output or input:
- the terminal engine's log is not captured;
- the API process never logs frame contents (ADR 0107);
- takeover keystrokes are never logged, only the reason one was refused.

A future log line that carries content has to be written knowing it travels.

Asked on 2026-10-01 whether paths and names should be stripped from Hives run by
someone other than the Keeper's operator, the operator ruled "Share as is":
paths and error text are what explain a failure, and stripping them would hide
the cause.

## Consequences

- A fault on a member can be read from the Keeper within one sync pass, and the
  two-Hive acceptance run checks that it can.
- A member on an older release sends nothing, and its Logs view says so. An
  older Keeper answers 404, and the member tries again each pass with the same
  bounded batch.
- Each connected member adds at most one small request per pass when it has
  something new, and nothing when it doesn't.
- This is a change to what crosses the trust boundary between two operators'
  Hives. In an Apiary of other people's machines it applies to them too, which
  is why the member's own screen says so.
