# Marking a resolved decision superseded

Spec for task `01a07374-18fc`, settled with the operator on 2026-09-17.

## The problem, stated exactly

An operator resolves a decision. It later turns out the card was mis-framed —
answered on a false premise, or replaced by a better-scoped one. There is no way
to say so on the record.

The concrete case in the ticket: `01a07349` (Hub disposition) was filed
mis-framed, the operator resolved it "Hub is done, re-scope A6/A8", and live
verification then showed the premise false. A corrected card `01a07352` was filed
beside it. `01a07349` remains live authority.

## The premise verified, because three tickets this day did not survive it

Checked directly, expecting it to be stale like the others. It is not.

| claim | verdict |
| --- | --- |
| `swarm_withdraw_decision` refuses a resolved decision | **true** — `decisions.rs:531`, the UPDATE carries `WHERE id = ?1 AND state = 'pending'` |
| the record has `withdrawn_at` / `withdrawal_reason` but they are gated | **true** — added by the migration at `decisions.rs:67`, reachable only through that pending-gated path |
| there is no `superseded_by` | **true** — `grep -rn superseded_by crates/ --include=*.rs` returns nothing |

## ⚠️ The option this spec REFUSES

The ticket offers "allowing withdrawal-with-reason on a resolved decision". That
would set `state = 'withdrawn'` on a record that **is** the operator's answer.

A resolved decision read from this store is first-party operator evidence — that
is the contract the whole Hive verifies against. An agent must not be able to
make one stop reading as resolved. Someone may already have acted on it, and
their authority has to remain auditable after the fact.

So supersession is **additive**. `state` stays `resolved`. `resolution_action`
and `resolution_answers` are never touched. The record gains a pointer and a
reason; it is not rewritten.

## Settled with the operator

### 1. Authority dies when the replacement resolves

X keeps authorising until the superseding card Y is **resolved**. Then X stops
being valid authority and readers are pointed at Y.

Rejected: *warning only* rebuilds the prose workaround as a struct field — a
worker reading X would still act on a false premise. Rejected: *dies immediately
on marking* opens a window where X is dead and Y unanswered, leaving work with no
authority at all.

### 2. Requester or Queen may mark it

Exactly the existing withdrawal rule (`decisions.rs:519` — `w.id =
d.requesting_worker_id OR w.role = 'queen'`), so no new authority surface is
invented.

⚠️ **Named tradeoff:** this lets Queen declare the operator's own answer stale
without asking them. Accepted deliberately; the alternative costs the operator a
round-trip on every mis-framed card, and the mechanism exists to spare them noise.

### 3. Chains resolve to the terminal card

Z supersedes Y supersedes X. A reader of X is pointed at **Z**, not Y. Cycles are
refused.

This is the lesson from the prerequisite traversal the same day: an edge is not a
terminus. Stopping at the first hop hands the reader another superseded card as
if it were current — the identical failure one level down.

### 4. Reversible only while the replacement is pending

Unmark freely until Y resolves — X is still authorising then, so nothing has
changed hands. Once Y resolves, X is genuinely dead and un-marking would
resurrect authority the operator has already replaced. The way back after that is
a fresh card, not an edit.

### 5. Cross-task supersession is allowed

Y need not sit on X's task. A mis-framed card is often replaced by one scoped
differently — the ticket's own case re-scoped A6/A8. Requiring the same task
would force the correction into the shape that was wrong to begin with.

### 6. ⚠️ Nothing is notified, and this is a deliberate limit

Work that already cited X as its gate is **not** told when X dies. The record is
corrected; the consequences are not chased.

**Stated plainly so nobody reads more into this than it does:** the ticket's own
case is work that PROCEEDED on a false premise. This spec does not address that.
It stops the next reader being misled; it does not find the people already
misled. If that turns out to matter, it is a second piece of work and should be
filed as one rather than smuggled in here.

## Invariants to enforce and test

1. Marking supersession NEVER alters `state`, `resolution_action` or
   `resolution_answers`. If it does, an agent has edited the operator's answer.
2. The superseding decision must EXIST. A pointer to nothing is the prose
   workaround with extra steps, which is the ticket's complaint.
3. A superseded-and-resolved X refuses to authorise; a superseded-but-pending-Y X
   still authorises. These are separate tests — one passing does not imply the
   other.
4. Cycles are refused; chain reads return the terminus.
5. Unmarking after Y resolves is refused.

## Tool surface

A NEW tool rather than a flag on `swarm_withdraw_decision`. Overloading the
withdrawal path is how one error message comes to cover several causes, and this
repository has spent two separate tickets this day splitting exactly that
(`CommitsNotReported` out of `CompletionEvidenceRequired`; the three-cause
message in `supportFiles.ts`).

Changing the surface requires bumping `AGENT_TOOL_SURFACE_REVISION`, currently
**29**, so stale sessions are caught.

## Not in scope, and not assumed in

The ticket's last paragraph pairs a second complaint: an offered ACTION cannot be
amended after filing, only the summary, so a predicate baked into an option is
uncorrectable — which is what produced the mis-framing here. The ticket calls it
"worth pairing" rather than in scope. It is not addressed by this spec and needs
its own decision: allowing an action to be edited after an operator may already
have read it is a different and more dangerous change than adding a pointer.
