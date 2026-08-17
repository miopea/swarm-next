# Ring 1 legacy decision packet

Status: **Two safeguards closed; focused operator choices not yet recorded**

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
3. **Pending boundary safeguard:** refuse unrelated automated terminal delivery while the exact current provider
   activity is `AwaitingOperator`. Preserve the intended delivery as recoverable
   rather than marking it delivered, dropping it, or replaying it later without
   context.
4. **Pending boundary safeguard:** carry typed actor and input-shape provenance through every ordinary terminal
   write and record it at the terminal-host choke point without content.

Items 3 and 4 protect boundaries; they do not decide whether Queen may eventually
answer a provider question.

## Ring 1 overlap that required no product choice

The live desktop terminal could fill its browser container while Claude stayed
at an older narrow PTY width. Legacy's resize history made the operator outcome
recognizable, but Next's cause was its own: geometry authority expired with
attention or was absent after revival. Next now keeps geometry authority
separate from attention, lets the first identified viewer fit an unowned fresh
session, and lets the selected foreground view replace stale cross-device
geometry on worker selection or refresh (`e8c73a7`). Later passive devices
cannot steal the shared size, while real input transfers authority. Queen and
Scout passed desktop, Android, and
desktop-again proof. This is classified **already prevented after Ring 1 fix**;
there is no remaining operator decision and no Legacy resize code was ported.

## Decision 1: Queen and provider questions

**Evidence.** Legacy lost 14.8 measured worker-hours to two unanswered pickers.
Its first attempted solutions falsely claimed success, typed a digit as free
text, or lost the refused message. Next already recognizes a picker but does not
yet guard automated writes with that state.

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
