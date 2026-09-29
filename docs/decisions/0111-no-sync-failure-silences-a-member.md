# 0111 — No synchronization failure silences a member for good

- Status: Accepted
- Date: 2026-09-27
- Supersedes: ADR 0030 ("invalid, oversized, foreign, or unsupported responses
  halt federation reconciliation as incompatible") and the capability-inventory
  line "halts authentication or protocol failures for operator action"

## Context

A member Hive stopped reaching its Keeper and stayed stopped for days. It still
ran, and its operator's screen said only "Shared setup needs attention". Four
symptoms were really one:

- the Keeper went on listing the release the member last reported (1.16.2) while
  the member was already on 1.16.5;
- a watch was never acknowledged, so the window never got past "waiting";
- a takeover was never accepted, so the window said "The takeover ended" a second
  after it opened;
- no shared tasks moved.

The cause was reproduced on the released 1.16.5 by the two-Hive acceptance run
(`scripts/dogfood/two-hive-acceptance.sh`). One refused request (a 409 on the
project catalog, through a proxy standing where the field's tunnel stands)
recorded `incompatible` with `next_attempt_at: null`. The reconcile loop then
returned early for that condition on every pass, forever. Nothing retried it and
nothing said which request had been refused. The only way out was the operator
pressing "Retry Apiary synchronization", and nothing on screen told them to.

A single refusal is not evidence of a permanent incompatibility. A tunnel, a
proxy, a Keeper mid-restart or a Keeper mid-migration can each return one 4xx.
"Fail closed" was meant to stop a member acting on authority it could not
verify, and retrying does not do that. What ADR 0030 actually did was take a
member out of the Apiary on its first bad answer.

## Decision

1. **Every failure schedules a retry.** Authentication and protocol refusals wait
   out the same bounded backoff as an outage: 5, 15, 30, 60 and 120 seconds,
   then 300 seconds. No condition leaves `next_attempt_at` empty.
2. **Refusals do not skip the backoff.** A Keeper announcement (the event-socket
   doorbell) retries an outage at once. It does not shorten the wait after a
   refusal, because the doorbell does not make a refused credential more likely
   to be accepted.
3. **The failure names its step.** `local_federation_sync.failed_step` records
   which part of the pass failed, such as "membership credential", "project
   catalog" or "shared tasks". The member's Apiary page and Settings show it
   with the next attempt: "Stopped at project catalog · next try in 2m". The
   value is a fixed label from the code, never response content.
4. **The Keeper records when each member last reached it.**
   `apiary_federation_memberships.last_contact_at` is written when a member
   credential authenticates, at most every 30 seconds. The Keeper's roster shows
   "heard from 3m ago" beside the reported release. It raises any member not
   heard from in ten minutes, twice the longest backoff, so a stale version can
   no longer pass for a current one.

Fail-closed still holds where it matters: a response that fails validation is
still not applied. The failed pass changes nothing. It is only retried later.

## Consequences

- A member stops by itself only while the Keeper keeps refusing it, and it says
  so. When the refusal stops, it recovers without anyone pressing anything.
- A member whose credential the Keeper has really revoked retries every five
  minutes indefinitely: one small request per member every five minutes.
- Migrations 196 (`failed_step`) and 197 (`last_contact_at`) are additive
  nullable columns. A record from before them shows "Stopped" with no step, and
  a member shows "no contact recorded" until it next reaches a Keeper running
  this release.
- The two-Hive acceptance run exercises decisions 1 and 4 end to end. It fails
  on 1.16.5 and passes on this change.

## Addendum, 2026-09-29: a loop was doing the pacing's job

The two-Hive run now keeps both Hives' logs. Those logs showed a member and its
Keeper ringing each other about fifty times a second while a doorbell socket was
connected. Every member pass exchanges its public profile. The Keeper announced
a directory change on every exchange, and every announcement started another
member pass. Every check still passed, because each thing the member was meant
to do happened almost at once, just unintentionally.

Stopping the loop exposed four places where pacing had quietly delayed what the
loop had been doing immediately. Each is now fixed:

1. **The Keeper announces a directory change only when a profile changed.** The
   store already knew this; the application layer dropped it.
2. **A member reads back what it just accepted.** Accepting a watch or a
   takeover left its own copy `requested` until the next full pass, which is
   paced to a minute, and only an active one relays. Both steps now re-read the
   Keeper's list after accepting.
3. **Local changes sync now.** A reclaim, a handoff answer or a Steward request
   asked for a sync through the paced entry point, which returned at once, so it
   waited up to a minute. They use a prompted pass, as an announcement does.
4. **Reconnecting counts as an announcement.** The catch-up sync on reconnect
   stands in for doorbells that were missed, but it was paced, so a takeover
   requested between two connections was not seen for up to a minute.

A prompted pass still waits out the backoff after a refusal, as in decision 2.
The run now fails if the member makes more than 150 sync passes in a single
run. A healthy run makes about fifteen, and the loop made more than 750.
