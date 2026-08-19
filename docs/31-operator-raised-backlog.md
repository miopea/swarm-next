# Operator-raised backlog

Status: **Running capture — 2026-08-19**

Items the operator raised during live dogfooding, written down as they arrived
rather than held in a conversation. Everything here came from using the product,
not from planning it.

The Swarm Next worker cannot create durable Swarm tasks: `swarm_create_task` is
Queen-only through MCP, and a worker holds list, transition, comment, and
decision tools. Queen should file anything here that deserves queue tracking.
This file is the durable record until then.

## Open

### 4. A task needs one operator instruction line — *fixed*

The operator frequently wants to say something that governs how a task is
approached rather than what it contains: "interview me first", "analyse this,
do not act on it". Today that has to go into the description, where it reads as
part of the work.

Wanted: a single overarching operator comment on a task, distinct from the
description and from activity notes.

Fixed across `7c61be0` and `74f88bc`: a task carries one line, bounded to 280
bytes and a single line, and the briefing states it before the work. Creating a
task cannot carry one yet — that path goes through a constructor shared with
the Jira and email intakes.

### 10. A worker-engine update loses the roster it promised to restore — *fixed*

Raised as: coming back from a worker engine update, the previously active
workers did not return and had to be woken one at a time. An error was visible
at the same time (`Runtime request returned 524`).

Root cause: the list of workers to revive exists only as a local variable inside
the maintenance request handler. The handler stops every worker *first*, then
waits up to 45s for the engine to report the new release, and only revives on
the success path. Any interruption — our own timeout, a proxy timeout, or the
API process restarting — discards the list permanently, and the workers are
already stopped. What still runs on the failure path is `supervise_workers`,
which starts *autostart* workers only, which is why one worker came back and
the rest did not.

The promise on the card is "briefly stops 7 active workers, then revives them".
That promise cannot be kept by a value that dies with the request.

Fixed: the roster is written to the database before anything is stopped, and
the supervisor brings back whatever is still owed, on this pass or a later one.
Recording it is now a precondition — if it cannot be written, no worker is
stopped. Revival waits for the engine to settle, so a worker is never handed
back onto the engine being replaced. Intents age out after fifteen minutes so
they cannot wake workers the operator later chose to leave asleep.

### 11. The update indicator does not use the words the settings page uses — *fixed*

Raised as: there was no indication at the top that a *worker* update was
running; there was one for the app, and it said "engine", which matches nothing
on the settings page. Settings names two things: "Worker engine" and "App and
API". The header must use those names, and must appear for both.

Fixed: the indicator now says "Worker engine update" and "App and API update",
and it reports a worker-engine replacement while it is running — the only
in-progress state it had was the app build, which is why the update that
actually takes workers away ran unannounced. A running engine replacement
outranks everything else in the indicator for the same reason.

### 12. The app updater should look like the worker-engine updater — *fixed*

Raised as: the worker-engine card is the better UI — it states what will happen,
then shows inline progress in place ("Updating worker engine... Stopping active
workers and preserving their conversations"). The app/API reload should work the
same way. This supersedes item 7's open question of where to put progress: the
answer is in the card, where the operator started the action.

Fixed: the App and API card carries the same live progress block as the worker
engine card while a build runs, and separates a build that has been asked for
from one that is under way. Previously a build changed only the wording on the
card, which reads as another resting state.

### 13. Adding an image fails once, then succeeds on retry — *partly fixed*

Raised as: "image could not be added" on the first attempt, worked on the
second. Seen shortly after a worker-engine update, so an API restart mid-upload
is a candidate, but the retry-succeeds shape means this needs reproducing before
it is diagnosed.

Two things found without reproducing it. The upload was the one runtime call
that did not go through the shared transient-failure recovery, so an API
restarting underneath it failed outright while every other call rode it out —
the operator was retrying by hand what the runtime already does for itself.
And the proxy timeout the operator saw the same afternoon, `524`, was not in
the set of statuses treated as transient at all, so nothing retried it either.

The cause of this particular failure is still unproven: the reason was
discarded by an empty `catch`. It is now shown, so the next occurrence
identifies itself.

### 14. Three workers claim the operator at once — *fixed*

Raised as: three workers show "with you" while the operator is plainly in one
session. Observed as BFG Operations `WITH YOU · 4M`, BudgetBug `WITH YOU · 4M`,
and Swarm Next `WITH YOU` together.

"With you" is the one worker state that is exclusive by definition — the
operator is in one place. Three simultaneous claims mean the engagement lease is
not being released when the operator moves on, and nothing enforces that only
the newest holder can claim it. The `4M` on the stale two says they are aged,
not fresh, so age is already known and simply not acted on.

Related to the presence work in `892e439`, but distinct: that fixed *whether the
operator is present*, this is *which worker they are present with*.

Fixed in `f98ee29`: typing into a worker ends that device's engagement
everywhere else.

### 15. The App and API card vanishes during the reload it is reporting — *fixed*

Raised as: the app update ran, and after it compiled the module disappeared;
opening a worker showed `Runtime request returned 502`; afterwards the card
said current but still offered a reload.

Root cause: the hook feeding the card discarded its last known state whenever a
refresh failed. Activating a build restarts the API, so the one moment that
call reliably fails is the middle of the operation the operator is watching —
and with no runtime the card renders nothing at all.

Fixed: the last known runtime is kept, and the card holds its place with a
"Reconnecting…" state saying the API not answering is expected while a new
build takes over.

The 502 is the same restart, seen from another surface. `5bdd8a8` already added
502-and-family to the statuses that retry.

Also fixed: **a completed build now says it succeeded.** The card returns to
offering a reload — correctly, because newer commits exist — but it states
first that the last build completed and which revision is serving the page. The
runtime already carried a `ready` state; nothing surfaced it.

### 6. Worker engine upgrades need care proportional to their harm — *fixed*

Raised as: this is the most harmful operation and currently has the least
friction around it. Wanted:

- Check whether workers are actually working, and warn before interrupting.
- Come back with the same workers on the same sessions afterwards.

Addressed in three steps. `d8e77a1` restored the workers an upgrade unloads.
`198af91` made that restoration survive an interrupted update, which is when it
was actually failing.

Now the warning distinguishes loaded from busy. Loaded and working are
different questions and only the second costs anything: replacing the engine
while a worker rests loses nothing, while doing it mid-command kills work that
is not resumed. The card names the workers running a command, says plainly when
none are, and repeats the same sentence in the confirmation — where the
operator commits, not only where they first read it.

The count of sessions still comes from the host, which is what stops them;
which of those are busy comes from the roster, which is what knows.

### 7. App upgrade progress is close to invisible — *fixed*

The control-room indicator added in `64e2f13` shows a spinner and the revision
being built, but the operator did not find it, which means it is not doing its
job where they actually look. Item 12 records the answer the operator gave:
put the progress in the card, not only in the header.

Closed by items 11, 12 and 15 together — the header names the subsystem and
reports a run in progress, the card carries live progress where the operator
started the action, it no longer disappears mid-build, and a finished build
says so.

One piece was left: the header indicator had the same defect the card did. A
refresh during the restart returned nothing for every subsystem, and an empty
answer was treated as "nothing to update", so the indicator vanished at the
same moment the card did. It now keeps its last answer until something is
actually learned.

### 8. Takeover is visibility only — *decision drafted, needs the operator*

Engagement now names the device driving a worker (`a39a95c`), but there is no
control to take it back. Claiming engagement without sending input would be a
new input-authority path and needs an ADR, not a button.

Drafted as [ADR 0049](decisions/0049-claiming-worker-engagement-without-input.md),
status Proposed. It argues the claim should be granted immediately — there is
one operator, so a second device asking is not contention — but on a shorter
lease than typing earns, and without taking terminal geometry. Nothing is built:
this needs an operator ruling first.

### 9. A phone cannot see what a worker is carrying — *fixed*

Recorded in `docs/29`. The worker context bar is desktop-only, so a phone shows
which worker is selected and nothing about its work. Reinstating the whole bar
would return the vertical chrome the phone layout reclaimed, so this needs a
deliberate choice rather than a default.

Fixed by folding the task into the worker switcher trigger, which already
carried a small line for the Hive indicator, so no row was added. The trade is
the Hive line on the worker surface only; `docs/29` records the reasoning and
how to reverse it.

### 16. The web suite passed or failed depending on the order it ran in — *fixed*

Not operator-raised. Found while verifying something else: a run reported one
failure, three re-runs passed, and under `--sequence.shuffle` the failures moved
around. A suite that passes in one order and fails in another is not evidence
that anything works.

Three causes, all in the tests:

- Five test files rendered components and never unmounted them. Vitest runs
  without `globals`, so Testing Library never installs its own cleanup, and
  those files did not install one either — each test queried a document that
  still held the previous test's render. Cleanup now happens once in the shared
  setup, for every file.
- Three tests answered `fetch` by call order through chained
  `mockResolvedValueOnce`, which assumes the app issues one fixed sequence of
  requests. They now answer by URL.
- Two assertions pinned exact call counts on components that also poll, so a
  poll landing inside the test rather than after it failed a test about
  recovery for reasons of scheduling.

Verified by twelve consecutive shuffled runs and one single-worker shuffled run,
all 404 passing, plus the default order.

One thing this turned up that is worth keeping in mind: answering an unmodelled
endpoint with an empty object is worse than not answering it, because the
component then renders against a shape it cannot read. The test that navigates
into Settings keeps its ordered mock for that reason, and says so.

### 17. The control room never said an update was waiting — *fixed*

Raised as: Settings shows an App and API reload available, and nothing at the
top right says so for either subsystem.

Root cause: the indicator was set only as a side effect of `refreshControlRoom`,
which nothing calls on load or on a timer — only a manual refresh, a Jira sync,
or a task-board import. Settings knew because it polls; the header did not,
because it did not. Item 7's original complaint that "the operator did not find
it" was frequently the indicator simply not being there.

Fixed by giving the indicator its own polling, the same way the settings card
has, keeping the last answer when a refresh learns nothing.

### 18. The update pill is in the wrong place — *fixed*

Raised as: it belongs at the bottom in the runtime area, beside the `Runtime
0.1.0-dev-…` line, rather than in the control-room lockup at the top.

Moved. It now sits under the runtime version it is about.

One consequence to note: `.rail-footer` is hidden on the phone, so the pill no
longer appears there at all. Settings still reports the update. Worth an
operator ruling if the phone should keep a way to see it.

### 19. "Put worker to sleep" should leave the terminal bar — *fixed*

Raised as: dangerous to misclick for something that would rarely be used, and
prime real estate that could serve other purposes.

Sleep already lives in the worker-list menu, which is where an earlier operator
ruling put it — see the reversal of `d2c2eb2` recorded in `docs/29`. The
terminal bar copy is the one to remove, not the only path.

Removed, along with the `onStop` prop it was the only user of. The bar keeps
the Queen chips and the "Always active" marker, which is now gated on the
worker being unstoppable rather than on which branch of a ternary rendered.

### 20. A popped-out worker window loops adjusting the terminal — *fixed*

Raised as: popping out the worker window produces a repeating "adjusting
terminal" loop, and this is exactly what happened on mobile.

Root cause: the presence device id lives in `localStorage`, which every window
and tab of one browser shares. A popped-out window and the window it came from
are therefore **one device** as far as the server is concerned, and
`claim_unowned_worker_geometry` grants geometry when the row is unowned *or
already owned by this device* — so both windows pass the ownership check and
both resizes are applied.

The loop then runs through the snapshot path: any resize produces a canonical
snapshot, each window restores at the other's size, re-fits to its own viewport,
and resizes back.

[ADR 0045](decisions/0045-engaged-device-terminal-geometry.md) is not wrong, it
is too coarse: it reasons about devices, and two windows on one machine are two
viewers of one device.

Fixed without changing the protocol, by keying the client on
`document.hasFocus()` rather than `visibilityState`. Exactly one window has
focus browser-wide, whereas a pop-out and its opener are both visible. An
unfocused window no longer re-fits after a snapshot and no longer claims
geometry, so it accepts the canonical size instead of arguing with it.

Left open: the server still cannot distinguish two viewers of one device, so
this is enforced by the client's good behaviour rather than by the rule. A
per-viewer geometry identity would make it structural, and belongs in an
amendment to ADR 0045.

### 21. A provider update is installed but not running until each worker restarts — *fixed*

Raised as: Claude shows "Update installed · Restart to update" in its own
terminal — confirm whether that means workers need restarting, and if so add it
to the worker updater and show the option in the runtime area.

**Confirmed, and worse than the banner suggests.** Measured on this machine at
2026-08-19 20:50 UTC. Claude installs each release as its own file and moves a
symlink:

    versions/2.1.233   Aug 14 20:41
    versions/2.1.234   Aug 17 20:25
    versions/2.1.235   Aug 18 20:40
    versions/2.1.236   Aug 19 20:12   <- what `claude` points at now

Two worker provider processes were running, started Aug 17 02:37 and Aug 18
19:46. The newest release existing at those moments was 2.1.233 and 2.1.234
respectively. Both were therefore two to three releases behind, and nothing in
Swarm said so.

`/proc/<pid>/exe` is not readable here, so the running version was inferred from
start time against install time rather than read from the process. That
inference is the same one the fix uses, and it is conservative: it can say a
worker predates the installed release, not which release it holds.

Fixed. The terminal host resolves each provider executable, because it is the
process that spawns them and its `PATH` decides which release a worker gets, and
reports the resolved path, version, and install time. The API compares that
install time against each live session's start time. The runtime area gains a
Providers card naming the release, how many workers are behind, and a restart
that stops and revives exactly those workers through the same durable
revival-intent path the engine update uses.

## Landed

- The unconfirmed-delivery mark now explains itself where the operator lands.
  Opening the worker states what it means and that the briefing is in the
  terminal below. Nothing is retried automatically: a briefing delivered twice
  is worse than one the operator was told about, so the next step is to read the
  terminal rather than press a button.
- The Inbox chooser can be refreshed without closing and reopening the flow.

- Worker state now reads as a scale: green is work happening, amber is work
  waiting on the operator, red is work that cannot proceed, with neutrals for a
  worker doing nothing wrong and the accent for the worker the operator holds.
  Only the live state moves, and sleeping is separated from resting by fill
  rather than hue so the distinction does not depend on colour.
- The terminal header no longer names a second task. The eyebrow was resolving
  work by session while the context chip resolved it by worker, so one bar
  carried two disagreeing answers.

- Presence no longer flips to Away when a phone changes apps, and a locked
  desktop that stops reporting stops being described as locked.
- Work waiting on an operator answer is no longer reported as stale (`f224968`).
- Touch scrolling follows the finger (`8674023`).
- Workers return after a worker-engine upgrade (`d8e77a1`).
