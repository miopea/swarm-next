# Legacy stable release boundaries

Status: **Validated snapshot sample; runtime success is not inferred from code survival**

## Question this pass answers

The complete ledger says when a capability changed. It does not by itself say
whether that change survived packaging, whether its UI actually initialized,
or whether a late repair ever reached an operator release. This pass samples
the explicit packaged boundaries and checks selected implementation owners in
the trees at those boundaries.

“Present at a later release” means only that the implementation remained in the
packaged tree. It is stronger than a commit subject and weaker than live proof.
Ring 1 evidence, regression commits, and captured incidents can still contradict
an apparently stable file.

## Boundary map

Legacy has no explicit `release:` marker until sequence 936. The first 935
reachable commits—from the 2026-02-07 root through most of 2026-04-16—are launch
and development evidence, but the repository cannot identify their exact
packaged boundaries.

| Sampled packaged boundary | Ledger sequence | Releases in month | What the boundary proves |
| --- | ---: | ---: | --- |
| `2026.4.17` (`0d689429`) | 936 | 29 | First explicit package marker; earlier history cannot be called release-proven from Git alone. |
| `2026.4.30` (`ce8f4f12`) | 992 | 29 | April verifier and context-pressure implementations are present. |
| `2026.5.31.14` (`6d2888cd`) | 1,187 | 152 | April automation and May Command Center code survived a later monthly package. |
| `2026.6.27` (`370a7b13`) | 1,243 | 41 | Fan-out limits, engagement logic, and external-blocker task state are present. |
| `2026.7.30` (`3925e58e`) | 1,285 | 14 | Outlook intake and the repaired Command Center are present. |
| `2026.8.13` (`3d477fc9`) | 1,458 | 139 | Last explicit packaged boundary; prompt-write guard and Queen-only broadcast are included. |
| `origin/main` (`1f559e84`) | 1,529 | not packaged | Seventy-one later commits are development evidence only. |

Release count is not a quality score. The most concentrated days produced 30
release commits on 2026-08-06, 28 on 2026-08-09, 24 on 2026-05-05, and 20 on
2026-08-10. Those bursts are evidence of rapid operator feedback and repair,
but a same-day version bump cannot substitute for a sustained soak.

## Outcomes that survived more than one packaged boundary

### Deterministic work before LLM work

`4249a39f` introduced a two-tier completion verifier: deterministic evidence
checks ran before an optional LLM judgment. Its verifier owner remains present
from `2026.4.30` through `2026.8.13`. `607e3507` added a bounded,
hysteresis-aware context-pressure owner in the same period.

**What survives:** mechanical evidence and resource policies should run below
Queen, with explicit thresholds, bounded retry, and visible outcomes.

**What does not follow:** Next does not need the verifier subprocess or terminal
`/compact` injection merely because their files survived. Current providers,
task evidence, and resource ownership differ. Treat both mechanisms as
**unresolved evidence** until Ring 1 shows a false-completion or context-pressure
journey that a typed policy would prevent.

### Fleet fan-out and operator engagement are real hazards

`fc86c671` stopped one worker message from waking an entire fleet;
`b3ba6612` added engagement-aware Queen prompts and duplicate-handoff
suppression; `0a35a73e` made external blocking visible without repeated nudges.
Their owners are present at `2026.6.27`, `2026.7.30`, and `2026.8.13`.

**What survives:** fan-out, duplicate delivery, and “blocked but still open” are
durable operator problems.

**Next comparison:** direct worker broadcast is removed, worker outcomes return
through Queen, delivery is revision-bound, engagement leases protect operator
focus, and Blocked is first-class task state. The outcomes are classified
**already prevented or redesigned**; Legacy's message-body similarity,
poll-driven nudges, and advisory “always send” prompt path are not candidates to
port.

### Email is a real task-intake journey

`0e3d0f3f` added Graph-backed multi-select Outlook intake with separate-task and
merge modes. The route remains present at `2026.7.30` and `2026.8.13`.

**What survives:** email intake is not an incidental convenience. Operators
need selection, merge, durable source linkage, attachments, assignment fields,
and a reviewed reply after completion.

**Next comparison:** Swarm Next keeps this outcome with a typed email source and
reviewed reply boundary. Ring 1 has already refined the requirement beyond the
Legacy snapshot: inline images must render, multiple messages may become one
task, and assignment and task fields belong in the import flow. This is a
**relevant redesign**, not a UI port.

## Presence that did not prove working behavior

### Command Center code survived while initialization was dead

`9800ce62` introduced the Command Center in May, and its markers remain in each
later sampled package. Yet `cac94f63` records that a cross-IIFE reference had
prevented its initialization since `2026.6.8.2`; resize handles, digest polling,
and its default view were dead until the July repair.

**Lesson:** tree survival and test volume cannot prove a composed browser
surface initialized. Swarm Next must keep rendered desktop and mobile journeys,
not static template scans alone. The Legacy panel layout is **obsolete as an
implementation**; Queen, Needs You, Tasks, and Workers remain the outcomes to
prove in Next.

### The packaged prompt guard was a beginning, not closure

`fe4e1eb4` entered the final `2026.8.13` package and correctly established that
ordinary automation may not answer an open provider prompt. The twelve-commit
follow-up after that package added stable prompt identity, typed choices,
cursor-relative answers, read-back, refusal, and recoverable held messages.

**Lesson:** the authority boundary is release-backed; the reliable answer and
recovery mechanism is only post-release development evidence. Next should
implement the boundary and typed recovery contracts, not claim that Legacy's
parser was a settled solution. This remains a **relevant redesign** and a
focused Ring 1 decision about how much answering authority Queen receives.

### Queen-only broadcast held, but Next can remove more

`ee54b7fc` moved broadcast authority to Queen before the last package. It is
strong evidence that unrestricted fleet messaging harmed focus and capacity.

**Next comparison:** Next goes further: ordinary worker-to-worker broadcast is
absent, targeted outcomes are durable, and Queen is the coordination point.
The constraint is **already prevented**; a Queen broadcast should return only
for an explicit operator journey, not because Legacy retained one.

## Post-release development evidence

The final 71 commits contain useful outcomes but no later package boundary.
Examples include the wired daily verification sweep (`dbe59a07`, followed by
`11f0e495` and `ea33e4a0`), exact Jira closure/divergence checks, and the fuller
provider-prompt and PTY-write attribution chains.

The first verification-watcher commit explicitly said it was not wired, which
is a useful control: a green, tested component was not yet a running feature.
These commits may justify typed Next policies or safeguards after comparison
with Ring 1 evidence, but are classified **unresolved development evidence**,
not stable Legacy behavior.

## Release-boundary rules for the remaining atlas

1. A commit subject identifies a hypothesis, not a shipped outcome.
2. Presence at a later package proves code survival, not initialization or live
   success.
3. A repair after the final package can establish an incident and design
   constraint without establishing a stable mechanism.
4. Same-day release density increases the need for a soak; it does not increase
   confidence by itself.
5. A Legacy outcome enters the Next decision queue only after current owners,
   tests, and Ring 1 behavior are compared.

