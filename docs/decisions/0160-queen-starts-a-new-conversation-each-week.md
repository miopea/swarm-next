# 0160 — Queen starts a new conversation each week

- Status: Accepted
- Date: 2026-11-05
- Decided by: the operator, 2026-11-05. Offered a rotation triggered by
  size for every worker, they chose a weekly rotation for Queen only. How it
  carries her context forward was Queen's choice, the same day: a fork,
  rotated only at a start Swarm already makes, with a sentence in her brief
  that carried context is a pointer and not authority.
- Builds on: ADR 0011 (a worker's conversation is pinned), ADR 0101 (a
  reported fork moves the pin), ADR 0109 (a lost pin tries the conversations
  that exist), ADR 0124 (a worker's history is what its own Claude reported).

## Context

Swarm resumes a Claude worker's pinned conversation at every start. Queen's
had been resumed since 2026-08-11, and a Claude conversation file only grows:

- Compaction appends a summary, and Claude loads only what follows the last
  one. Every earlier line stays in the file.
- Claude's own cleanup removes a conversation only after it has gone
  untouched for its cleanup period, 30 days by default. A conversation that
  is resumed at every start is never untouched.

On 2026-11-05 Queen's file was 4.37 GB, with another 3.4 GB in its side
folder of saved tool results and subagent transcripts. About 5 MB of it
follows the last compaction. It grows about 1 GB a month.

Queen starts often, and Swarm does not choose when. Over the 28 days to
2026-11-05 she started 37 times, from engine updates, host restarts and
recoveries, never more than 3.6 days apart.

Measured on a throwaway conversation with Claude Code 2.1.318:

- `claude --resume <old> --fork-session --session-id <new>` writes a new
  file under the id Swarm chose. It begins at the old conversation's last
  compaction: the summary and the turns after it, nothing earlier. The old
  file's size and modification time are unchanged.
- That start's SessionStart hook reports source `fork` and the new id.
- Copied messages keep their message ids and timestamps. The usage panel
  counts a message id once across all files, so a fork is not counted twice.

Queen weighed two ways to carry her context forward: a fork, which keeps the
summary and recent turns, or a fresh conversation primed with a short note.
She chose the fork, on three conditions:

- It happens only at a start Swarm already makes, never by stopping her, and
  not while an automation run or a takeover pause is live, if that can be read
  cheaply.
- Her brief says that carried context is a pointer, not authority. A
  compaction summary had already carried a premise forward that the decision
  store contradicted.
- If a fork launch fails, she resumes the old conversation and the next start
  tries again.

## Decision

At each start of Queen, Swarm decides whether to resume her conversation or
fork it into a new one.

- **When.** A rotation is due when the pinned conversation is a week old by
  the rotation that started it (`QUEEN_CONVERSATION_SECONDS`). A pin no
  rotation started is due at once: it predates rotations, or the operator or
  Claude moved it, and nothing says it is young. Only Queen, only Claude, and
  only a conversation that has already run.
- **Held.** A due rotation waits for a later start while an automation run
  that reached Queen is unfinished (`delivering` or `running`), or while a
  Steward takeover lease is active. Each is one row, read without running the
  automation's own expiry steps. A queued or uncertain run does not hold it,
  so the rotation is never put off indefinitely.
- **The pin moves first.** `rotate_conversation` moves the pin from the old
  id to a new one Swarm chose, and records the rotation, in one transaction.
  It refuses if the pin has already moved or Queen has a live session. The
  start then binds the session to the new id as to any other.
- **The fork.** The start asks Claude for
  `--resume <old> --fork-session --session-id <new>`, the new start mode
  `ClaudeConversationStart::Fork`. The engine's lifecycle gate is told the id
  it launched, so the SessionStart that reports source `fork` with that id
  settles as this start, not as a conversation change.
- **Only an engine that reads it.** A fork is protocol 19. The API still
  drives an engine from protocol 17 (ADR 0157), so it pings the engine before
  rotating and asks for a fork only of one on 19 or later. Any other answer,
  or an error, resumes her as before and the next start asks again. The engine is
  pinged only once a rotation is otherwise due.
- **Never fails a start.** Every refusal above resumes the old conversation.
- **Falling back.** If Claude cannot open the old conversation, the engine
  goes down the ADR 0109 ladder as for any lost pin and the pin follows what
  opens. If the fork's Claude dies before it writes the new file, the next
  start's resume finds no such conversation and the ladder resumes one that
  exists, as for any lost pin. No rotation started that pin, so the start
  after it forks again.
- **The brief.** Queen's standing brief carries her sentence, as
  `QUEEN_CARRIED_CONTEXT_GUIDANCE`: a compaction summary, a memory or a note
  says where to look, not what is true, and rulings, decision answers and
  task state are re-verified from the decision store and the board before she
  acts on them.
- **The trail.** `worker_conversation_rotations` keeps each rotation's old
  id, new id and time, the newest 16 per worker (schema 223).

## Consequences

- The old file stops being written at the rotation. Claude's cleanup removes
  it once it has gone untouched for its cleanup period.
- Queen's live file holds what the fork copied plus the growth since. At the
  rate she starts that is about a week and a half at most, longer if a hold
  keeps putting it off. Her side folder of tool results stays with the old
  conversation.
- A fork carries only what follows the last compaction. Anything older that
  Queen still needs has to come from the board or the decision store, which is
  what her brief now says.
- Other workers are unchanged.
- The app installs alone (ADR 0157). Queen keeps resuming as today until the
  operator applies the worker engine update, and then her next start forks.

## Evidence

- Persistence tests: a rotation moves the pin and dates the new conversation,
  refuses a running worker and a pin that already moved, a pin moved elsewhere
  has no rotation date, and the trail stays bounded. The hold is nothing on a
  quiet Hive, a run only once it reached Queen, and a takeover only until its
  lease expires.
- Application tests: due after a week or with no rotation date, never for
  another worker, a worker that never ran or a provider other than Claude, and
  put off by either hold.
- Terminal tests: the fork's arguments, that no other start reads as a fork,
  the start modes the protocol pins, the fork gate, and the lifecycle gate
  settling only the fork it launched. A process spawned with the fork's
  arguments settles its startup when it reports that fork with the
  capability it was handed; it fails if the spawn or the gate setup drops
  the id.
- Engine tests: a fork from a conversation Claude holds goes ahead; one Claude
  cannot open resumes what it can.
- API tests, against a fake engine: a due Queen forks on protocol 19 and the
  pin and trail move; on 18 she resumes and nothing moves; a conversation
  rotated this week is resumed; an automation run that reached her puts the
  rotation off. Removing the hold or the protocol
  check fails a test.
- The fork itself was measured on Claude Code 2.1.318, above.

## Not proven

- No rotation has run on a Hive. That needs this release reloaded, the engine
  update applied, and Queen's next start after it.
- That the old file's modification time stops advancing after a rotation, and
  that Claude's cleanup then removes it.
- When a fork's Claude dies at launch, Queen resumes at her next start, not
  within the failed one. This change adds no restart of its own, and which
  conversation the ladder then resumes was not driven end to end.
