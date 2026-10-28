# 0128 — A terminal answer keeps its notes, and one Swarm cannot use says so

- Status: Accepted
- Date: 2026-10-27
- Decided by: the operator, in the Swarm Next worker's terminal on 2026-10-27.
  Asked about terminal answers that never reached their card, they chose
  "Answers with notes or previews" and "Say so on the card".
- Amends: ADR 0065, native interview capture.

## Context

An AskUserQuestion answer typed in a worker's terminal resolves its Needs You
card once the engine confirms it against Claude's final batch. Many answers
left the card lit and saying nothing:

1. **Answers with notes.** Claude Code lets a person type notes beside the
   option they chose, or instead of choosing one. The tool result carries them
   as `annotations`, and the engine refused every annotation. This machine's
   transcripts hold 22 answers with notes, written by Claude Code 2.1.250 to
   2.1.302 (read on 2026-10-27).
2. **Answers the engine refused.** A refused completion, or a payload it could
   not read, removed the pending request. The final batch then had nothing to
   report. Annotated answers were the common case. Others were answers with no
   recorded typing and answers with more typing after the submit.
3. **Answers cut off from their final batch.** The engine dropped an answer
   when its batch did not match it. It also dropped one when input, a new
   question, a lost session or an unreadable payload arrived before the batch.
   The card already had an "unverified" notice for this case, but nothing could
   reach it.
4. **Questions asked and never answered.** 790e64aa (2026-09-13) taught the
   engine to report these and the card to explain them. The API then refused
   that evidence at intake because it carried no answers and no typing. The
   refusal table's CHECK also did not allow `never_completed`. Either problem
   alone kept the notice off the card.

## Decision

- **Notes are captured.** The final batch is matched exactly against Claude's
  grammar: `"Q"="A" notes: N` beside an option, and `"Q"=(no option selected)
  notes: N` in place of one. The answer stays the chosen option. The notes
  become the decision's resolution note, which the card shows after the answer.
  With no option chosen, the notes are the answer. With several questions,
  each note is labelled with its question's header. A note over 4,000 bytes is
  cut and says so, because the worker already received all of it.
- **An answer the engine cannot use is refused, and its card says so.** Some
  answers cannot be read without guessing:
  - blank notes;
  - notes for a question that was not answered;
  - "(notes only)" with no notes, or where an option has that label;
  - notes over 16 KiB.

  Each is refused, like every other refused completion. The request stays in
  place, tainted, so a retry of that answer cannot complete it, even after more
  typing. The final batch for the same invocation ends the request and reports
  it as never completed. It is not reported if the terminal selection moved, or
  if the engine already holds 32 unacknowledged reports.

  Only the answer itself counts as refused. Other traffic the engine cannot
  read, such as a subagent's batch, clears the request as before and reports
  nothing. An answer given while that happens is still lost without a notice.
- **A quote that moves the boundary confirms nothing.** `"Blue" notes: x"`
  reads as Blue with the note `x"`, or as the answer `Blue" notes: x` alone.
  The engine reads the answer, but the batch cannot confirm which reading is
  right, so the card says "unverified". The existing rule refusing quotes when
  there are several questions now covers notes too.
- **Previews are not captured.** A decision's options carry a label and a
  description, and no preview. So a decision asked in a terminal cannot carry
  one, and a question whose options have previews is refused when it is asked,
  as before. Capturing previews would add code that no card can reach. This is
  the one part of the operator's choice not built.
- **An answer cut off from its final batch is passed on, not dropped.** It
  still cannot resolve anything; only an exact final batch can. The card shows
  its "unverified" notice instead.
- **A question that was never answered reaches its card.** Intake admits that
  evidence with no answers and no typing. Schema 212 rebuilds
  `decision_native_answer_refusals` with `never_completed` in its CHECK and
  keeps its rows.

## Consequences

The changes land in two steps:

- **On reload:** the API changes. These are never-answered intake, schema 212,
  and notes in the resolution.
- **After the worker engine update:** the engine changes. These are capturing
  notes, reporting refused answers, and passing on cut-off answers.

Until the engine update, the running engine still refuses every annotated
answer and the card stays silent, as before.

The reload also takes in a backlog. The running engine has been holding up to
32 never-answered reports since it started, because intake refused them and
nothing acknowledged them. A full queue would also have stopped all new
capture. Once the reloaded API admits them, each report goes on any pending
decision from the same worker with the same questions. A newer decision asked
from the same template would get an older notice.

Two notices fit their new cases imperfectly:

- The "never completed" text opens by saying the answer "never came back as a
  completed answer", which is not true of a refused one. It also advises
  choosing an offered option, which a refused answer may already have done.
- The "unverified" text offers answering in the terminal again. An answer that
  failed on a quote will fail the same way again.

Both notices still point to answering on the card, which always works.

## Validation

- **Tests.** Each of these cases has a test:
  - Notes beside an option, and notes in place of one, each match their exact
    final batch and nothing else.
  - With several questions, notes match only when the text is unambiguous.
  - A quote that moves the boundary confirms neither reading.
  - Evidence for a never-answered question is admitted.
  - Notes on an unanswered question are refused.
  - A refused answer is reported by its final batch, and stays refused after
    more typing and a retry.
  - An unreadable completion is reported through the terminal registry.
  - A subagent's batch, or an unreadable answer to another invocation,
    reports no failed answer.
  - A batch ends its request even when it cannot report it.
  - Each way an answer is cut off from its batch passes it on unconfirmed.
  - Notes reach the decision beside its answer.
  - A note too long for the card is cut on a character boundary and still
    resolves the card.
  - A never-answered question reaches its card.
  - Schema 211 migrates to 212, and the rebuild keeps recorded refusals.
- **Ablations.** Each fix was reverted in turn, and the test above it failed:
  - the intake gate;
  - the CHECK;
  - copying rows in the rebuild;
  - passing on unconfirmed answers;
  - keeping and tainting a refused request;
  - reporting it from the batch;
  - ending the request when its batch arrives;
  - the registry's unreadable-payload path;
  - telling the answer apart from other unreadable traffic;
  - matching notes;
  - the resolution note;
  - cutting a long note;
  - the quote boundary.
- **Not proven.**
  - No annotated answer has yet resolved a real card; that needs the worker
    engine update.
  - Every notes test builds its final batch by hand from the 2.1.267 fixture.
    If Claude puts annotations into the batch's tool input, a noted answer will
    be refused as "unverified" rather than resolve.
  - No never-answered notice has yet been seen on a live card.
