# Ring 1 legacy decision packet

Status: **Four safeguards closed; focused operator choices not yet recorded**

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
a browser regression test. Closure still requires one final live proof after
that build is deployed. This remains a mechanical Ring 1
safeguard with no operator product choice, and no Legacy resize code was ported.

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

**Operator choice.** Keep Ring 1 notify/recommend-only, or authorize an earlier
small allowlist of Queen-answerable prompt classes.

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

**Operator choice.** Diagnostics-only by default, or a small recent-input trail
in each worker's Activity surface.

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

**Operator choice.** Accept repository-owned policies as the default boundary,
or nominate one additional cross-repository check for the first Ring 1 week.

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

**Operator choice.** Defer and measure, or identify one known daily shortcut that
must be present before Ring 1 starts.

## Evidence that would change these recommendations

- A naturally occurring prompt class that Queen can answer safely and repeatedly
  without external effect.
- A terminal incident that requires more than content-free actor/shape evidence.
- The same completion failure across multiple unrelated repositories.
- A repeated mobile or desktop command whose current path is materially slower
  or unreliable.

Decisions should be recorded after that evidence or an explicit operator choice,
not inferred from Legacy commit volume.
