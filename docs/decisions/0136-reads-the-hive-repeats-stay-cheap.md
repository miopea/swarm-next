# 0136 — Reads the Hive repeats stay cheap

- Status: Accepted
- Date: 2026-11-01
- Decided by: the operator's report on task 01a19964 ("Everything seems
  sluggish") and its acceptance: the fleet reads cost a few milliseconds of CPU
  and stop making other requests wait.
- Amends: how the decision projection works out discharge (introduced with the
  "authorised act never happened" read), and how ADR 0110's and ADR 0114's raises
  find their earlier raises. Neither changes what is decided.

## Context

On a Keeper with about 2,000 tasks and 1,500 decisions, swarm-api held one core
at 100% for 5 to 16 seconds at a time. Measured from loopback:

- `GET /api/v1/decisions` cost 1.08 s of CPU per call.
- `GET /api/v1/apiary/fleet-versions` cost 1.15 s.
- `GET /api/v1/apiary/fleet-health` cost 1.1 to 2.3 s.
- Other reads that wanted the store waited behind them. `GET /api/v1/apiary/members`
  took up to 2 s instead of 2 ms.

All of the time was in one place: working out each resolved decision's
discharge. The projection did it in SQL, as a correlated
`description LIKE '%' || substr(d.id, 1, 13) || '%'` over every task, twice for
each resolved decision in a 256-row inbox. That is about 1.3 GB of text compared
per read, and the store's single connection is held for all of it.

Every open page reads the inbox again on nearly every change, because the
control room snapshot includes it. The fleet raises read the whole inbox on
every member report and every Keeper page poll, and they use four fields of it.

## Decision

1. **Discharge is worked out in one pass over the live tasks, in the
   persistence layer.** A task names a decision by the first 13 characters of
   its id, `xxxxxxxx-xxxx`, and the hyphen is always in the same place. So each
   description is scanned once at its hyphens, and each 13-byte window is looked
   up among the names being asked about. The matching is the same as `LIKE`: anywhere in the text, ASCII case
   folded. Origin counting and evidence strictly after the ruling are unchanged.
   Every reader of the projection goes through one function, so no surface can
   skip the derivation.
2. **The fleet raises read their own earlier raises directly, without derived
   columns:** by exact title for "Hives are behind", and by an evidence line
   that starts with the key for a Hive fault. Pending raises come first, then
   the newest, capped at the inbox's 256.

## Consequences

- On a copy of the Keeper's data, the inbox read takes 17 ms, against 960 ms
  for the SQL derivation over the same 256 rows. All 256 answers are the same.
  The raise reads take 5 ms, and the fleet version report 0.3 ms.
- A test keeps the old SQL as a reference and checks all three readers against
  it, case by case.
- **An answered raise now holds however many decisions came after it.**
  Filtering the newest 256 decisions lost any answered raise older than that,
  so it could be asked again. No duplicate was found on the live Hive, so this
  was latent.
- The scan grows with the total size of task descriptions, not with tasks times
  decisions.
- Unchanged and worth knowing: a name is the millisecond a decision was made, so
  two decisions made in the same millisecond share one, as they did under SQL.

## Rejected

- **Caching the inbox between changes.** A cache of who-did-what has to be
  invalidated by every task, deployment and exemption write. A pass that takes
  a few milliseconds needs no cache.
- **Storing which tasks name which decision.** Every path that writes a
  description would have to maintain it. One that forgot would read wrong,
  silently.
- **Moving the store work onto `spawn_blocking`.** It would stop a slow read
  from holding an async worker. It would not stop a slow read from holding the
  store, which is what made other requests wait. Making the read cheap does
  both.

## Not proven

That this is all of what the operator felt as sluggish. It is measured on a copy
of the data and in tests, and has not yet been watched on the running Keeper.
