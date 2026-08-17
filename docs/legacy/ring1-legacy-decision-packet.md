# Ring 1 legacy decision packet

Status: **Four safeguards closed; nine operator choices recorded 2026-08-17**

This packet contains only choices that survived the full-history evidence pass
and comparison with current Swarm Next. It deliberately excludes findings that
are already prevented, clearly obsolete, or mechanical defects with one safe
answer.

## Immediate safeguards with no product fork

These are ordinary quality work rather than product forks:

1. **Closed:** the Outlook test helper now retains an owned temporary directory
   only for the test lifetime and asserts cleanup.
2. **Closed:** the newest migration and declared current schema version share one
   named ceiling, and a structural test migrates exactly the immediately
   previous version.
3. **Closed:** `1a3c89b` makes every durable coordination writer read the exact
   current provider snapshot before writing. When that snapshot is
   `AwaitingOperator`, the delivery returns to its durable queue without using a
   retry attempt. A real-PTY test holds the same decision through five delivery
   cycles, proves no Swarm bytes entered the picker, answers it as the operator,
   and then observes the queued delivery complete.
4. **Closed:** `7c84cb9` makes typed actor and coarse input shape mandatory
   at the terminal-host write boundary. The holder keeps a bounded, content-free
   audit of accepted and rejected writes, including operator device, Swarm
   coordination, and Steward lease identity; compile-enforced request shapes
   prevent a new ordinary writer from silently omitting provenance.

Items 3 and 4 protect boundaries; they do not decide whether Queen may eventually
answer a provider question.

## Ring 1 overlap that required no product choice

The live desktop terminal could fill its browser container while Claude stayed
at an older narrow PTY width. Legacy's resize history made the operator outcome
recognizable, but Next's cause was its own: geometry authority expired with
attention or was absent after revival. Next now keeps geometry authority
separate from attention, lets the first identified viewer fit an unowned fresh
session, and lets the selected foreground view replace stale cross-device
geometry on its initial attachment (`e8c73a7`). Queen and Scout then passed the
first desktop, Android, and desktop-again proof, but a later wide-desktop Ring 1
session falsified the broader closure claim: the visible terminal filled its
container while both live PTYs still measured `31 x 99`. An already-connected
foreground client could send a resize without reclaiming geometry from an older
attachment, so neither an ordinary resize nor the page's refresh control could
change the PTY.

`30aa2d5` moves that claim into the resize protocol. Every visible-client resize
explicitly reclaims geometry; a hidden client remains passive, and ordinary
terminal input still transfers authority. Focused WebSocket coverage proves an
existing desktop attachment can reclaim geometry after a phone attachment
without reconnecting or sending a sacrificial keystroke. The full web and API
suites passed, the fix is deployed, and the worker-engine process was preserved.
Post-deploy PTYs remained narrow. A later current installed PWA reproduced the
failure more sharply: Scout's visible desktop surface filled the viewport while
the live Linux PTY measured only `7 x 50`. The first browser fit had accepted a
usable transitional mobile measurement, and an unchanged xterm geometry could
then suppress the later host repair. `c27ecc5` requires two stable fit frames
and republishes settled visible geometry even when xterm's own row and column
count did not change. The exact `7 x 50` to `42 x 168` transition is covered by
a browser regression test. The operator-approved protocol-9 migration deployed
that build and restarted the worker engine. The revived Queen then occupied
1,052 by 738 CSS pixels at desktop 1,440 by 900, 398 by 576 at Android-size
412 by 915, and exactly 1,052 by 738 after returning to desktop, with zero page
overflow and no browser warnings or errors. The temporary proof tab was closed.
The live outcome is therefore closed as a mechanical Ring 1 safeguard with no
operator product choice, and no Legacy resize code was ported.

## Decision 1: Queen and provider questions

**Evidence.** Legacy lost 14.8 measured worker-hours to two unanswered pickers.
Its first attempted solutions falsely claimed success, typed a digit as free
text, or lost the refused message. Next now refuses unrelated durable
coordination while the exact current snapshot is a picker and preserves that
delivery without consuming its retry budget. It still deliberately does not
answer the picker.

**Current Next boundary.** `ProviderActivity::AwaitingOperator` is projected as
an attention state and is checked by decision delivery, task briefing, worker
outcomes, and Queen automation. Each path returns the item to its durable queue.
There is no general prompt-answer command or hidden Queen-answer policy.

**Recommendation.** For Ring 1, Queen may observe the question, explain the
choices, and notify the operator, but may not answer automatically. Build the
typed prompt identity, exact-answer, expiry, read-back, refusal, and recovery
contracts first. After real examples accumulate, allow per-prompt-class policy:

- operator-only for trust, permission, destructive, credential, purchase,
  external-message, or otherwise effectful questions;
- Queen-recommend/operator-confirm for ambiguous plan or product choices;
- optionally Queen-answer for explicitly allowlisted, reversible workflow
  choices when the Hive's confidence policy permits it.

**Operator decision (2026-08-17).** Keep Ring 1 notify/recommend-only. Do not
authorize Queen to answer provider questions automatically.

## Decision 2: PTY write evidence visibility

**Evidence.** Legacy could not identify who answered a worker picker because
high-level task and approval logs did not cover the byte-write path. Recording at
the holder choke point made the question answerable without storing secrets.

**Current Next boundary.** Every ordinary holder write now requires a typed
actor and coarse input kind. The holder retains at most 10,000 content-free
events for at most 24 hours, and the API exposes a private no-store read capped
at 1,000 newest entries. The browser does not currently place this evidence in
worker Activity or the normal diagnostics report.

**Recommendation.** Retain a bounded rolling content-free audit—for example,
the newest 10,000 events or 24 hours, whichever is smaller. Show summarized
actor, worker, input kind, write result, and time inside private diagnostics and
dogfood evidence; expose full export only through an explicit private action.
Never show or persist typed bytes.

**Operator decision (2026-08-17).** Keep PTY write evidence diagnostics-only by
default. Do not add a routine recent-input trail to worker Activity.

## Decision 3: completion verification policy

**Evidence.** Late Legacy added citation, branch-containment, per-repository
checker, and Jira-divergence sweeps. The general problem is real, but the exact
checks encode one organization's workflows and arrived after the final packaged
release.

**Current Next boundary.** Task completion already requires durable verification
evidence, and Jira-linked state has explicit convergence machinery. Worker
descriptions can tell Queen about repository-specific release responsibility,
but no global citation, ancestry, branch-containment, or organization-specific
checker framework currently turns those conventions into universal authority.

**Recommendation.** Keep universal completion requirements narrow: durable
verification evidence, confirmed Jira convergence, and explicit release or
handoff evidence when shipping was in scope. Put repository-specific checks in
repository-owned declarative policy or skills, with bounded execution and clear
failure evidence. Promote a check to a global default only after Ring 1 shows it
prevents the same false completion across multiple repositories.

**Operator decision (2026-08-17).** Use repository-owned policies or skills as
the default boundary. Add no organization-specific global checker during the
first Ring 1 week.

## Decision 4: worker shortcuts

**Evidence.** Legacy added operator-defined shortcuts and immediately required
fixes because the settings list was inert and persistence was absent. The need
was real enough to build; the general macro surface was fragile.

**Current Next boundary.** Swarm provides fixed, tested navigation shortcuts for
primary surfaces, adjacent workers, and quick navigation. Those controls pause
while the operator is typing. They are product navigation, not configurable
terminal commands or arbitrary worker macros.

**Recommendation.** Defer arbitrary shortcuts during the first week. Record
repeated operator inputs and mobile friction without content capture. Promote a
small named action only when the same outcome repeats and cannot be handled by a
typed task/decision action, mobile control, or natural-language worker message.

**Operator decision (2026-08-17).** Defer configurable worker command macros and
measure real repetition. Keep the fixed product navigation shortcuts.

## Decision 5: learnable developer preferences

**Evidence.** Legacy's Dreamer and learning-miner experiments tried to turn
operator corrections into future behavior. The useful outcome is personal
adaptation; the danger is hidden policy that silently changes Queen's authority
or spreads one correction across unrelated repositories and operators.

**Current Next boundary.** Swarm Next preserves task history, discussions,
worker descriptions, repository-owned policy, and explicit settings, but it
does not promote repeated operator corrections into reusable guidance.

**Recommendation.** Add a transparent Queen learning-suggestion queue. A
suggestion must cite the bounded correction evidence that produced it, propose
the narrowest useful scope, and remain inert until the operator approves it.
Approved rules are visible and editable in Settings, carry provenance and
revision history, and can be disabled or removed. Rules customize each
developer's Hive; they do not silently expand Queen authority, approve external
effects, expose private terminal content, or become Apiary policy merely because
several Hives behave similarly.

**Operator decision (2026-08-17).** Build the learnable preference outcome with
a Settings ruleset. Keep learning developer-specific, reviewable, scoped,
editable, and explicit rather than automatically mutating hidden policy.

## Decision 6: Queen-discovered routines

**Evidence.** Legacy accumulated pipelines, playbooks, and standing loops as
separate workflow mechanisms. Their underlying value was repeatable work; their
risk was another broad automation surface whose steps, authority, and failure
behavior were difficult to see together.

**Current Next boundary.** Tasks express durable outcomes, repository skills
express repository-owned procedure, and the deterministic coordinator performs
typed policy-complete actions. Next does not yet preserve a repeated multi-step
operator journey as one named, reviewable routine.

**Recommendation.** Queen may recognize a repeated journey and propose a
Routine. The proposal shows its scope, typed steps, Queen judgment points,
trigger or schedule, required approvals, external effects, cancellation, and
failure behavior. It remains inert until reviewed. Settings owns the routine
list and all edit, enable, schedule, disable, and retirement controls. The
coordinator executes only the approved deterministic steps; Queen retains
ambiguity, and every external effect keeps its separate authority.

**Operator decision (2026-08-17).** Use the same transparent learning pattern
for routines. Queen decides when repetition merits a proposal and surfaces it
in Settings; the operator controls its definition and activation. Do not build
a free-form workflow editor before real Queen-proposed routines prove the
necessary vocabulary.

## Decision 7: worker queues are the preparation surface

**Evidence.** Legacy's speculative task preparation attempted to improve
throughput before assignment was certain. It produced wrong-recipient and
context-injection failures because preparation and delivery were coupled to a
live terminal rather than an authoritative worker queue.

**Current Next boundary.** Tasks already carry durable worker assignment,
repository ownership, lifecycle, revision, and queue order. The task lifecycle
enforces one Active item per worker, while other assigned work can remain Ready.
The coordinator and Queen can observe whether a completed worker advances.

**Recommendation.** Do not add a separate staged-work concept. Each worker's
durable ordered task queue is the staging surface: exactly one item may be In
Progress, with no product limit on assigned queued work. On completion, the
worker should normally advance to the next eligible item. Deterministic
coordination may perform an exact policy-complete handoff without a Queen call;
Queen keeps the worker moving when self-advancement stalls or when priority,
dependency, blocker, or ownership requires judgment. No queued task becomes a
terminal injection merely because it is next.

**Operator decision (2026-08-17).** Preserve unlimited per-worker queued work
with one In Progress item. Treat that queue—not speculative preparation—as the
authoritative upcoming-work surface. Workers normally move themselves forward;
Queen intervenes when they do not.

## Decision 8: conversation health and safe renewal

**Evidence.** Legacy's context-pressure watcher injected `/compact`, rotated
sessions, and accumulated heuristics around provider text and timing. The
desired outcome was not an automatic slash command; it was recognizing when a
long-lived conversation had become burdened, confused, repetitive, or detached
from current work, then renewing it without losing worker continuity.

**Current Next boundary.** Provider conversations are retained independently
from worker and App/API lifetime, modern providers perform their own compaction,
and durable tasks preserve the work boundary. Next does not yet combine
provider context metrics, compaction events, task transitions, repeated
failures, and operator corrections into an explainable conversation-health
assessment.

**Recommendation.** Add visible conversation health to each loaded worker.
Queen may recommend a clean provider conversation at a safe task boundary and
must show the evidence behind that recommendation. Renewal preserves the worker
identity, repository, task queue, durable history, and a reviewed handoff
summary. It never silently replaces an actively working conversation, never
uses terminal text as sole authority, and never treats provider compaction
alone as failure.

**Operator decision (2026-08-17).** Build the explainable conversation-health
and Queen-recommended safe-renewal outcome. This is the intended successor to
Legacy's context-pressure automation, not a port of `/compact` injection or
timer heuristics.

## Decision 9: cross-device return briefing

**Evidence.** Legacy's Command Center attempted to summarize fleet state, but
some surfaces were assembled UI rather than a proven event-driven operator
brief. Swarm Next has durable task, decision, worker, sync, resource, presence,
and Apiary evidence, yet a returning operator can still need to inspect several
workers to understand what changed while away.

**Current Next boundary.** Needs You, Queen, Night Watch, device-aware presence,
notifications, and the Keeper overview expose their own state. They do not yet
compose one acknowledged, cross-device summary of meaningful changes since the
operator last checked in.

**Recommendation.** Queen prepares a quiet Return briefing when the operator
comes back from Away or Night Watch, including completed or shipped outcomes,
running work, blockers and decisions, stalled or recovered workers, failed
external synchronization, important resource or update events, and Queen's next
plan. It appears as a dismissible surface in Queen and Needs You rather than a
blocking modal. Opening the mobile PWA also counts as a check-in and receives a
compact mobile-first version. Read and dismissal state synchronize across the
operator's devices so reviewing it once does not create duplicate noise.
Keeper receives only cross-Hive exceptions and meaningful rollups, not routine
worker chatter.

**Operator decision (2026-08-17).** Build the cross-device Return briefing with
mobile as a first-class check-in surface. Make it useful both for returning to
work and for briefly checking on the Hive while away.

## Evidence that would change these recommendations

- A naturally occurring prompt class that Queen can answer safely and repeatedly
  without external effect.
- A terminal incident that requires more than content-free actor/shape evidence.
- The same completion failure across multiple unrelated repositories.
- A repeated mobile or desktop command whose current path is materially slower
  or unreliable.
- Repeated operator corrections that support one stable, narrowly scoped rule
  without expanding authority or contradicting later corrections.
- A repeated multi-step journey whose trigger, deterministic steps, Queen
  decisions, approvals, cancellation, and failure outcome can be stated
  explicitly.
- A conversation-health recommendation whose provider metrics, task boundary,
  repeated failures, compaction history, and operator corrections explain why
  renewal would improve the worker without interrupting active work.
- A return interval whose durable changes can be summarized without repeating
  routine worker chatter or showing the same unread briefing independently on
  every device.

Decisions should be recorded after that evidence or an explicit operator choice,
not inferred from Legacy commit volume.
