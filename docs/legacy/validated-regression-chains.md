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
