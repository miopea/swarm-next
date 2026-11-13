# 0173 — Swarm settles its own unconfirmed message writes

- Status: Accepted
- Date: 2026-11-13
- Decided by: the operator. A report said workers' messages to Queen stopped
  arriving while her session sat idle, and every task waiting on her stalled.
  The report asked for delivery within a minute. Shown that Queen's terminal
  takes one paced write every five minutes, the operator chose "Oldest first":
  pacing stays, the item that has waited longest goes first, and the target
  becomes delivery within one cooldown. They then approved the changes below.
- Amends:
  - ADR 0067. An uncertain message was never sent again unless Queen
    reconciled it. Swarm now settles one itself when the recipient's prompt
    shows what happened, and sends one again at most once, labelled.
  - ADR 0168. A framed paste is now led by a short typed marker.
  - The order of one coordination pass. Outcome reports always went before
    messages, and a Queen review that had yielded once went before both.

## Context

Measured read-only on the operator's Hive on 2026-11-13, from the delivery
tables, the API journal, and one read of Queen's screen the operator allowed:

- **Six stalls since 2026-11-09, each caused by Swarm's own write.**
  - A long message reached Queen's prompt as a framed paste (ADR 0168) after
    delivery had stopped watching for it. Delivery waits ten seconds, then
    records the write as uncertain and never presses Enter.
  - Claude shows a framed paste as a chip, `[Pasted text #1 +40 lines]`. The
    chip hides the delivery's marker, so nothing could tell the paste was
    Swarm's.
  - Every later delivery to Queen saw unsent text it could not attribute and
    held, as it must for an operator's typing. Deliveries were held for up to
    about nine hours.
  - In the latest stall, Queen's terminal drew about one byte in the ten
    seconds delivery watched. Why it drew so slowly is not known, and nothing
    below depends on it.
- **Uncertain messages had no way out but Queen.** 82 message deliveries were
  uncertain, 77 of them from workers to Queen. Only Queen could move one, by
  reconciling it, and she was the one not receiving them. Outcome reports and
  Queen's automation already retry their own uncertain writes. Messages did
  not.
- **With the prompt clear, pacing starved messages.**
  - Queen's terminal takes one paced write per five-minute pause, and the
    first queue to write in a pass takes it.
  - Outcomes were always tried before messages, and a review that had yielded
    once went first.
  - For 55 minutes, outcome reports and review runs took turns while workers'
    messages, older than both, waited.
- **A held message was not listed.** Held outcomes and reviews record a
  delivery hold, which shows in Queues; held messages recorded none. The
  holds that did show told the operator to "clear the line", which would have
  deleted the stranded messages.

Measured on Claude Code 2.1.325 in a throwaway session (Queen runs 2.1.323):

| Write | Claude's prompt |
| --- | --- |
| `[Swarm delivery <id>] `, then a 40-line framed paste, in one write | `❯ [Swarm delivery <id>] [Pasted text #1 +40 lines]` |
| The same, as two writes 300 ms apart | The same |
| The first, then Enter | Sent as one message, lead and paste together. The reply answered the paste's last line. |

## Decision

1. **A framed paste is led by its marker.**
   - The lead `[Swarm delivery <id>] ` is typed in front of the frame, where
     `<id>` is the first message the write carries. Control bytes in the
     marker become spaces.
   - The lead is drawn before the paste behind it. So for a framed write, only
     the new chip proves the render. Seeing the lead alone would send it and
     strand the paste.
   - An unframed write is typed whole, so its marker still proves the render,
     as before.
2. **One write, one claim.** The messages a pass writes to one terminal share
   one claim, so the write can be found again whole.
3. **An unconfirmed write is settled from its recipient's prompt.**
   - This happens at the start of every pass, before anything it would hold.
   - It reads each live terminal holding uncertain message writes once, under
     the same policy and activity checks as any delivery. A terminal the
     operator is engaged in is skipped, as a delivery to it would wait: what
     follows Swarm's text in that prompt may be theirs, half typed.
   - **The write's marker is unsent in the prompt.** Swarm presses Enter once,
     the write's messages are delivered, and the terminal's cooldown starts.
     This is how a delivery already sends its own unsent message.
   - **The prompt is empty and has drawn since delivery gave up.** The write
     is not waiting there. Its messages are queued once more, and each says
     so: "Swarm could not confirm an earlier delivery of message `<id>`, so you
     may already have it. Act on it once."
   - **Anything else** leaves the write for the next pass. That covers
     somebody's text in the prompt, a terminal that has drawn nothing since,
     and a busy provider.
   - A resend is the write it repeats: the same messages in the same order,
     written on their own. A newer message waits for the next write.
   - A message already sent a second time is never sent a third time. A
     superseded one is never sent again. Both stay uncertain for Queen, as
     ADR 0067 describes.
   - Bounded: at most 64 uncertain messages per pass, one Enter per terminal
     per pass, and live sessions only.
4. **Queen's oldest wait goes first.**
   - Each pass reads, for Queen's live terminal, when her oldest queued
     message, her oldest queued outcome, and her queued review began waiting.
   - Messages go before outcomes when the oldest message is older.
   - A review that has yielded once loses its head start to an older message.
   - A tie keeps the old order. This is ordering only: cooldown, engagement,
     provider and prompt checks all still apply.
5. **A held message is listed, and nothing says "clear the line".**
   - A message held at a prompt is recorded as a delivery hold, one per
     terminal, because one prompt holds every message waiting there. It shows
     in Queues under Delivery waits, beside held outcomes and reviews. Like
     them, it stays out of Needs You (ADR 0067).
   - The hold clears on a delivery to that terminal or a busy provider. It
     stops showing once no message waits there, however the message left.
   - The reason now says the prompt holds unsent text Swarm cannot match to
     one of its deliveries, and asks the operator to send it or clear it. The
     attention card's wording for this hold is changed to match, although the
     app routes these holds to Queues rather than to that card.

## Consequences

- A paste stranded in a prompt costs one pass, not hours. The next pass finds
  its marker and sends it.
- A resend can repeat a message a recipient already has. It happens only when
  the prompt is empty and has drawn since the first attempt, and the message
  says it may be a repeat.
- A resend can meet the original stranded in the prompt, if the original drew
  late. Both lead with the same message, so the resend finds its own marker
  unsent and presses Enter on the original instead of writing again. That
  Enter delivers exactly what the resend carries, because a resend carries
  nothing the original did not. Mixing in a newer message would have recorded
  it delivered without ever writing it.
- No schema change. A resend is recorded in the delivery row's existing
  resolution reason.
- **On the first pass after this release, uncertain messages on live
  terminals are settled.** Any whose marker is unsent in the prompt are sent.
  Those whose prompt is empty are sent once more, labelled. A reader may
  already have handled them.
- Queen still reconciles what Swarm cannot settle, from the same attention
  list. That covers a second loss, a superseded request, and a write to a
  session that has ended.
- A message to a worker gets the same recovery. The worker's prompt is read
  the same way.

## Not proven

- Queen's own long-running session on 2.1.323. The lead and chip were
  measured on 2.1.325 in a fresh session.
- Why her terminal drew so slowly at the latest stall.
- The ordering is covered by unit tests of the wait comparison, not by a pass
  that delivers an older message ahead of a newer outcome end to end.
