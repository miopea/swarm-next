# ADR 0055: When a worker may reload this Hive

## Status

Accepted, 2026-08-25. **Supersedes ADR 0051's presence rule**, on the operator's
ruling recorded as decision `01a038b0-06b4-7142-be35-e449623ea37a`: "Supersede
ADR-0051 — I will define safe on the task." The rest of ADR 0051 stands: two
actions rather than one, only the worker whose workspace is the checkout, no
release, and `status` changing nothing.

## What was wrong with the old rule

ADR 0051 refused a worker-initiated reload while the operator was at the Hive or
holding a terminal. It was a crude rule that is always safe, and it was chosen
deliberately over queueing.

It cost more than it protected. Six tasks sat in Review for hours on 2026-08-25,
finished and pushed and unverifiable, because the only thing that could make them
demonstrable was a reload nobody was allowed to perform. The operator's own
summary: "otherwise it's hanging up at a human and it makes no sense."

That is the failure this fleet keeps finding in other forms — an answered
question converted into an unanswered one and handed to a person.

## What "safe" means

The operator defined it, in these words:

- "safe reload means you can do a reload without breaking the existing workers,
  which should be fine on basically any app reload"
- "you're not going to mess up my usage if we start a reload and I get a quick
  refresh, that's just development"
- "you should be clean to do your own reload when you finish your work"

Two things follow, and the second replaced the first.

**Operator presence is no longer a refusal.** Workers survive a reload because
the terminal host is a separate service — which ADR 0051 already noted, and which
is what makes an app reload safe by the operator's definition. The disruption the
old rule protected against is a page refresh, and they have called that
acceptable.

**A worker's own unfinished work is a refusal.** "When you finish your work" is
the operative clause. A worker holding an Active task is mid-sentence, and
restarting the API under its own in-flight work is the case nobody wants. This is
a state query — `list_visible_tasks` filtered to Active — not a judgement of the
moment: the worker does not decide whether now is a good time, the board does.

The other three refusals are untouched and unrelated: a checkout that does not
contain the deployed source, nothing to reload, and a build already in flight.

## Observability, which is the condition this was relaxed on

A reload the operator did not press is recorded with the worker that asked for
it. The status file carries `requested_by`, and the control room's own button
writes `operator`. They should be able to see that a reload happened and who
caused it, not discover it by the surface changing under them.

## The conflict of interest, stated rather than solved

A worker must not reload to make its own task closeable. The tasks blocked on
2026-08-25 were the reloading worker's own, and that is exactly the shape to
worry about.

This ADR does not claim to have closed it. What it does:

- **Refuses while the worker holds Active work**, so a reload cannot be part of
  finishing something.
- **Attributes every worker-initiated reload**, so a reload followed by that
  worker recording evidence is visible as one sequence rather than two facts.

What it relies on is the evidence discipline that already exists: a reload makes
code run, it does not make code demonstrated. On the day this was written, the
same worker declined to record deployments on three tasks whose code was running
because it could not demonstrate them. That discipline, not the reload guard, is
what protects the board — and if it fails, this rule should be revisited rather
than patched.

## What this does not change

It does not let a worker release. It does not let Queen reload — she coordinates,
she does not build. It does not change what "deployed" means. And it does not
permit restarting WORKERS, which is a different operation: the operator has said
that is safe only when every worker is resting, and nothing here implements it.

## Where the behaviour lives

`reload_app` in `crates/swarm-api/src/agent.rs`, with the shared guards in
`crates/swarm-api/src/maintenance.rs`. Pinned by
`a_reload_waits_for_the_worker_to_finish_rather_than_for_the_operator_to_leave`,
which asserts presence no longer refuses, active work does, and reporting that
work to review lifts it.
