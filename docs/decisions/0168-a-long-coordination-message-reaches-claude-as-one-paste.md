# 0168 — A long coordination message reaches Claude as one paste

- Status: Accepted
- Date: 2026-11-10
- Asked for by: Queen, routing a report that messages to her "go uncertain in
  batches and nothing says so". The operator, on seeing one: Swarm "thought I
  was mid typing" and stopped delivering.
- Amends: ADR 0067. Delivery still never replays an uncertain write. This
  changes only how the one write is framed.

## Context

Coordination writes a message into the provider's prompt as raw bytes, waits
to see it rendered, then presses Enter. Claude Code enables bracketed paste
(`ESC[?2004h`), but the bytes arrived unframed, so Claude had to guess what was
a paste.

Measured read-only on the operator's Hive on 2026-11-10, from Queen's terminal
history and the delivery table:

- Queen's session had four uncertain batches. In three of them her prompt
  ended up holding only the batch's last characters, as typed text, with no
  paste chip and no marker. The three tails were about 712, 273 and 714
  characters. In one, of a 30,920-byte batch, the prompt kept the last 712
  bytes, starting mid-word.
- Seeing neither chip nor marker, delivery declared the batch uncertain, as it
  should. The text left in the prompt then held every later delivery as
  operator typing. When it was finally submitted, Queen received only the
  tail.
- The fourth was different. The host was overloaded at the time (terminal
  host broken pipes, timed-out requests), and nothing was drawn at all. This
  decision does not address it.

Claude Code 2.1.322, which Queen's process runs, decides paste or typing for
each stdin read on its own. A read longer than 800 characters is a paste, and a
shorter one is typed. While a paste is being processed it holds back Enter, but
not typed text. A long write can reach it as several reads. When the last read
is short, it is typed. All three tails were under 800.

Reproduced with the same bytes against the same Claude, in a fresh session:

| Write | Claude's prompt |
| --- | --- |
| Raw, head, then the last 712 bytes 3 ms later | `[Pasted text #1 +325 lines]` then the 712 bytes as typed text, cut at the same character |
| Raw, in 1 KB pieces 20 ms apart | about thirty chips, then typed text |
| Framed, same split | `[Pasted text #1 +329 lines]` |
| Framed, 1 KB pieces 20 ms apart | `[Pasted text #1 +329 lines]` |

A framed paste is gathered across reads into one event. Claude Code frames its
own injected input to a terminal the same way.

## Decision

When the prompt delivery reads is Claude's, and its snapshot shows bracketed
paste on, a coordination message longer than 800 bytes is written as one
framed paste: `ESC[200~`, the message, `ESC[201~`. Enter is still a separate
write, sent only after the render is seen.

- Any escape byte inside the message becomes a space first. `ESC[201~` ends
  the frame, so a message carrying one would end its own paste early and have
  the rest read as keys.
- A message of 800 bytes or less is written exactly as before. Claude types it
  whole, and its marker stays visible, which is how an unsent copy of it is
  recognised later. Claude collapses even a short framed paste into a chip,
  which would hide that marker.
- Other providers keep the unframed write. Nothing here has measured how they
  read a frame. One that never asked for framing would read it as an Escape key.

## Consequences

- It applies to every terminal, not only Queen's. Every coordination delivery
  goes through the same write, so all of them can be framed: task briefs,
  task messages between Queen and workers, task outcomes to Queen, decision
  deliveries, clarifications, operator broadcasts, Queen's automation and
  review focus, and the restart and reconnect notices.
- A long message arrives whole however its bytes are split. It renders as the
  new paste chip that delivery already accepts as proof of a render. A long
  unframed message was already a chip whenever it arrived as one read, so on
  most deliveries an operator sees no difference.
- Where framing was not asked for, nothing changes:
  - Claude with bracketed paste off, or switched off again, gets the unframed
    write.
  - Every other provider does too.
  - A prompt whose snapshot cannot be read is still not written to at all.
- What it can interrupt: nothing new. The frame adds no key press. It is sent
  only after the baseline has found an empty, resting Claude prompt, and only
  when that prompt asked for framed pastes.
- A framed write that is cut off part way, for example by a host that stops
  answering, leaves the opening frame without its close:
  - Claude holds what arrived as a paste in progress. Two seconds after the
    last byte, it puts what it has in the prompt as one unsent paste.
  - Inside those two seconds, anything else typed into that terminal joins the
    paste rather than acting as a key, Enter included.
  - Delivery reports that write as uncertain and does not replay it. The unsent
    paste then holds later deliveries as unsent text, as a cut-off unframed
    write already did.
- A message that has already gone uncertain is still settled by Queen. This
  removes the cause measured above. It does not make an uncertain write safe to
  replay.
- Not proven: a framed batch to Queen's own long-running session. The cut was
  observed there and reproduced in a fresh session. The head disappearing
  entirely, rather than staying as chips, was observed only in her session, and
  why is not known.
