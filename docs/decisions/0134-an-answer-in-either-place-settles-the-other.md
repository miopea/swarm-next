# 0134 — An answer in either place settles the other

- Status: Accepted
- Date: 2026-10-31
- Decided by: the operator, answering two questions on task 01a19279 (recorded
  on the task). Their report: "I answer in the worker and the needs you page
  still shows and vice versa. There is no fast/clean cleanup in both
  directions. if this happens the needs you doesn't help."
- Amends: ADR 0065 (exact correlation of operator statements) and ADR 0128
  (terminal answers resolve the card), on what "the same question" means.
  Amends the rule in `coordination_delivery::activity_deferral` that nothing is
  ever written while a provider waits on the operator.

## Context

A worker that asks the operator a question raises it in two places: as a card
in Needs you, and as Claude's `AskUserQuestion` menu on the worker's screen.
The operator can answer either one. The other is supposed to clear itself. It
often did not.

**Answered on the worker's screen, the card stayed.** Over 30 days, 258
terminal answers were captured. 10 settled their card. 3 matched a pending
card's question and still did not link. In all 3, the card's option
descriptions were blank or worded differently from the screen's. One card
asked four questions and stayed in Needs you for 42 minutes after the operator
had answered them all on the worker's screen. The binding compared the whole
question, descriptions included, so any difference in a description refused
the answer.

**Answered in Needs you, the worker's question stayed.** Over 14 days, 180 of
577 inbox answers took more than a minute to reach the worker. Delivery is held
while a worker's screen shows a question, because text typed into an open
question becomes its answer. When the open question is the very one the
operator just answered, holding protects nothing. In one case the answer was
held 581 times over four hours behind the question it answered, until the
worker's screen changed for some other reason.

## Decision

1. **A terminal answer settles the card when the wording, header, selection
   mode and option labels match exactly. Option descriptions may differ.**
   - The operator's ruling: "Yes, wording+options match".
   - `NativeInterviewQuestion::answers_decision` and
     `native_interview_answers_decision` are the one rule. Finding the card,
     binding the capture, and resolving all use it.
   - The statements recorded for the card name the card's own question, not
     the screen's. So resolution compares the card with itself, and the
     descriptions the screen showed are not written anywhere as if the card
     had asked them.
   - ADR 0065 called descriptions approval-relevant, and schema 161 fences
     older readers so they cannot ignore them. That still holds for the card:
     Needs you shows and compares them. For a terminal answer the operator read
     the terminal's descriptions, and the captured interview keeps exactly
     those, so the record of what they saw is not lost. The operator ruled
     that the labels are the choice.
   - Needs you's own staleness check (`displayed_decision_questions_match`)
     stays strict. A card whose descriptions changed under the operator's eyes
     still clears their draft, as ADR 0065 decided.
2. **An answer given in Needs you closes the same question on a Claude
   worker's screen, then is delivered.**
   - The operator's ruling: "Yes, close it and deliver", only when the
     question on screen matches the card's wording. "Any other question is
     left alone."
   - When a decision's answer is held because the worker is waiting on the
     operator, Swarm reads the screen once more. It presses Esc only if all of
     these hold:
     - the worker runs Claude Code;
     - the same read shows it waiting on the operator;
     - a live menu is open (its footer is the bottom line);
     - the menu's question, rejoined across wrapped rows, has the same words as
       one of the card's questions.
   - Esc is pressed **at most once per decision**. A second press on an empty
     prompt opens Claude's rewind menu, so it is never repeated on a timer.
     `AnsweredQuestions`, owned by `AppState` and capped at 256, records the
     presses. A press the terminal host refused does not count. A press whose
     result is unknown does.
   - Swarm then watches up to five seconds for the question to leave the
     screen. If it does, the answer is delivered in the same pass, through the
     ordinary delivery path and all its checks. If it does not, the answer
     waits for the next pass, as it did before.
   - The record is not persisted. After a restart, a question that is still
     open and still matches may be pressed once more. That is the same
     decision made again on fresh evidence.

## Why the screen, and why that is acceptable here

The capability inventory says provider-specific terminal scraping should be
replaced with hooks where possible. The terminal host already observes
`AskUserQuestion` through hooks, including the moment it is asked. But that
observation lives in the host, and handing it to the API needs a terminal-host
protocol change, which a Hive can only install by stopping every worker.

The screen read here is narrow: it can only stop an action. Every doubt reads
as "leave it alone", and a "leave it alone" costs exactly what this decision
replaces: an answer that waits. The doubts include:

- a menu that is not live;
- a question scrolled off the screen, or taller than it;
- a bordered block above the menu's top border;
- a hard-wrapped word that rejoins with a space.

Exposing the host's hook observation to the API would remove the read. That
belongs with the next protocol change.

## Consequences

- A worker whose question was closed sees it cancelled, exactly as if the
  operator had pressed Esc, then the "[Swarm decision … resolved]" message
  with the answers. That message
  already states the answers, so the worker has what it was waiting for.
- A card can now settle from a terminal answer whose screen showed different
  option descriptions. The operator ruled that descriptions are explanation,
  not the choice. The labels, which are the choice, must still match exactly.
- Codex and the alpha providers are unchanged. Their menus are not read, and
  an answer to them still waits for the worker's screen to change.
- A multi-question card matches when the screen shows any one of its
  questions. That is the same set of questions, shown one tab at a time.

## Validation

- `a_terminal_answer_settles_a_card_whose_option_descriptions_differ`
  (application): blank and different card descriptions both resolve, with the
  answer recorded. Changed wording, labels, or selection mode stay pending. It
  failed before the change with `NoLongerApplicable`.
- `exact_binding_is_idempotent_and_does_not_resolve_or_deliver`
  (persistence): its refusal case changed from cleared descriptions to changed
  wording, because cleared descriptions now bind by this decision.
- `an_open_menu_is_recognised_by_its_question_at_any_width` and
  `a_question_that_is_not_the_live_menus_own_is_never_matched` (terminal):
  - wrapped at 80, 30 and 12 columns, with Claude's cursor moves between words;
  - reworded and truncated questions refused;
  - an answered menu refused, and a menu with no footer refused;
  - a different live question refused;
  - the question only in the transcript refused.
- `the_same_question_on_screen_is_closed_once_and_then_left_alone`,
  `a_different_question_or_provider_is_never_pressed` and
  `a_refused_press_does_not_spend_the_only_one` (API).
- `an_answer_given_in_needs_you_closes_the_same_question_and_arrives` runs the
  real decision delivery pass against a simulated Claude screen. Esc is
  pressed once, then the answer is typed and submitted, and nothing is left
  queued.
- Each new test was broken on purpose to check it catches the failure:
  - Removing the top-border stop fails the transcript case.
  - Removing the delivery wiring fails the end-to-end test.
- Not yet observed on a live worker. The end-to-end test simulates Claude's
  screen; the first real closure will log "closed it on the worker's screen
  to deliver the answer" with the decision id.
