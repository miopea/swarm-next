<!-- Relocated from the queen workspace as part of task 01a016be. The spec was
     written outside this repository on purpose while this tree had uncommitted
     changes to the decisions domain; that reason has passed. Content is
     unchanged from the 2026-08-18 original. -->

# Interview-shaped decision requests

**Status:** specified, not built
**Author:** Queen, from an operator interview on 2026-08-18
**Implementer:** Swarm worker (`/home/bschleifer/projects/personal/swarm-next`)
**Origin:** operator instruction — "You should always /interview me when you have
questions. This should be something that Swarm builds into the system."

---

## 1. The problem, stated from evidence

A worker that needs operator judgment today has exactly one instrument:
`swarm_request_decision`, which carries a `title`, a `reason`, and up to six
`allowed_actions` buttons plus one `suggested_action`.

That instrument forces the asker to **collapse an open question into a small set
of pre-guessed answers before the operator has said anything**. When the guess is
wrong, there is no recovery inside the record — the operator can only pick a
wrong-ish button, or dismiss.

Three failures from a single evening, all real, all in the Hive record:

| Record | What happened | Root cause |
| --- | --- | --- |
| `01a01229-fdc3-7c53-9613-d87587c46066` | Operator **dismissed** without a note. Dismissal is not one of the offered actions, so the resolution carried no recoverable intent — "hold everything" and "stop asking me" are indistinguishable, and both readings had to be treated identically. | Options did not contain the operator's actual answer. |
| `01a01677-7bab-7bb0-a475-527012d2d7fd` | Operator chose "Send it back to draft until it has a description". The ruling was then **impossible to execute**: `ready -> draft` is not a legal transition, and the fallback `ready -> blocked` failed with "Jira workflow mapping is invalid". | The asker offered an action it had not verified was executable. |
| `01a01673-a282-7320-836d-51e5d1e5fd92` | Correct outcome, but the asker (BFG Operations) had to compress a nuanced cross-repo finding into three buttons, and pre-committed to one interpretation of an ambiguous number. | Nuance survived only because the `reason` field was long enough to carry it — a workaround, not a mechanism. |

The pattern: **a button set is a good instrument for a ruling that is already
understood, and a bad instrument for a question that is still open.** Swarm
currently has only the first.

## 2. What is being built

`swarm_request_decision` gains an **optional** `questions` array. When present,
the record behaves as an interview: the operator answers a series of questions
rather than pressing one button.

```
swarm_request_decision({
  task_id, kind, title, reason, risk, evidence,   // unchanged
  questions: [                                     // NEW, optional
    { header, question, options[], multiSelect },
    ...
  ],
})

resolution: { answers: { <header>: choice | text }, note }
```

**Existing single-ruling records stay valid with `questions` omitted. There is no
migration and no deprecation.** This was chosen over a separate
`swarm_request_interview` tool deliberately: the delivery path, inbox, resolution
states, wake behaviour and audit trail are already built and proven, and this
adds a field rather than a second subsystem.

### 2.1 Decided by the operator

| Question | Decision |
| --- | --- |
| Shape | Extend `swarm_request_decision` with an optional `questions` array. No new tool. |
| Blocking | **Hard block.** The worker holds its loaded session while waiting. It does not rest and does not proceed on unblocked work. |
| Who may ask | **Any worker, on its own task.** Not Queen-only, and not Queen-reviewed. |

The "who may ask" ruling is load-bearing and matches the evidence: of the five
questions in the operator's inbox that evening, three came from workers rather
than Queen (an iPhone inspection, an Android device with Health Connect, a
Google Health re-consent). Those are exactly the cases where a fixed button set
is a guess, and routing them through Queen would have inserted a translator who
had not done the investigation.

### 2.2 Deliberately NOT decided here

These need the implementer's judgment against the real schema, and each should
be stated explicitly in the delivered work rather than settled silently:

1. **Are `questions` and `allowed_actions` mutually exclusive?** Recommendation:
   yes — a record is either a ruling or an interview. Allowing both invites a
   record whose button says one thing and whose answers say another, with no
   defined precedence. If the implementer finds a real case needing both, it
   should be specified, not permitted by accident.
2. **Limits.** `AskUserQuestion` accepts at most 4 questions, each with 2–4
   options. Whether Swarm mirrors those caps or sets its own is open, but
   the caps must exist — an unbounded interview is a worse instrument than a
   button.
3. **Free text.** `AskUserQuestion` always offers "Other" with custom input, so
   an answer may be text that matches no option. The `answers` map must
   accommodate that; a schema that only accepts one of the declared options will
   silently lose the most informative answers.
4. **Partial answers.** What a record means when the operator answers two of four
   questions and stops.

## 3. Requirements

### 3.1 Core

- `questions` is optional on `swarm_request_decision`. Records omitting it behave
  exactly as they do today, byte for byte in their resolution shape.
- Each question carries `header` (short label), `question` (full text),
  `options[]`, and `multiSelect`.
- Resolution carries an `answers` map keyed by `header`, plus the existing
  free-text `note`.
- Any worker may open an interview **on its own assigned task**. `task_id` is
  required, as it already is during Queen automation.
- Interviews are subject to the same one-record-per-concrete-task rule that
  governs decisions today. An interview is not a licence to bundle a fleet
  review into one record.

### 3.2 Hard block — and the detector consequence

The operator chose hard blocking: the asking worker holds its session until
answered. This is the fastest resumption path and avoids wake latency, but it
has a direct and non-obvious consequence that **must** be handled in the same
change:

> **`stale_owned_work_attention` currently cannot tell "stuck" from "waiting on
> the operator", and under hard blocking it will misfire on every open
> interview.**

This is observed, not predicted. On 2026-08-18 the detector raised
`01a01695-0e89-7b73-8253-b4f1286b543a` against the BFG Operations worker —
"Active work is unchanged while its loaded worker is resting" — at a point where
that worker was neither stuck nor crashed. It had filed decision
`01a01673` and correctly stopped, because the answer was not its to give. The
detector was reporting a healthy wait as an anomaly.

Under hard blocking that becomes the normal case rather than an edge case.
Required:

- A task with an unresolved interview is **excluded** from
  `stale_owned_work_attention`, or surfaced under a distinct kind that reads as
  "waiting on you" rather than "unchanged".
- Resolving the interview delivers the answers to the **holding session** and
  resumes it, without requiring a fresh wake.
- A held session must be visible to the operator as held, and for how long. The
  decisions in that evening's inbox sat pending for between ~20 minutes and ~13
  hours; a pinned session with no visible reason is a worse failure than the
  original guess-the-button problem.
- The existing `deadline` field should govern what happens to a session held
  past it. Behaviour on expiry is open, but silence is not an acceptable answer.

### 3.3 Dismissal must carry recoverable meaning

Dismissal today produces an unrecoverable resolution: the record shows
`resolution_action: "dismissed"` with an empty note, and the asker cannot
distinguish "hold, I'll deal with it later" from "stop asking me about this".
Both readings had to be collapsed into "change nothing", which is correct but
lossy.

For interviews specifically, dismissal should either be disallowed, or captured
as a distinct outcome that the asker can act on differently from an answer. The
implementer picks which; the requirement is that the two readings stop being
indistinguishable.

## 4. Acceptance criteria

- A worker can file a decision request carrying `questions`, and the operator can
  answer it, with the answers reaching the asking worker intact — demonstrated
  end to end, not asserted from unit tests.
- A decision request **without** `questions` behaves identically to today.
  Demonstrate this against an existing pre-change record shape.
- A free-text answer that matches none of the declared options survives to the
  asker unmodified.
- A task with an open interview does **not** appear in
  `stale_owned_work_attention` as unchanged/stale work. Show the detector
  producing the old behaviour first, so the fixed result means something.
- A held session resumes on answer delivery without a manual wake.
- Answering an interview is visible in the record's audit trail with the same
  fidelity as a button resolution today.
- The open questions in §2.2 are each answered explicitly in the delivered work.

## 5. Boundaries

- **This spec lives outside the swarm-next repository on purpose.** At the time
  of writing, the Swarm worker had uncommitted changes in that tree —
  including a new untracked `crates/swarm-domain/src/decisions.rs` — and writing
  into a repo mid-change risks the file being swept into someone else's commit.
  The implementer should relocate this into `docs/` under the existing numeric
  convention (next free index after `28-developer-dogfood-audit.md`) as part of
  the work.
- **Sequencing matters here.** The decisions domain is being actively edited in
  that same tree. This change extends exactly that surface. Land it in a defined
  order relative to the in-flight work rather than in parallel.
- Do not change the task lifecycle as part of this. The `ready -> draft`
  restriction is real and inconvenient, but it is a separate question tracked on
  task `01a016b9-fa37-72f1-be4e-d322c86668c9`.

## 6. Provenance

Operator answers were collected in a four-question interview on 2026-08-18 —
using the very mechanism this spec asks to be built into the system, which is the
argument for it.


---

## 7. Delivered — 2026-08-20

Built across `82b5912` (record and store), `87b4bd2` (inbox surface),
`ed715fe` (delivery), `8b0044b` (held-session visibility and deadlines),
`d9dfe10` (the summary field), and `d702c9d`.

### Acceptance, measured

| Criterion (section 4) | Status |
| --- | --- |
| A worker files a request carrying `questions`, the operator answers, the answers reach the asking worker intact — end to end | **Demonstrated.** Decision `01a01ffc` filed by this worker, answered in the inbox, delivered to its terminal at 16:41:09 with the answers matching the record. |
| A request without `questions` behaves identically to today | **Demonstrated.** Every other decision resolved today used that path unchanged. |
| A free-text answer matching none of the declared options survives unmodified | **Unit-proven only.** The operator chose offered options both times. The storage and rendering are covered by tests; the delivery path is the same one proven above, so the untested variable is the string content. |
| An open interview is not reported as stale, with the old behaviour shown first | **Demonstrated.** `f224968` asserts one stale candidate before the request exists and none after. |
| A held session resumes on answer delivery without a manual wake | **Demonstrated.** The answer arrived with no wake. |
| Answering is visible in the audit trail at button fidelity | **Demonstrated.** `resolution_action: answered`, the surface, and the answers are all recorded. |
| The section 2.2 open questions are each answered explicitly | **Done.** All four, in code and in tests. |

### The section 2.2 answers

1. **Questions and actions are mutually exclusive.** A record is a ruling or an
   interview. Both would allow a record whose button says one thing and whose
   answers say another, with no defined precedence.
2. **Bounded to 4 questions, 2–4 options each**, mirroring `AskUserQuestion`.
   Headers must be unique because they key the answers.
3. **Free text survives.** An answer matching nothing offered is the case the
   asker failed to guess, which is why interviews exist.
4. **Partial answers do not resolve.** A worker holding its session waits for
   the whole set; half an answer resumes it with an incomplete picture and no
   way to ask for the rest.

Section 3.3 is answered too: dismissing an interview requires a reason, which
is the failure that opened this spec.

### What this cost to learn

Two defects surfaced only because the demonstration was attempted, and neither
was visible to any unit test:

- Requiring `summary` broke **every already-connected worker**, because a
  worker's tool schema is fixed when its MCP session connects. Tests construct
  the input struct directly and never see it.
- `parse` reported every unreadable argument as an authorisation failure, so
  the symptom was "this agent is not authorized" for a missing field.

Both fixed in `d702c9d`. The spec's insistence that unit tests are not evidence
for this criterion was correct.

### Extended beyond the spec

Any pending request — not only an interview — can now be answered in the
operator's own words, after they hit exactly the failure this spec describes on
a button-shaped request. Recorded under one reserved key with the same
`answered` action and the same delivery.
