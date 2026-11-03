# 0148 — A command card never waits for an Allow that cannot grant

- Status: Accepted
- Date: 2026-11-03
- Decided by: the operator (2026-11-03). Shown that two of their Allows had
  granted nothing because the cards were filed on a task that had already
  closed, they chose "Build it now".
- Amends: ADR 0129. A grant still dies with its task and with the session
  that asked. Now the card asking for one does too.

## Context

ADR 0129 binds a command grant to the task and the session that asked. The
resolve makes no grant once the task has left the board or the session has
ended. Nothing stopped a card from being filed on closed work, and nothing
took a card down when its task closed or its session ended. Such a card
stayed in the operator's inbox, and pressing Allow on it resolved the card
and granted nothing. The worker was then told "No grant was made", and asked
again.

Measured read-only on the operator's Hive on 2026-11-03, over every command
card resolved with Allow (97 in all), with each task's close and each
session's end read from the record:

| Outcome | Count |
| --- | --- |
| Granted | 93 |
| No grant: the task had closed before the card was filed | 3 |
| No grant: the asking session ended before Allow was pressed | 1 |
| No grant: the task closed while the card waited | 0 |

Two of the three were filed 23 and 77 minutes after their task completed.
They waited about six hours, the operator pressed Allow on both, and the worker
asked again for one of them. The session case ended 70 seconds before the
press.

## Decision

1. **A command card is refused on closed work.** Filing one on a task that is
   completed, abandoned or removed returns `command_request_task_closed`, and
   nothing reaches the inbox. The worker hears it at once and asks on the
   open task the command is for.
2. **Closing a task takes its waiting command cards down,** in the same
   transaction that closes it. All four writes that close a task do it: the
   lifecycle transition to Completed or Abandoned, the release settle, the
   Jira sync and removal.
3. **Ending a session takes its waiting command cards down,** in the same
   transaction that ends it. All three writes that end a session do it:
   release, a terminal missing from the host, and a continuation handing
   over to a successor. A successor never inherits a grant (ADR 0129), so it
   does not inherit the card either.
4. **The card is withdrawn, not resolved.** The operator decided nothing. The
   card says why Swarm withdrew it, names no worker as the one who withdrew
   it, and the inbox is told to refresh.
5. **Only command cards.** A question or a ruling can still have an answer
   after its task closes or its asker restarts, so it stays up.

## Consequences

- The resolve's own checks, which make no grant for closed work or an ended
  session, stay. They are now reached only by a write outside the paths
  above.
- A task brought back after removal does not bring its withdrawn cards back.
  The worker asks again.
- A worker that restarts asks again for a command it still needs. Before,
  the operator pressed Allow, nothing was granted, and it asked again
  anyway.
- The worker is not sent a message when its card is withdrawn. The card reads
  withdrawn, with the reason, in `swarm_list_decisions`.

## Not proven

- **On the Keeper.** This needs a reload. Prediction: no more command cards
  resolved with Allow that grant nothing. This is wrong if one appears after
  the reload.
- **A worker waiting on a withdrawn card.** One whose session continues
  while its task closes is not told, and could wait for an answer that will
  not come. No such case is on the record.

## Evidence

Tests:

- `a_command_request_is_filed_only_on_an_open_task`: refused on a completed,
  an abandoned and a removed task, with nothing added to the inbox; a
  question on the completed task and a command card on an open one still
  file.
- `closing_a_task_any_way_takes_its_command_cards_down`: completed, abandoned,
  removed, released and closed in Jira, each through its own public path.
  Each time, the command card is withdrawn with the reason and no worker. An
  inbox event is written, and Allow finds nothing to press. A question on the
  same task and a command card on another task stay up.
- `a_session_ending_takes_its_command_cards_down`: released, and missing from
  the terminal host. The card is withdrawn with the session reason. A
  question, and another session's card, stay up.
- `a_continuation_handover_takes_the_previous_sessions_command_cards_down`.
- `a_grant_ends_with_its_task_or_its_session_and_belongs_to_one_worker`: the
  resolve still grants nothing for closed work or an ended session. Its
  session now ends by a raw write, since every path that ends one takes the
  card down first.

Ablations: each change below fails a test, and the file is restored byte for
byte afterwards.

- No check when a card is filed.
- The transition takes cards down only on Completed.
- No takedown on:
  - the transition;
  - removal;
  - the release settle;
  - the Jira sync;
  - release of a session;
  - a terminal missing from the host;
  - a continuation handover.
- No inbox event.
- Questions are taken down too.
- Every task's and session's cards are taken down.
- A session's cards give the closed-task reason.
