# 0139 — A roster look reads only what was drawn since

- Status: Accepted
- Date: 2026-11-01
- Decided by: the operator, who asked for the server follow-ups to ADR 0137 on
  2026-11-01 ("keep going with the server follow-ups after the watch").
- Builds on: ADR 0123, which combines the screen with Claude's turn report.
  Does not change what a look concludes, the terminal-host protocol, or how
  delivery reads a session before typing into it.

## Context

The roster (`GET /workers`) and the worker supervisor both ask each live
session's terminal what it is doing. Every time, for every session, they read
the whole snapshot. That snapshot is the visible screen plus up to a thousand
rows of scrollback, sent as JSON.

Measured on the Keeper with eleven live sessions:
- One `GET /workers` cost about 90 ms of API CPU and 80–105 ms of
  terminal-host CPU. Idle, both cost nothing.
- It moved 4.3 MiB of JSON for 1.3 MiB of snapshot bytes. Each snapshot was
  60–280 KiB.
- Each snapshot was then parsed two to four times, by classifiers that keep
  only the visible screen.
- Between looks 2, 5, 10 and 30 seconds apart, eight or nine of the eleven
  sessions had drawn nothing at all.

So most of that cost reread screens that had not changed. Every open control
room page fetches the roster again after almost every board event, and the
supervisor looks every thirty seconds.

## Decision

1. **A roster look first asks the host only for what was drawn since its last
   whole read.** The protocol already has this: a read names the sequence it
   holds.
   - **No frames:** the screen is unchanged, so the screen's answers from the
     last whole read are reused.
   - **Frames:** something was drawn, so the look reads the whole snapshot
     again and classifies it fresh, as it does today.
   - **A snapshot:** the host answers a read across a resize, or past what its
     journal still holds, with the snapshot itself, and that is used directly.
2. **Reuse is exact, not an approximation.** The host advances a session's
   sequence for every frame of output and for every resize. It answers with no
   frames only when it holds nothing newer than the sequence asked about.
   Session ids name one terminal for its whole life, so a sequence is never
   compared across terminals.
3. **Only the screen's answers are reused.** These are the activity the screen
   shows, the background work it lists, and whether Claude's end-of-turn line
   sits above the prompt. The turn report and the last-output time arrive with
   every answer, so a turn that changes without drawing anything is still
   seen. Fresh and reused readings go through one function that combines them.
4. **The readings are bounded and owned.**
   - They live in `AppState`, one per live session.
   - Only the roster refresh reads or writes them.
   - Each look keeps readings only for the sessions live at that look.
   - Looks hold the readings for their whole duration, so two pages looking at
     once do not both pay for a whole read. The second waits and finds nothing
     new.
5. **Delivery is untouched.** Before typing into a session, delivery still
   reads the exact current snapshot itself.

## Consequences

- A session that drew nothing costs one small round trip per look instead of a
  whole snapshot. A session that drew something costs a small read and then
  the whole read it costs today.
- The supervisor's thirty-second look saves the same.
- No terminal-host change, so no worker engine update is needed.
- A Claude session whose screen changed now always has its end-of-turn line
  checked. Before, that check ran only when the turn report needed it. It is
  one extra parse, and only of a screen that changed.

Tests:
- For every pairing of eight screens, two providers, five turn reports and two
  quiet times, a reused reading answers exactly as a fresh read of the same
  screen does.
- Against a real terminal host behind a counting proxy, these are the reads a
  look makes:
  - a first look, one whole read;
  - a look with nothing drawn, none;
  - a look after output, one delta read and then one whole read, which shows
    the new screen;
  - a look after a resize, the host's own snapshot and no second read;
  - a session that is no longer live, its reading dropped.
- Each of these fails its test when broken: always reading whole, reusing
  after frames, keeping readings for sessions that are gone, and dropping the
  end-of-turn check.

## Rejected

- **Applying the frames to a parser kept on this side.** It would make even a
  busy session cheap, but rebuilding a screen from a formatted snapshot and
  raw frames can drift from the host's. A frame can split an escape sequence,
  and the snapshot does not carry every mode. A drifted screen would misreport
  a worker until it redrew. Rereading on change cannot drift.
- **A host request for the visible screen alone.** It would shrink every read,
  but it changes the terminal-host protocol, so every worker would need an
  engine update. Whether to do that is the operator's call.
- **Reading less often.** That would trade freshness for cost. Reusing an
  unchanged screen costs no freshness.

## Not proven

- Not yet measured on the running Keeper: that needs a merge and a reload.
- Busy sessions still cost a whole read on every look that follows output.
- How often the control room fetches the roster is a browser follow-up. It is
  not changed here.
