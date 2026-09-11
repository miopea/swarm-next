# Resolving a Needs You item from an answer typed in the terminal

Spec for task `01a091a0-a05b`, settled with the operator on 2026-09-11.
Implements the resolution half of [ADR 0065](../decisions/0065-first-party-operator-statements.md).

## The problem, stated exactly

You answer a question in a worker's native terminal. Needs You stays lit. You
answered; the alarm did not care.

## Why — the pipeline is five steps and only two are connected

| | step | state |
| --- | --- | --- |
| 1 | capture, `swarm-terminal/src/native_interview_capture.rs` | **wired**; sets `final_result = ExactBatch` only when `matches_final_batch` holds and the selection revision still matches |
| 2 | retain, `retain_native_sources` | **wired** from `swarm-api/src/native_operator_sources.rs`; stores the whole serialized payload, so the flag survives |
| 3 | bind, `bind_native_interview` | implemented, **zero callers** |
| 4 | record, `record_operator_statement` | implemented, **zero callers** |
| 5 | resolve, `resolve_operator_statement_interview` | implemented, **zero callers** |

⚠️ **This is orchestration, not a feature.** Every safety property the ticket
asks to be proven is *already enforced* inside steps 3 and 5:

- same worker **and** same session as the evidence
- the decision must be `pending`
- the worker session must still be **active**
- the decision's questions, converted losslessly through
  `NativeInterviewQuestion::from_decision`, must equal the captured snapshot
  **exactly**
- re-binding the same decision returns `Ok(false)`; a different one is `Conflict`

Do not reimplement these. Call them.

## Decisions

### 1. Trigger: automatic, on the retain pass

Answering in the terminal must simply work; that is the whole ticket. The blast
radius is acceptable *only because* steps 3 and 5 already refuse anything that
is not an exact, same-session, pending match.

### 2. The application authenticates the source

ADR 0065 assigns this to the application, and it is the one gate not already
written. **Refuse any source whose `final_result` is not `Some(ExactBatch)`.**

⚠️ Absence means *unchecked*. It must never read as implicit success — the field's
own documentation says so, and older stored sources have it absent.

### 3. A refusal must be visible in Needs You

If the bridge refuses — snapshot drifted, session ended, not `ExactBatch` — the
item stays open **and says an answer was seen and could not be used, and why**.

⚠️ This is the difference between fixing the bug and appearing to. A silent
refusal is indistinguishable from today's behaviour: you answer, nothing
happens, nothing explains it.

### 4. The active-session rule stays strict

An answer whose session ended before the bridge ran is permanently unusable.
Accepted deliberately: ADR 0065 ties an answer's meaning to the live session, and
a reconnected session is not the one that asked. Losing a rare answer beats
resolving a decision with evidence whose context is gone. Decision 3 is what
keeps that loss visible rather than silent.

### 5. Partial answers are held, not discarded

Answer two of three questions and the two are kept; the set resolves when the
last one lands. Statements are already stored per question, so holding is the
natural shape, and interrupted answering is how people actually behave.

The snapshot must still match when the final answer arrives. If the decision
changed underneath, the whole set is discarded through decision 3's notice.

### 6. Statement text is readable by workers, like composer submissions

Operator choice, against this author's recommendation of option-only. Consistent
with the existing `swarm_operator_submissions` surface and gives workers the
reasoning.

⚠️ Worth stating plainly so nobody is surprised: a terminal is a casual surface.
Whatever is typed alongside an answer becomes readable by any agent that can call
that tool.

### 7. Kill switch: a Settings toggle, operator credential required

⚠️ **`authorize()` short-circuits on a loopback `Host` header with no credential**
(`auth.rs:204`), and workers run on the same host — so an ordinary Settings
endpoint is agent-flippable. This one endpoint therefore uses
`authorize_operator_credential` instead. A worker must not be able to turn
auto-resolution of the operator's own decisions on or off.

### 8. Default ON

**Operator override, recorded as such.** The ticket and the Codex handoff both
say to keep capture OFF until the linkage is proven; the operator chose ON after
being offered ship-dark twice. The constraint came from a worker handoff, the
override from the operator, and the operator outranks it.

## Done when

- An answer typed in a terminal resolves **exactly one** Needs You item on full
  ID plus exact question/option match.
- The negatives are proven and each fails for its own reason: uncertain
  (`final_result` absent), replayed, and wrong-session answers resolve nothing.
- A replay is a no-op. `OperatorStatementId::new()` is random, so bind, record
  and resolve run in **one transaction keyed on the source id** rather than three
  separately-idempotent calls that are not jointly idempotent.
- A refused answer is visible in Needs You with its reason.

## Not in scope

Changing capture, the audit's content-free rule, or any of the checks in steps 3
and 5. If a check here seems to need loosening, that is a signal the bridge is
wrong, not the check.
