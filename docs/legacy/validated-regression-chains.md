# Validated legacy regression chains

These chains were selected from the generated ledger, then checked against the
commit messages and touched implementation/test files. They establish operator
outcomes and architectural constraints; they are not port instructions.

## Terminal ownership, reconnect, and resize

`c7e2efb7` replaced tmux with a direct PTY holder, but the following days still
needed `0f41d62d`, `b203c945`, and `7540f7cb` for holder races, runtime behavior,
death notification, and spurious revival. `98f50970`, `f23d6006`, `21b1c087`,
and `511b6c53` then corrected worker switching, partial ANSI snapshots,
scrollback retention, and resize stability. The fixes span the holder, pool,
process, bridge, dashboard, and focused tests rather than one cosmetic layer.

**Stable outcome:** the terminal process owner must be independent from the web
app; reconnect and resize require explicit protocols, bounded journals, and
idempotent process identity. Swarm Next keeps those contracts in the Rust worker
engine and tests app reload separately from holder replacement.

Ring 1 reproduced the operator outcome on 2026-08-16 through a different cause:
the browser renderer filled its desktop container, but the shared PTY retained
an older narrow geometry after attention moved between devices and after a
fresh worker revival. `ca91cc3f` separated durable geometry authority from the
expiring operator-attention lease, and `5871b7f0` let the first identified
viewer fit a newly revived, otherwise unowned terminal. A later Ring 1 repro
showed that a dormant session could still retain another device's width;
`e8c73a7` lets an explicitly selected foreground attachment claim its viewport
at its initial WebSocket attachment. Focused WebSocket tests proved that
background and later passive viewers could not steal geometry and that actual
input transferred it. The first live proof then took Queen and Scout through
desktop, Android, and desktop again with zero page overflow and rendered rows
reaching the terminal's right edge.

A subsequent wide-desktop Ring 1 session exposed one more ownership boundary.
The renderer occupied the full container while both Queen and Scout PTYs
measured `31 x 99`; the already-connected foreground PWA's resize messages were
passive, so maximizing, resizing, and the page refresh control did not reclaim
geometry from the older attachment. `30aa2d5` adds an explicit
`claim_geometry` bit to every resize: visible clients claim, hidden clients stay
passive, and the server preserves backwards-compatible unowned claims for older
clients. An integration test now has an existing desktop socket lose ownership
to a phone socket and reclaim it by resizing, without reconnecting or typing.
Full web and API validation passed and App/API deployment preserved the live
terminal-host and provider PIDs. Final live closure is intentionally pending
until the installed PWA loads the new asset and the PTY width is remeasured.
This still addresses the Next-owned protocol rather than copying Legacy's
dashboard resize machinery.

## Automated input authority and operator focus

`d547b6e2` added a terminal-active guard after drones and Queen injected while
the operator typed. `599d0947` blocked bash approval hidden inside an
accept-edits prompt. `b6b568e7` replaced a fragile state blacklist with a narrow
BUZZING-only whitelist. `de1763d1` later bound deferred actions to state and
process snapshots while fixing six other PTY/drone races. Each change touched
the automation path and its tests, not only presentation.

**Stable outcome:** current terminal text cannot grant authority. Swarm Next
uses explicit operator engagement, durable role identity, revision-bound
actions, and at-most-once uncertain delivery. Provider-native approval removes
most legacy prompt clicking; external effects remain separately authorized.

## Revive loops and retry identity

`703c9ca3` records revive timestamps outside a counter that reset during brief
BUZZING transitions, caps three rapid revives in sixty seconds, and escalates.
`de1763d1` then made reaping, disconnect, deferred actions, and holder discovery
safe under concurrency.

**Stable outcome:** retry budgets need durable or independently monotonic
identity, not state that a successful intermediate transition clears. Swarm
Next never replays an ambiguous coordinator wake and surfaces uncertainty to
the operator.

## Speculative preparation and wrong-recipient work

`8b693339` introduced speculative task preparation along with several unrelated
features. `d44ee3e7` disabled it hours later after arbitrary pending tasks reached
unrelated workers. `6b4b061a` restored it only behind exact target-worker
matching, rate-limit awareness, operator inactivity, and an opt-in defaulting
off. The sequence changed the drone pilot and later added configuration guards.

**Stable outcome:** speculative delivery remains deferred in Swarm Next. A task
must already have durable ownership before deterministic coordination can wake
or brief a worker. Cancellation and wrong-recipient proofs are required before
any broader preparation returns.

## Mobile terminal scrollback

`d7ef6ff8` added a fullscreen mobile terminal. The same-day chain
`193eec32`, `10282919`, `73a4c051`, `65fcc301`, `117a621f`, and `fe48aeb0`
cycled through synthetic wheel events, raw escape sequences, two-finger
gestures, tmux copy mode, auto-exit, and terminal reuse before the interaction
held. Most changes concentrated in one dashboard template and terminal route.

**Stable outcome:** mobile scrolling needs one owned gesture path with direct
tests at touch size, not layers of browser, tmux, and copy-mode translation.
Swarm Next maps a one-finger captured gesture directly to xterm scroll lines and
has an Android CDP smoke that proves the viewport actually moves.

## State classification and the continuous-event boundary

`700255f8` used captured real buffers and fixtures to show that completed Claude
turn summaries were being classified as active work. It split ambiguous
patterns, restored sleeping timestamps, and added positive controls. Correcting
the classifier changed the fleet from a near-dormant event stream to a
continuous one, exposing the later browser-process memory incident described in
the atlas.

**Stable outcome:** provider/runtime events and durable lifecycle ownership are
authoritative; terminal text is bounded supporting evidence. Swarm Next keeps
one invalidation stream, quiet steady state, content-free diagnostics, and
browser-process soak evidence.

## Provider prompt authority and recoverable refusal

`fe4e1eb4` first stopped ordinary automated text from answering an open provider
prompt. The later `de3870ae`, `a184cfaf`, `be122e22`, `58339e99`, `5576f21d`,
`85491fc4`, `af324d2c`, `72c66362`, `78c1ef4`, `cbca9aeb`, `314f05bf`, and
`b338f1c8` sequence shows why a hold alone was insufficient. Legacy needed a
stable prompt fingerprint, structured choices, explicit answer and dismiss
verbs, cursor-relative navigation rather than typed digits, read-back before
claiming success, refusal instead of ineffective interrupt, and a recoverable
copy of a message refused while the prompt was open. The tests use captured
real prompts, and live probes falsified several code-reading assumptions.

The last code in this chain is after Legacy's final packaged `2026.8.13`
boundary, so its mechanism is not release-proven. The incident evidence is
nevertheless strong: two workers lost a measured 14.8 hours on unanswered
pickers, and an automated message was observed to disappear on refusal.

**Stable outcome:** a provider question is a typed attention object and an input
authority boundary. Unrelated automation must refuse without losing its body;
an authorized answer must bind to the exact current question and report written,
observed, and accepted as different facts. Swarm Next already classifies a
visible picker as `AwaitingOperator`, but its coordination delivery currently
does not consult that state before the shared `HostRequest::Write` path. The
outcome survives; Legacy's terminal parser does not automatically come with it.

## Every terminal writer is attributable

`ec70c136` moved PTY-write attribution to the holder choke point after a picker
was answered six times without any record of who supplied the input. It recorded
actor, worker, timestamp, byte count, and a coarse input kind while deliberately
excluding content. `a562a02f` isolated the audit during tests, `f3057e8f` asserted
actor propagation on every path, and `9d568098` added a sweep that fails when a
new writer omits its actor. The sequence then supported worker-level commit
identity and live holder-drift diagnosis in `4dd95466` and `f40dd284`.

This chain is also post-`2026.8.13`; treat the implementation as development
evidence. Its architectural lesson is independent of the JSONL mechanism.

**Stable outcome:** terminal input provenance belongs at the single byte-write
boundary, with `unknown` as a visible failure state and no secret-bearing content
in the record. Swarm Next has strong actor provenance for task activity and
durable high-level deliveries, but both operator and automation bytes still
converge on `HostRequest::Write { session_id, bytes }`. Add typed provenance and
a bounded content-free audit at that boundary rather than trying to reconstruct
an incident from surrounding task events.

## Approval rules, effect gates, and brakes

`814876ca` and `19730e44` showed that deletion-oriented deny rules missed SQL
mutation, privilege grants, credential persistence, device writes, and package
publication. `dccf03c8` proved that a safe word in one compound-command segment
could approve everything around it. `70f9d10b` then separated safe-looking verbs
from dangerous effects such as credential reads and outbound payloads. When
those guards changed from advisory escalation to denial, `9173b1ed` immediately
found ordinary commands blocked. `5f16ad5f`, `d6476e99`, and `26796719` ultimately
separated a hard effect gate from a human-approval brake because one return type
could not safely represent both.

The chain repeatedly measured both hazards and ordinary-work corpora. It also
states its honest limit: substring configuration cannot become a complete shell
security boundary, and a denylist cannot recognize every sensitive object or
legitimate destination.

**Stable outcome:** hard boundaries are typed effect contracts owned by code;
operator convenience rules cannot widen them. Human-review brakes and absolute
gates need different types, defaults, and failure behavior. Swarm Next does not
port Legacy's approval drones or regex policy engine. Its deterministic
coordinator is restricted to typed application operations, so the mechanism is
obsolete while the design constraint remains mandatory for each future action.

## Migration ceiling and temporary artifact ownership

`93284b41` changed one version constant after discovering that a previously
shipped migration could never execute. `da7d8a10` replaced `tempfile.mktemp`
patterns after the test suite accumulated 16 GB of abandoned files and pushed
the disk to 95 percent. Both failures were silent infrastructure drift rather
than user-facing feature logic.

**Stable outcome:** the latest migration step and current schema version need a
mechanical invariant, and every temporary artifact needs one explicit owner with
cleanup proven by a test. Next's forward-only SQLite design and owned Rust
temporary values substantially reduce both risks. Ring 1 found and removed one
small recurrence: the Outlook probe helper deliberately retained its temporary
OAuth directory. The helper now returns the owned `TempDir` and proves cleanup,
while the newest migration uses one named schema-version constant and a test
migrates exactly `CURRENT_SCHEMA_VERSION - 1` to the declared ceiling. These are
ordinary safeguards, not reasons to port Legacy's migration or scratch system.

## Jira closure and divergence evidence

`3bbea838` ran a project citation check when a Jira ticket closed. `dbe59a07`,
`11f0e495`, and `ea33e4a0` built a daily, per-repository verification sweep, and
`1f559e84` added board-versus-Jira divergence. Nearby `862a49fd` and `35854dd9`
showed that ancestry alone cannot prove squash-merged work reached main.

These are all post-release development changes. They demonstrate recurring
operator need for canonical-state convergence and trustworthy completion proof,
not a universal requirement for these exact scripts.

**Stable outcome:** linked Jira terminal states must reconcile without depending
on a bounded open-issue query, and a completion claim must retain concise durable
verification evidence. Swarm Next already fetches exact linked issue identities,
including closed issues, and persists mapped inbound and outbound transitions.
Project-specific citation, branch-containment, or deployment checks remain
optional typed policies that should be adopted only when Ring 1 evidence shows
they prevent real false completion.
