# A question that cannot be unasked

A design for amending, withdrawing, and reading a decision card, from task
`01a06b6a-e56b-76e2-8186-9d97e01920d9` and the seven amendments on it from Queen,
Platform and Public Website.

**Nothing here is shipped.** The ticket's scope says so in as many words: *"DO NOT
SHIP A DECISION-MUTATION PATH ON THIS TICKET — it changes what the operator sees
on a record they may be reading, and that wants its own review."* This is the
review it wants.

## What was established before designing

Four facts, read from the code and the database rather than argued.

**1. The mode is purely the author's choice, and the two are exclusive.**
`validate_new_request` (`crates/swarm-persistence/src/decisions.rs:933`) refuses a
request carrying both `questions` and `allowed_actions`, with the reason stated:

> A record is either a ruling or an interview, never both. Permitting both invites
> a record whose button says one thing and whose answers say another, with no
> defined precedence between them.

So the `in_their_own_words` escape hatch Queen found in active use is reachable at
will. Nothing gates it. Choosing buttons is a choice, not a default the surface
imposes — which matters, because the action-only rule below is then enforceable by
the author who already has the alternative in hand.

**2. There is no close, cancel, withdraw or edit path. At all.** Every public
function on the decision store is `workers_awaiting_operator`,
`forget_moot_unconfirmed_answers`, `workers_holding_for_an_answer`,
`create_decision_request`, `get_decision_request`, `count_decision_requests`,
`list_decision_requests`, `answer_decision_request`, `resolve_decision_request`,
`live_command_grants`, `consume_command_grants`, and four delivery functions. A
pending decision leaves `pending` only when the operator answers it.

**3. ⚠️ A withdrawn card cannot be expressed in the schema, and the obvious
workaround recreates this ticket's own defect.**

```sql
state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','resolved')),
CHECK ((state = 'pending' AND resolution_action IS NULL AND resolved_at IS NULL)
    OR (state = 'resolved' AND resolution_action IS NOT NULL AND resolved_at IS NOT NULL))
```

There are two states, and `resolved` **requires** a `resolution_action`. So
"withdraw by resolving it with the action `withdrawn`" is the cheap implementation
— and it writes a string into the one column whose entire contract is *that it is
the operator's own answer*. `swarm_list_decisions` tells every reader so. A worker
verifying at source, doing exactly the right thing, would read `withdrawn` as the
operator having chosen it.

That is the laundering failure this ticket exists to stop, reintroduced by its own
fix. **Withdrawal needs a third state, not a reserved action string.**

**4. `verified` carries three meanings and only one of them populates `state`.**
`verify_decision` (`crates/swarm-api/src/agent.rs:1443`) returns `verified: false`
for a malformed id, for a nonexistent id, and for a live pending card. The first
two return no other fields; the third returns `state`, `summary` and `task_id`.

    THE CHECK IS   state == "pending" means a live card exists.
                   verified == false alone means nothing.

## The shape, and why it is one problem

    A CARD IS FROZEN AT CREATION, AND EVERY FIELD ON IT IS READ LATER AS FACT.

Five reported symptoms collapse into that sentence:

| symptom | what is frozen |
| --- | --- |
| a stale summary | the description |
| a false premise in a button | the label, which becomes `resolution_action` |
| two live cards for one question | the existence of the card |
| `verified: false` on a live card | a boolean standing for three states |
| a replacement that cannot mark its predecessor | the superseded card predates it |

Queen's own correction is the load-bearing one and it is why labelling cannot be
the fix. On 2026-09-03 **both answers were correct.** `01a06831` was answered
first and correctly; `01a0677e` was also answered correctly — its summary admits
the swap *"releases five other staging changes I have not identified"*, and *leave
it* is the right answer to a question with a stated unknown.

> If either answer had been mistaken, better labelling could plausibly have fixed
> it. Both were correct. So there is no wording of either card that improves on a
> right answer.

Marking shifts which question gets answered. It cannot make a correct answer to a
superseded question harmless, because the harm is that the answer is **right and
no longer applicable.**

## The proposal

Four changes. Two are cheap and independent; two want the operator's eye.

### A. Amendment — append-only, attributed, rendered as an amendment

`swarm_amend_decision_facts`, shaped like `swarm_amend_task_facts`: original text
never erased, author and timestamp recorded, rendered in the inbox **beside** the
card rather than folded into the summary.

Rendering it as an amendment rather than merging it is not cosmetic. Platform
named a failure no feature fixes: *the "would they answer differently" test is
always applied by the party who would rather the answer were no.* An amendment
that is visible as an amendment leaves that judgement auditable. One folded
silently into the summary does not.

### B. ⚠️ The answered-in-flight case, which the ticket demands be resolved first

This is the part that decides whether A is safe, so it is specified rather than
sketched.

**An amendment on a resolved decision is refused, not applied.** The write is
conditional in the same statement that performs it:

```sql
UPDATE decision_requests SET ... WHERE id = ?1 AND state = 'pending'
```

Zero rows updated means the operator answered between the amender's read and their
write. That returns a distinct error — *"this decision was answered at
`<resolved_at>`; your amendment was not applied"* — carrying the resolution, so
the amender learns the answer in the refusal.

**Why refusal rather than a late amendment:** the ticket says an amendment landing
after an answer, silently, is worse than the stale description, because the
operator would have answered a question that no longer exists and nothing would
say so. Refusing makes that impossible; the amender is then holding new
information and a resolved decision, which is a supersession case, not an
amendment case.

**And the operator may be reading the card right now.** An amendment cannot be
made invisible to an open card, so it must be *legible on arrival*: the amendment
block carries its own timestamp, and a card the operator opened before it landed
shows it as an addition rather than as text that was always there. Silently
rewriting a summary under a reader is the one behaviour this design forbids
outright.

### C. Withdrawal — a third state, and the replacement path stays

Add `withdrawn` to the state CHECK, with `resolution_action` NULL — which the
existing constraint already forbids for `resolved`, and which is exactly why it
must be its own state. A withdrawn card is not answerable and reads as withdrawn
to `verify_decision`, `swarm_list_decisions`, and the inbox.

**Filing a replacement must withdraw its predecessor in the same transaction.**
Public Website's finding is that no prose on card B prevents card A being clicked,
and Queen proved it: on 2026-09-03 the marking was done thoroughly on the card that
could carry it, and the unmarked predecessor stayed answerable and was answered.
You can announce supersession on the card doing the superseding; you can write
nothing onto the card being superseded, because it predates its own replacement.

**The replacement path is NOT retired, and here is the reason** — the ticket asks
for this explicitly. Withdrawal and amendment answer different questions:

    WITHDRAWAL    removes a question that should no longer be asked
    AMENDABILITY  stops a question going stale in the first place

`01a0677e` needed amendability: its premise was honestly provisional and got
resolved. The C22 card needed withdrawal: it carried a claim that should never
have been there. Neither substitutes for the other, and a replacement that
withdraws its predecessor is the correct move whenever the *question* changes
rather than the facts around it.

### D. Two cheap fixes that are independent of all of the above

**Action-only labels.** A button label must carry an action and no claim.

    "Publish 0.19.0"                                       action
    "Publish 0.19.0 — same content, same reach as 0.18.0"  action + an unadjudicated claim

The second was clicked, and `resolution_action` now records the operator asserting
something measurably false: `git log bc5fd8d..fee37845 -- packages/ui` returns one
commit, so 0.19.0 **is** 0.18.0 plus `d6b98c0`. Nobody lied; a fact does not fit in
a button. Where the question genuinely turns on a contested claim, the bound is
already available — ask it as `questions`, and the record carries the operator's
prose instead of the author's.

This is enforceable at `validate_new_request`, and it should be advisory first: a
lint on em-dashes and clause separators in labels is a heuristic, and a hard
refusal on a heuristic blocks legitimate labels. **Its false negative, stated:** a
claim phrased without punctuation ("Publish 0.19.0 unchanged") passes. The rule is
worth having anyway because it moves the common case, not because it is complete.

**Split `verified`.** Return `exists` and `resolved` as separate fields, keeping
`verified` as their conjunction for existing readers. The three states then read
directly instead of by inspecting which siblings are populated.

## What this design does not fix

**None of it prevents a card being answered correctly and becoming inapplicable a
moment later.** Withdrawal narrows the window; it does not close it, because the
operator can answer between the amender's read and the withdrawal write — which is
the same race as B, with the same resolution: the withdrawal is refused and the
answer is returned.

**And the honesty of the "would they answer differently" test remains a judgement
by an interested party.** A cheap amendment path reduces the temptation. It does
not remove it, and no schema change will.
