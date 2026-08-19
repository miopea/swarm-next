# Worker context surfaces

Status: **Specified and delivered 2026-08-19**

Decided with the primary operator while dogfooding a 32-worker Hive. Nothing
here is a legacy port; each item below traces to something observed during real
use on the night of 2026-08-18.

## Problem

Once the operator opens a worker terminal, the product stops telling them
anything about that worker. The desktop terminal toolbar spends a wide bar on a
connection chip, a collapsed session popover, and one destructive button, while
the facts an operator actually asks for — what is this worker working on, is it
stuck, which device is driving it — live only in the rail or nowhere.

The evidence is the operator's own questions during the session: *what is my
status*, *am I away*, and a worker rail that could not be searched at 32
workers.

## Purpose of the desktop bar

The bar answers **who am I talking to and what are they doing**. It is worker
and task context, not terminal mechanics.

Terminal mechanics stay out. Scrollback, jump-to-latest, paste, and zoom are
either already solved or already keyboard-reachable on desktop, and adding
buttons for them would spend the space on controls that go unpressed. This is
the `docs/25` rule against building a framework for behaviours that are not
proven to be shared.

## Ownership

The bar renders **once for the selected worker**, in the workspace header — not
inside `TerminalView`.

All running terminals stay mounted in a stable deck. A per-session bar would
render, subscribe, and hold task and decision state thirty-two times to show
one worker. Rendering once also matches the phone, where the connection dot was
already folded into the workspace header, so both surfaces share one model.

Facts that genuinely belong to one terminal — connection state, geometry,
attachment progress — remain owned by `TerminalView` and are reported upward.
`TerminalView` already reports connection state through a ref so a changing
callback identity cannot detach a live terminal.

## Contents

Shipping, in priority order:

1. **In-progress task and queue badge.** The worker's Active task with its
   state, read-only, opening the existing task detail. A count badge for the
   worker's other open work opens the task panel already filtered to that
   worker, reusing the board's existing worker filter rather than adding a
   mechanism. One-active-task-per-worker is an invariant, so there is always
   exactly one right answer for the primary slot.
2. **Engagement and geometry owner.** Which device currently owns input and PTY
   width, with an explicit takeover. Terminal geometry follows the engaged
   device; the operator hit exactly this case when a sleeping desktop left a
   phone holding presence, and today that condition is invisible and reads as a
   rendering fault.
3. **Repository, branch, and working-tree state.** A fleet of coding workers each
   owns a repository, and which branch a worker sits on is invisible until
   asked. This must come from a cheap cached read, never a poll per worker.
4. **Unconfirmed delivery.** When Swarm holds a delivery for this worker it
   could not confirm, it says so on the worker it concerns.

Deliberately excluded:

- **Attention state** stays in the rail. The operator selects a worker from the
  rail, so the state is read immediately before entering the terminal.
- **Model.** Whether Queen may only recommend a cheaper model or may apply an
  in-provider switch is an unresolved product question in `docs/28`. Putting a
  model in a toolbar invites acting on it and would answer that question by
  shipping instead of by deciding.
- **Provider process memory.** Runtime evidence keeps one owner in Diagnostics.
- **Put worker to sleep.** See below.

## Destructive actions leave the bar

Sleeping a worker moves out of the terminal bar entirely and lives only in the
worker list action menu, which already carries start and stop with right-click
parity. A bar the operator clicks for context should not also carry the control
that unloads the worker.

This also reverses the phone fold shipped in `d2c2eb2`, which placed a sleep
icon beside Lock. The connection dot stays in the phone header; the sleep icon
is removed.

## Role shape

One skeleton, with role-filled slots. Queen's autonomy and automation chips
occupy the slot where an ordinary worker shows its repository state, and Queen
shows `Always active` where sleeping would otherwise appear. The rail already
treats Queen as a profile with different content rather than a different kind
of thing, and the bar follows it.

## Sleeping and unselected workers

Context persists; terminal controls drop.

A sleeping worker still owns tasks, and that ownership is exactly what the
operator needs before deciding to wake it. Name, owned work, and repository
remain; connection and engagement disappear because they only exist for a live
PTY. The action in that state is Wake.

## Overflow

The worker rail is resizable, so the header width varies. Priority is fixed:

1. Task **state** and worker identity survive longest — short and bounded.
2. The task **title** truncates first; a truncated title still identifies the
   task.
3. Repository and engagement collapse into the existing details popover.

The bar never wraps to a second line and never scrolls horizontally. Wrapping
reintroduces the vertical chrome that was just removed from the phone layout,
and offscreen controls read as missing.

## Worker list: staleness in the state badge

Last output age appears **in the worker list**, so the operator does not have to
open a worker to find out it is stuck. It extends the existing state badge
rather than adding an element: `RESTING · 4m`.

The terminal host owns every PTY and sees the last byte, so it is the one owner
of that fact and reports it on the control-room feed, which already invalidates
typed snapshots. It must not be a periodic poll across thirty-two workers:
correctness cannot depend on repeated timers, and a client-side derivation would
only know about terminals the browser has opened this session.

## Diagnostics entry point

A control in the Control room lockup opens the **existing** Diagnostics, scoped
to the selected worker inside a terminal and to the whole Hive otherwise. This
adds an entry point, not a second surface rendering runtime evidence.

## Rejected during the interview

**A new attention rule for unsubmitted prompt text.** Raising worker attention
when text sits unsent at a prompt was proposed and rejected by the operator as
engineering around a defect. The fix belongs at the cause, not in a detector for
its failure.

The cause has been recorded wrongly here twice, and is corrected again from
evidence rather than from further reading of the code.

It was first recorded as server-side injection not honouring the submission
contract the browser implements. That was wrong: `submit_terminal_message`
already strips the trailing carriage return, waits for a render, and sends Enter
separately with bounded retries. It was then recorded as the collapsed
`[Pasted text #N]` comparison failing. That is also wrong for the observed case.

What the evidence establishes, from the durable terminal history and the
delivery tables on 2026-08-19:

- Two different failures were being treated as one. The Queen automation review
  that sits on the operator's card was never written to a terminal at all: it
  deferred every thirty seconds for four minutes because the prompt was
  occupied, exactly as designed, and was then marked uncertain when an App/API
  release restarted mid-flight. Its run id appears nowhere in Queen's history,
  while five earlier run ids do.
- The unsubmitted briefing the operator saw is a worker outcome, not that
  review. It was written at 22:04:22 and marked uncertain at 22:04:32 — the ten
  second marker deadline exactly — with no Enter ever sent. Its text is present
  in Queen's history, so the write landed and only the confirmation failed.

Three candidate mechanisms have been eliminated rather than assumed:

- The paste chip was not involved. No `[Pasted text #N]` placeholder appears
  anywhere in that session's history.
- The marker did not fall outside a size bound. The message is 3,575 bytes
  against a two megabyte snapshot.
- The marker did not scroll out of view. The snapshot carries scrollback, not
  only the visible screen.
- Output was not preventing the stability window. A resting Queen terminal was
  measured completely silent across six seconds, so the 750ms stable render it
  waits for is reachable.

What remains unexplained is why a marker present in the snapshot was not
observed within ten seconds. The next step is a reproduction with the delivery
path logging what it actually saw, not another reading of the code.

It would also have required amending an accepted invariant — that `Awaiting you`
is driven by an explicit durable decision and never by terminal-text guessing —
which is too high a price for a symptom that disappears when the cause is fixed.

Swarm's own delivery confirmation state is a different matter and is kept: it is
a fact Swarm owns rather than a reading of terminal content, and a crash or
restart mid-delivery reproduces it whether or not the submission bug exists.

## Sequence

1. Desktop context bar, worker-list staleness badge, diagnostics entry point.
2. Server-side submission contract, and the deterministic rule for an uncertain
   run whose target worker session has ended.

The operator chose this order knowing the submission defect is live. It is
recorded here so the second item is not lost.

## Open: what a phone should show

The desktop bar is deliberately absent from the phone, except for one part.

The operator's standing complaint about the phone is vertical space, and a row
of context chips would return the row the phone layout reclaimed. So the phone
keeps only the fact it cannot work out for itself: that another device is
driving this worker, and therefore owns the terminal width being rendered.
Without that, a phone showing desktop-shaped output reads as a fault.

What the phone still cannot see is which task the selected worker is carrying.
It is reachable through the worker switcher, which lists work per worker, but
not from the terminal the operator is looking at. Whether that is worth a row,
worth folding into the switcher trigger that already has two lines, or worth
leaving alone is a judgment about a surface the operator has strong views on,
and it is recorded here rather than guessed at.

**Resolved: folded into the trigger, without adding a line.** On the worker
surface the phone hides the header's own name and Hive line and lets the
trigger stand in for both, so the trigger already carried a small line holding
the Hive indicator. The task takes that line. Adding a third would have grown
the header, which is the vertical space the phone layout was built to reclaim,
and a row of chips was ruled out for the same reason.

The trade is the Hive line on this one surface. It is on every other surface's
header, and what the worker is carrying was not visible anywhere on the phone,
so the line is worth more to the task than to the Hive. Reversing it is one
conditional if the operator disagrees.

## Delivered

Everything above is implemented and deployed except where noted.

- In-progress task and queue badge, opening the board filtered to that worker
  and ordered by state.
- Last output age in the roster state badge, sourced from the terminal host.
- Diagnostics entry point in the control room lockup.
- Unconfirmed delivery marked on the worker it concerns.
- Engaged device named when it is not the device looking.
- Repository, branch, and changed-path count for the selected worker.
- Runtime updates and build progress reported in the control room, which was
  not in the original interview and was added from the same dogfood session.

Two parts are deliberately not built, and both are decisions rather than work:

- **Takeover.** Engagement is claimed by sending input. A button that claimed it
  without input would be a new input-authority path, which belongs in an ADR
  rather than in a toolbar.
- **The deterministic resume for a legacy uncertain run.** The rule keys on the
  session a delivery was written to, and rows created before that column exists
  cannot prove their terminal has ended. They keep waiting for an explicit
  operator retry rather than being guessed at.

Verified against the live 32-worker Hive rather than only under test: the
repository read was exercised across every configured workspace, including the
two whose workspace is not a Git checkout at all, where it correctly reports
nothing instead of failing the view that asked.
