# ADR 0056: Telling Queen what her job is

## Status

Accepted, 2026-08-25. Asked for by the operator after they had to prod Queen by
hand: "I had to prod the queen so she knew she could open, or wake up, workers
and put them back to sleep because there were 10 tasks open. How do we have her
responsibilities defined so she actually understands what she can and can't do?"

The standard they set for the whole thing: "ultimately the goal of the app is to
allow me to issue work and have things run autonomously for hours and hours at a
time", and "if she can't run on her own supervised, then there's no hope she'll
keep running overnight when I'm sleeping."

## What was actually wrong

Two things, and only one of them was the one being reported.

**A tool list is not a job description.** Queen's entire stated role was one
sentence in the MCP server instructions: durable Hive task coordination, worker
authority limited to its assignment, Queen coordinates the roster and queue.
Everything else she had to infer from twenty-eight tool descriptions.

That fails hardest on capabilities that have no tool. There is no wake tool, no
start, no stop. Waking a worker is a **side effect of assigning it Ready work**,
documented inside `swarm_assign_task` — a description she would only read once
she had already decided to assign. Looking at ten tasks parked on sleeping
workers and asking "can I wake anyone?", nothing told her yes. She writes a brief
with acceptance criteria for every task she hands out, and had one sentence for
herself.

**The trigger was never the problem.** Worth recording because the first
diagnosis was wrong: `observe_queen_automation` runs on the thirty-second
supervisor tick and re-queues on an unchanged board every fifteen minutes while
actionable work remains. The polling fallback the operator asked to be built
already existed. Queen was being woken; she looked at parked work and did nothing
about it, because she did not know it was hers to move.

## Decision

**The brief states the job, not the API**, and differs by role. For Queen: what
she owns, what is explicitly not hers, capabilities that exist only as side
effects, when she is woken, and where she will be refused. For a worker: its
authority is its assignment, it cannot complete its own work, and Queen's relay
is not the operator's approval — verify it (ADR 0054).

**"Where you will be refused" is part of the brief.** Under the autonomy policy a
refusal was Queen's only way to find a boundary. Anything reaching outside this
Hive is refused during every unattended run at every level — `explicit_external_approval`
is passed as `false` from the automation gate, so no presence setting lifts it.
Telling her up front turns a wall she hits into a plan she makes.

**Sitting still is named as a decision.** The brief says she is woken on change
and again after fifteen minutes, and that nothing else will prompt her. A Hive
that stops is therefore hers, and a run that ends with work parked should say why
it is parked.

## The trapdoor found while verifying the above

The fallback had an exit that could close permanently.

A run becomes `uncertain` when Swarm cannot confirm it reached Queen — which
`recover_inflight_queen_automation` does to every in-flight run whenever the API
restarts, and app reloads are routine. Uncertain has exactly two exits, both
exact by design: the delivery session ended, or the run marker is still on
Queen's visible screen. Neither is guaranteed. A reload does not end her terminal
— the terminal host is a separate service, which is what makes reloads safe
(ADR 0055) — and the marker scrolls out of the window in minutes.

And `observe_queen_automation` will not queue while a run is uncertain. So one
unsettleable run stopped **every** automatic review from then on. The board kept
filling, the control room showed a state, nothing raised an alarm, and the only
exit was an operator pressing a button — the exact dependency the automation
exists to remove, and the one thing absent overnight.

**An uncertain run that nothing settles within thirty minutes is abandoned**, and
the next observation queues a fresh one. Abandoning rather than replaying is what
makes it safe: uncertain exists to stop a review being DOUBLED, and that risk
lives in reusing the run id. Dropping the run and issuing a new one with a new id
cannot double anything. A Queen still working the old one can still close it —
`finish_queen_automation_run` accepts `uncertain` — and at worst reads one
duplicated review request, which costs a turn. Sitting still until morning costs
the night.

Age is measured from `delivered_at`, falling back to `requested_at`. Not
`updated_at`: that column is written with the database's clock while everything
deciding here takes an injected `now`, so comparing against it compares two
clocks and never fires under test.

## The part that is not solved

Instructions are fixed when a session connects (ADR 0053), so a running Queen
keeps the old brief until she reconnects — measured at 419 minutes for one
session on 2026-08-25. Anything she needs *during* a run has to be in the per-run
prompt as well, which is why the wake capability was added in both places. That
duplication is the cost of the connect-time contract, and it will keep being
paid until sessions can be told their tools changed.

Nothing sleeps or stops a worker on purpose. The operator asked about putting
workers back to sleep; no such capability exists, and whether it should is left
open rather than guessed at.

## Where the behaviour lives

### Wake contract reconciliation (2026-09-05)

The historical no-start/no-stop account above is superseded by the existing
`swarm_start_worker` and `swarm_sleep_worker` tools. Queen's standing and per-run
briefs now share lifecycle guidance: explicitly start a stopped worker when its
existing work needs to continue, preserving Active/Blocked/Review task state.
Ready assignment may still queue its guarded wake; repeating assignment or
rewinding task state is not the general recovery operation. The explicit Queen
start adapter enforces provider eligibility before capacity checks, so experimental
providers cannot bypass Night Watch through this tool. Ending Night Watch removes
that policy hold, not the separate capacity requirement. No task or provider is
changed by a refused start, and explicit operator startup is unchanged.
Provider policy is revalidated under lifecycle ownership before the actual start,
so a pending start cannot use a provider/presence observation from before another
lifecycle operation completed.

### Daily-driver judgment policy (2026-09-04)

The approved maturity plan qualifies the original review-only wording: supported
machine-verifiable completion does not need another Queen approval. Queen asks
the assigned worker first for unresolved judgment. An optional second opinion
may use only an available managed Scout, not arbitrary peers; a resting prompt
alone does not establish availability. Queen owns cross-repository dependent
task routing and includes worker context with her recommendation on escalation.

This guidance is shared by the standing brief and each automation message so
cached MCP instructions do not omit it. This is a prompt contract, not a new
authorization boundary or proof that model choices obey it. Existing task,
resource and delivery guards remain authoritative; runtime enforcement and
observed review-yield acceptance must be checked separately.

`standing_brief` in `crates/swarm-api/src/agent.rs`, pinned by
`queen_is_briefed_on_what_she_owns_and_on_capabilities_that_are_not_tools` and
`a_worker_is_briefed_on_its_limits_rather_than_on_queens`. The per-run half is in
`queen_automation_message` in `crates/swarm-api/src/coordination_delivery.rs`.
The trapdoor fix is `abandon_unsettled_uncertain_run` in
`crates/swarm-persistence/src/queen_conductor.rs`, pinned by
`an_uncertain_run_nothing_could_settle_stops_blocking_automation`.
