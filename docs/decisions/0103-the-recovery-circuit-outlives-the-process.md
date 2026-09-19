# ADR 0103: The recovery circuit outlives the process, and a new build earns one attempt

Status: Accepted, 2026-09-18.

⚠️ PROVENANCE, STATED EXACTLY, BECAUSE THE OBVIOUS CITATION IS WRONG. The
authority is a direct in-session operator instruction: asked which of three
behaviours they wanted, they asked for a recommendation, and on being given
"a new build earns one fresh attempt" they answered "build it".

Decision card `01a0b749-5def-7cf2-885b-360bada82c8b` was filed for the same
question and is RESOLVED AS `dismissed`, noted "This was handled in the worker."
It records no answer. Citing it as the authority would be citing an empty
record — the failure this Hive already has a rule about, where a placeholder is
read as the operator's words.

The automatic recovery circuit was correct inside one process and meaningless
across two. It now lives in `worker_recovery_circuit`, and a changed build
revision — not a restart — is what returns a worker's one attempt.

## What was already right, because most of it was

The supervisor gives an autostart worker with no live session ONE automatic
attempt; a second failure opens a circuit and stops. Policy deferral is checked
before recovery accounting, so a Night Watch hold cannot spend the attempt. The
stability window is preserved rather than restarted on each pass, because
restarting it means it never matures. "Is it coming up" is asked instead of "has
it exited", because a worker that never gets a session has not exited either, so
the circuit would never open and nothing would ever say so.

That last one matters here: the code already names a permanent silent stall as
strictly worse than the bug it was fixing. This ADR is decided the same way.

## The defect

`worker_recovery_attempts` and `worker_errors` were `Arc<RwLock<HashMap<..>>>`,
constructed empty with nothing to rehydrate them. So every API restart handed
each worker a fresh attempt and erased the failure the operator had been shown.
The comment beside the circuit says it exists to stop an unstartable worker being
restarted forever; across restarts it did not, and AGENTS.md requires every retry
policy to be bounded. Demonstrated incidentally: reloading to `3b0d267e1f7b`
wiped all in-flight circuit state.

⚠️ MEASURED BEFORE BUILDING, and it did not change the decision but it does
change the urgency: ZERO workers were in the gap. Of autostart, non-archived
workers, none lacked a live session. This was latent, not an incident, and
nothing here should be read as having rescued a stranded fleet.

## The choice, and why not the other two

**A new build earns one fresh attempt.** A restart on the same build tells you
nothing new about whether a worker can start, so it buys nothing. Shipping
different code is the one event that plausibly changes the answer, so it buys
exactly one.

Rejected — *an absolute bound until the operator clears it*: shipping the very
fix that would revive a worker leaves it down anyway, explained only by a string
that was itself being erased on restart. That is the permanent silent stall the
neighbouring code already rejects by name.

Rejected — *a fresh attempt on any restart*: nearest to the old behaviour, which
is the defect. Frequent restarts mean frequent retries, and the bound stops
meaning anything.

The named cost of the accepted answer: a worker unstartable for reasons unrelated
to the build is retried once per deploy. Bounded, not once-and-for-all. If deploys
become frequent AND stuck workers common, "once per deploy" approximates
"always" and this should be revisited — that is the condition to watch, stated
now rather than discovered later.

## Two things that look like details and are not

**Reconciliation happens on READ, not on write.** A build that never runs a
supervisor pass cannot leave a stale grant behind for the next one to honour.

**Clearing a failure does NOT return the spent attempt.** It reads as the
generous thing to do and it reinstates the unbounded retry: a worker that starts
and dies a second later would earn attempt after attempt, because each start
clears the failure. `clear_worker_recovery_failure` nulls the failure and leaves
`attempted_at` alone; only the stability window in the supervisor retires an
attempt, and it clears the durable row when it does.

Both are tested, and both ablate: removing the build-revision reconciliation
fails only the fresh-attempt test, and making the failure-clear delete the row
fails only the spent-attempt test.

## What this does NOT do

The escalation is durable; it is still not ON THE BOARD. An opened circuit
survives a restart and is readable, but nothing files a decision or raises an
attention, so nobody is asked — the same shape as 01a0b1de.

That gap is structural rather than an oversight, which is why it is not fixed
here: `coordinator_actions.task_id` is NOT NULL, so every attention is
task-scoped, and a worker that cannot start and owns no active task cannot be
expressed in that table. The nearest detector,
`exited_worker_owned_work_candidates`, requires `task.state = 'active'`. Closing
it means a nullable `task_id` or a different surface, and that is its own
decision rather than something to smuggle in beside a persistence change.

## Closed, 2026-09-19: the escalation reaches the board

⚠️ THE SECTION ABOVE IS SUPERSEDED. "It is still not ON THE BOARD" was true when
written and is not any more.

Operator decision `01a0b8da-c8f9-7c01-947f-6271f5b5caff`, answered in their own
words: **"Make `coordinator_actions.task_id` nullable, extending every attention
reader."** Schema 182 does exactly that, and `worker_cannot_start_attention` is
the first attention that names a worker without naming a task.

What the section above got right is why it could not simply be bolted on: every
attention was task-scoped, so a worker that cannot START — owning nothing — could
not be written down at all. The circuit recorded a failure that nothing could
raise.

Three things worth keeping in view:

**Only that one kind may omit a task.** The rebuilt table carries
`CHECK (task_id IS NOT NULL OR kind = 'worker_cannot_start_attention')`. Without
it a task-scoped attention could silently lose its task and read as a
worker-level condition, which is the inverse of this fix.

**`LIVE_ATTENTION_SOURCE` is deliberately untouched.** It has six consumers
across the coordinator, conductor and recovery paths, and its freshness test is
`task.updated_at = action.evidence_revision` — meaningless for a condition with
no task. Destabilising six coordination queries for a gap with zero live
instances is the wrong trade, so the worker-scoped kind gets its own source and
the existing six stay byte-identical.

**It self-clears.** The condition is "the circuit is still open", so clearing the
failure ends the attention without anything having to dismiss it — the same
property every other kind in that table has.

Wired into `run_deterministic_coordinator`, reached from `deliver_coordination`,
which production calls. That chain was traced rather than assumed, because this
repository has documented three separate detectors whose only callers sat in
`#[cfg(test)]` and whose silence therefore read as health.

Still measured at zero: no worker is in this state today. Latent coverage, now
reachable.
