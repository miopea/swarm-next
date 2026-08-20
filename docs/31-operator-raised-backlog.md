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

### 22. Worker revival deadlocked the API against itself — *fixed*

Not operator-raised as a feature request; raised as "we are stuck at the login
screen and I get a 524".

Sequence, 2026-08-19:

- **21:15** App/API deployed at `65e5db0`. That commit changed `swarm-terminal`,
  so the expected worker-engine build id changed with it.
- **21:47** `swarm-next-host-reconcile` saw the mismatch and applied the worker
  engine update **on its own**, stopping all seven workers. Nobody asked it to;
  a timer acted on a build id that a deploy had moved.
- The revival intents from item 10 were recorded correctly — six workers owed a
  return.
- Revival then deadlocked. `start_worker_process` takes `worker_lifecycle`, the
  mutex is not reentrant, and three callers held it and asked for it again: the
  supervisor's revival pass, the provider restart endpoint, and the worker
  engine update. The first revival waited forever for a lock it already held.
- Everything needing the lifecycle queued behind it. `/health` answered in
  1.6ms while `/api/v1/workers` never returned, so the proxy answered 524 and
  the login screen never cleared.
- **22:24** Fixed in `71c3ad5` and deployed. The supervisor checks the lock and
  releases it immediately, since starting a worker takes it anyway; the other
  two revive after unlocking.

Two things worth carrying forward.

**The bug shipped hours before it fired.** The revival path landed in `198af91`
and only runs when a worker is owed a return, which first happened at 21:47.
Deployed and exercised are different states.

**Deploying App/API is not isolated from workers.** The card says workers stay
online, and they do — until a build id change makes the reconcile timer restart
the engine underneath them.

Cost: the six workers were not revived. The fifteen-minute intent age-out from
item 10 discarded them while the deadlock blocked revival for thirty-seven
minutes, so they had to be started by hand. The age-out cannot tell "no longer
wanted" from "revival has been broken for a while", which is worth revisiting.

Covered by a test that fails against the bug: with the guard held, supervising
an owed worker does not return within twenty seconds.

### 23. The phone worker picker raised the keyboard over itself — *fixed*

Raised as: on mobile, opening the worker picker focuses the text field, so the
keyboard comes up; remove the text field filter on mobile completely.

The picker opened with focus on a search input, so asking to see the roster
covered the roster with a keyboard. Every worker was one scroll away and the
operator had to dismiss a keyboard to reach it.

Removed, along with everything that existed only to serve it: the query state,
the filtered lists, and the two empty states that explained a search returning
nothing. Focus now falls to the first control in the dialog, which is not a text
field. Narrowing the list is the Awake/All toggle's job, which needs no typing.

The desktop rail keeps its own filter, and the shared roster-matching helpers
are still used there.

### 24. Buzzing and resting were the same colour — *fixed*

Raised as: "buzzing and resting look almost alike, it is impossible to notice a
difference when scanning."

Measured rather than eyeballed: the two badge colours sat **14.6 dE apart in
light and 13.3 in dark**, on uppercase text at `.58rem` inside a 12% tint.
Above the threshold where two colours are formally identical, and far below what
a scan can separate at that size. Buzzing was borrowing `--good`, a soft success
tone that happens to sit right next to the resting grey.

Note for next time: WCAG contrast was the wrong instrument here and nearly led
to the wrong colour. It measures lightness only, so it cannot tell green from
grey — a darker green scored *worse* against the grey while being obviously
different. Perceptual distance is the measure that matches the complaint.

Fixed with a dedicated `--busy` (37 dE light, 51 dE dark) and, so the difference
does not rest on hue at all, a filled badge for buzzing against the flat tint
everything else uses — the same reasoning that made sleeping a hollow dot.

### 25. Tasks offered to wake a worker that was already working — *fixed*

Raised as: "why would it say to wake a worker that is already working."

Running-ness was judged from `task.assigned_session_id` — the session the task
was assigned to. A worker gets a **new session every time it restarts**, so
after the engine update at 21:47 that lookup found nothing for every previously
assigned task, and each one offered to wake a worker that was running at that
moment.

It asks the worker now, not the session the task remembers.

### 29. The reload card named the same revision on both sides — *fixed*

Raised as: "this makes no sense, they are all the same build, and I still see
the App and API update pill." The card read `Revision ed715fe is active. Build
and switch the browser and API to working-copy revision ed715fe.`

Confirmed from the live runtime: both revisions were `ed715fe3c3f3` and
`source_dirty` was true. The working copy differed by **uncommitted changes** —
which no revision comparison can show — and the card never mentioned them, so it
named one commit twice and offered a reload for no visible reason.

Fixed in two places. The card says the working copy has uncommitted changes on
top of the running revision, which is the actual reason to rebuild. The header
indicator stops reporting that case at all: uncommitted changes at the same
revision are work in progress, not an update waiting, and an indicator that
cannot go quiet while anyone is editing the checkout is not an indicator.

### 31. Only the most severe update got a pill — *fixed*

Raised as: make sure the pill shows down with the others when there are updates.

The indicator ranked everything into a single summary and showed only the top
one, so a provider update stayed invisible behind a worker engine update until
that was dealt with, and an App and API release behind either. The three are
independent and can all be true at once.

One pill per subsystem now, ordered by what each costs: the worker engine takes
workers away, a provider update is installed and running nowhere until each
worker restarts, and an App and API release leaves workers online throughout.
That mirrors the settings page, which has a card for each.

Two older tests asserted the ranking — "work in progress outranks work waiting",
"a stopped build outranks everything". That ranking only existed because a
single pill had to choose. They now assert the stronger property: neither hides
the other.

### 30. The runtime pill said nothing about a provider update — *fixed*

Raised as: a Claude update notification arrived, the settings page detected it
properly, and the header pill said nothing — there should be one for a
Claude/Codex update in that same area.

This was item 21's detection working on real data for the first time: the card
correctly reported Claude 2.1.237 installed with 8 running workers started
before it. Only the pill was missing.

Added, ranked below a worker engine update and above App and API. All three ask
for a restart, but replacing the engine also restarts the providers, and unlike
an App and API release a provider update is installed and running **nowhere**
until each worker restarts.

### 39. Queen's assessment is long and never says what is being decided — *fixed*

Raised as: replying with anything is better, but her assessment on the Needs-you
page is still way too long and gives no concise analysis of what is being
decided.

Measured on the live inbox — the three most recent requests carried **4,857,
5,500 and 5,409 characters** across title, reason, risk and evidence. Each of
those fields is capped at ten thousand characters, and a cap that generous lets
the argument stand in for the ask. There was no field for the ask at all: the
reason was doing double duty as both.

Fixed by adding one, required and bounded to 400 characters: what the operator
is deciding and what turns on it. The card leads with it and folds the reason
behind "Why, and what it rests on", so the argument is present without being in
the way.

Requiring it is deliberate. An optional field would go unused by exactly the
askers who most need it, and the error says what is wanted rather than that
something is missing. The tool schema now also describes the interview
questions array from item `01a016be`, which was accepted by the API but never
advertised.

### 37. "With you" and "awaiting you" were the same colour — *fixed*

Raised as: "with you needs another colour? Maybe a purple? Right now it is the
same as waiting."

Measured: **12.5 dE apart in light and 5.0 in dark**. The dark case is close to
no difference at all. Both were amber — `with_operator` borrowed
`--accent-strong`, which sits beside the `--warn` that means a worker has
stopped and is waiting on the operator. Two states that mean opposite things.

Fixed with the suggested purple, giving its own token rather than borrowing
one: 123 dE in light and 100 in dark, and well clear of the busy green and the
resting grey too.

### 38. A delivered prompt sits unsent while the provider is working — *fixed*

Raised as: the prompt is still sometimes not hitting enter and sits there until
pressed manually.

Read from the live log rather than reproduced. At 02:47:38 a delivery was
abandoned with `marker_is_visible=true` and `new_claude_paste_is_visible=true` —
the message had demonstrably rendered, and Enter was withheld anyway.

The confirmation required the marker to be visible **and the terminal's output
sequence to stop advancing for 750ms**. That second condition asks a different
question: it asks the provider to be idle. A provider that is thinking streams
output continuously, so the sequence never settles, the delivery is never
confirmed, and the message is left in the prompt — and a busy worker is exactly
when automation delivers.

Stability now measures how long **the delivery itself** has been continuously
visible, which is the thing actually being waited for. The sequence-stability
rule stays where it belongs: the check after submission, which genuinely does
ask whether the provider came to rest.

### 36. A Needs-you request is an unreadable wall, and its options were not the answer — *fixed*

Raised as: "this needs you is ridiculously long, block of text, impossible to
read" and "the options are terrible — I want to tell SS to add it to the play
store itself via the browser extension and that isn't an option."

**The wall was self-inflicted.** The request in question carried 16 line breaks
in its reason and 23 in its evidence: the worker wrote it with paragraphs and
headings, and the card rendered it as one continuous run, throwing all of that
away. Prose is now shown as written, and a long block folds until asked for
rather than being truncated — the operator is deciding something, so nothing
written for them is thrown away.

**The options being wrong is the exact failure `01a016be` exists for.** The
asker collapses an open question into guesses before the operator has said
anything, and when the guess is wrong the only ways out are pressing the closest
button or dismissing — both of which lose the answer. That spec's ruling was
that the operator should not be limited to guesses, and it applies to a ruling
just as much as to an interview.

So any pending request can now be answered in the operator's own words. It is
recorded under one reserved key, carries the same `answered` action, and reaches
the worker through the delivery an interview already uses — one answer shape,
one audit trail, one format, rather than a second of each.

### 35. Deployment evidence is the operator's chore, and a task claims COMPLETED without it — *fixed*

Raised as: "I shouldn't be managing this — that is part of marking something
complete. It should [not] be listed as complete if it wasn't verified."

Today a task reaches `completed` on a transition alone. Deployment evidence is a
separate form the operator fills in afterwards, and only then does a reply
become available. So the board shows **COMPLETED** for work nobody has shown to
be live, and the one person who cannot check it is the one being asked to.

This is the same distinction the shipping vocabulary already draws elsewhere in
this repo: committed, pushed, and deployed are not synonyms, and a completion
that has not been verified is a claim rather than a fact. The task lifecycle
does not currently make that distinction, and the panel's own wording admits it
— "deployment evidence prevents a completion status from emailing someone before
the change is actually available" — which is exactly right about email and
silent about the status shown on the board.

Two candidate shapes, and this needs the operator's ruling because it changes
the lifecycle:

- The worker records the evidence as part of finishing, since it is the actor
  that deployed and the only one that knows the reference. Completion without it
  is refused, the way the reply already is.
- Completion stays as it is, but a task without evidence does not read as
  COMPLETED — it reads as finished-and-unverified until evidence exists.

The first removes the chore; the second removes the false claim. They are not
exclusive, and the second is the one that makes the board honest.

**Operator ruling, 2026-08-20: both, in that order.** The chore went first —
`swarm_record_deployment` means the worker records it, since it deployed the
work and holds the reference. The false claim went second: completion still
works, and a completed task with no deployment record reads as
**Finished · unverified** rather than Completed.

Deliberately not a gate. Plenty of work has nothing to deploy — a research
task, a document, an interview — and refusing completion for those would put a
meaningless question in front of every task. The board says what is known
instead of blocking what is not.

Related and already built: item 32's card now reports a completed email task
nobody answered. The same reasoning applies one step earlier, to whether it
should have been called complete at all.

### 34. Swarm Next has a developer update path and no user one

Raised as: the Python Swarm had a "dev" mode for local work with fast hot
updates, and a normal user mode that polled GitHub for updates or checked on
demand. Swarm Next needs the equivalent before anyone else can use it.

What exists today, confirmed by reading the surface rather than assuming:

- **Developer mode is real and complete.** `swarm-next-package` carries
  `enable-development CHECKOUT`, `disable-development`, and
  `reload-development`, and `development.enabled` in the runtime response is
  literally "is a development reload path configured". That is the mode being
  used to build Swarm Next in Swarm Next.
- **Applying a release already exists.** `swarm-next-package install|update
  RELEASE_DIR` installs a prepared release directory, and that directory
  carries `SHA256SUMS`, `VERSION`, `PROTOCOL`, and `SOURCE_REVISION`, so
  integrity and compatibility checking are already part of the release format.
- **Nothing discovers a release.** There is no reference to GitHub, a release
  channel, or an update check anywhere in the Rust code. A user with no
  checkout has no way to learn a new version exists, and no way to fetch one.

So the missing half is discovery and fetch, not application. That is a smaller
gap than it first appears, and a more sensitive one: it decides where a release
comes from, how its provenance is verified before it is trusted, whether checks
happen on a schedule or only when asked, and what the operator consents to.
Those are decisions rather than work, so this needs an ADR before it needs code.

Worth carrying into that ADR from today's evidence: an App and API deploy
changed the worker-engine build id and a reconcile timer then restarted the
engine on its own thirty minutes later (item 22). Whatever the user-mode
updater does, it inherits that lesson — a release is not isolated from the
workers just because the card says workers stay online.

### 32. A worker can finish an email task without anyone answering the email — *partly fixed*

Raised as: "email workflows are not defined. I had D365 work on an email task
and it closed the email without drafting a response in my inbox to the ticket."

Two findings, both confirmed by reading the surface rather than guessing.

**A worker has no way to reply.** The MCP tool list carries `swarm_comment_jira_task`
for Jira and nothing at all for email. A worker handed an email-sourced task
cannot draft a reply, and nothing in the briefing tells it a person is waiting
on one.

**Nothing notices the silence.** The reply workflow in `EmailResolutionPanel` is
complete and careful — record deployment, then draft, review, and send, with
idempotent per-thread delivery — but every step is the operator's, reachable
only by expanding email details on a completed task. No coordination attention
detects a completed email task whose thread was never answered, so a finished
task simply goes quiet and the person who wrote in hears nothing.

What should happen is a product decision and is recorded here rather than
guessed: whether the worker should draft the reply as part of finishing, whether
completion should be blocked until a reply exists, or whether this stays an
operator step that is merely surfaced. The third is the only one that can be
built without deciding the other two, and it is worth building either way.

**Built: Swarm now reports the silence.** A completed email task with no
delivered reply appears in Needs you, naming the person still waiting and
whether a reply was written but never sent — drafting is not answering. It does
not send anything; the reply is still written and reviewed on the task, where
the thread and the deployment evidence are.

**Operator ruling, 2026-08-20:** *"When Tim sends via email, I shouldn't have to
go digging through tasks to close it out. And the system made me generate a
reply instead of doing it automatically. That whole process should be part of
the agent working on it."*

So the reply belongs to the worker, as part of doing the work — not to the
operator afterwards. That settles both questions this item left open, and it
settles item 35 the same way: the agent that did the work owns the evidence and
the answer, because it is the only actor that has them.

**Built.** A worker now holds both halves of finishing an email task:

- `swarm_record_deployment` records where the work is running. The worker
  deployed it and holds the reference; the operator cannot verify it for them,
  and until it exists the board shows a completion nobody has shown to be live.
  This also answers item 35's chore half.
- `swarm_draft_email_reply` writes the reply the requester will receive.
- The briefing for an email-sourced task now names who wrote in, says they are
  waiting, and says finishing includes both steps. A worker that is never told
  cannot know to answer, so the tools alone would have gone unused.

**Sending stays the operator's.** It is an external effect, and the objection
was to *writing* the reply, not to approving it — the existing review-and-send
panel is the good part of that flow. Drafting requires the task completed and
its deployment recorded, which is the order that stops a reply going out before
the change is available.

Not addressed: whether completion should be refused without a deployment
record. Item 35's second half — a task reading COMPLETED when nothing has shown
it to be live — is still open.

### 33. The completed email task's panels overlap and run off the card — *fixed*

Raised in the same report: opening a completed email task shows the Original
report panel and the Step 1 of 2 deployment form overlapping each other and
clipped at the right edge of the card.

Fixed in `d60b6ed`. Both panels are wrapped in a plain div so they can be
hidden without unmounting, and that wrapper is the card's grid item — the
full-width span was declared on the panel inside it, where it does nothing, so
the wrapper was auto-placed into a named column and drawn over the assignment
cell.

### 26. An imported email task woke its worker and then nothing happened — *fixed*

Raised as: an email task was imported for a sleeping worker; the wake worked and
the worker came up, but the terminal "just sits blank" and the task shows
`Briefing waits for a quiet moment`. Several minutes passed with no change.

Diagnosed from the record. The dispatch had `attempts = 0` — never claimed, so
never attempted, so no error to show. The claim required the task to be `ready`,
and the activity trail shows why it was not:

    00:32:29  imported from email, draft -> ready, assigned
    00:32:49  reassigned by the operator
    00:32:50  ready -> active by the operator

Delivery runs on a thirty-second timer. The task was started twenty-one seconds
after it was assigned, inside that interval, so the briefing missed its only
window and became permanently unclaimable — no retry, no timeout, no error, and
a woken worker sitting at a blank prompt.

Fixed: a briefing is owed for work that has already started, not only for work
still waiting. Other active work still holds a briefing back, since that is the
rule stopping a busy worker being handed more, but the task being briefed no
longer counts against itself, and queue order no longer applies to work already
under way.

### 27. Pop-out duplicates the whole window instead of detaching one thing — *fixed*

Raised as three parts:

- The pop-out control belongs in the board area rather than where it sits now.
- A popped-out window should show **only the thing that was popped out**, not a
  second copy of the whole app.
**Third part fixed** on the same answer: the control now sits beside the
heading of the surface it detaches, rather than among the header's global
controls. What it pops out is the thing named right there.

- The item that was popped out should then be greyed out in the main window, or
  clicking it should focus the window that already holds it. Two copies of one
  surface is the outcome to avoid.

Fixed. A detached window drops the navigation rail and the pop-out control, so
it shows the one surface it was opened for. The opener remembers what it has
detached: that surface reads as spoken for in the rail, and choosing it brings
the existing window forward instead of drawing a second copy. A window the
operator closed is forgotten on the way past, because browsers report closure
only when asked and never announce it.

One thing this turned up: `?surface=` already meant "open Swarm here", which is
what a notification deep link carries. Reusing it for detaching would have
opened notifications into a window with no navigation and no way out. An
existing test caught it. Detaching now carries its own flag.

The control's placement was answered by the operator on 2026-08-20 and is
recorded above.

### 28. Queen automation reports a stuck review, and nothing moves it — *fixed*

Raised as: "why isn't the Queen on a cron or polling cycle? When I go in, she is
sitting idle but the pill badge at the top says the same thing and links to
settings. Then on Needs you there is this, which is useless right now."

The state observed: `Review needs attention — Delivery was interrupted before
Swarm could confirm completion. Retry resumes this same review after you check
Queen's terminal.` It appears in three places at once — the Queen terminal
header, the Settings automation panel, and a Needs-you card — and none of them
move it forward on their own. Meanwhile Queen sits idle rather than running on a
cycle, and the counters read `17` stale/exited cases with `0` needing judgment.

Three distinct complaints inside one report: automation does not resume on its
own; the same message is repeated in three surfaces without any of them being
the place to act; and the Needs-you card offers no action that resolves it.

**First one fixed.** Read from the live record: the run went uncertain at
22:59:32 and was still parked ninety minutes later. The session it was delivered
to — `01a01bff` — had never ended, so the resume-on-ended-session rule correctly
did not fire, and nothing else could move it.

Uncertain means Swarm could not *confirm* the review reached Queen, not that it
failed. The existing rule's own reasoning said an ended terminal "cannot be read
from", which implies a live one can. The prompt carries the run id, so Swarm now
reads that exact terminal and, finding the marker, records the delivery as
landed: Queen has it and can finish the run herself.

It only ever resolves uncertainty toward "it landed". An absent marker proves
nothing — it may have scrolled out — and replaying a review that did arrive
would double it, which is what the uncertain state exists to prevent.

**Second part fixed.** The Needs-you card now offers "Resume review" when a
review is waiting to be resumed. Opening Queen stays the first action, because
the message asks the operator to check her terminal before resuming, but the
resuming itself no longer lives two screens away in Settings. It is offered only
for `uncertain`: a review blocked on an operator decision is resolved by
answering it, not by running it again.

**Third part fixed**, on the operator's answer of 2026-08-20: keep all three
surfaces, word each differently. They were sharing one sentence, and each is
asking something different — "what is true of the terminal I am looking at",
"how does this work and what can I change", "what wants me and what do I do".
One sentence cannot answer three questions, so it answered none of them well.

### 40. One settings test failed under load and passed alone — *fixed*

Not operator-raised. Found while verifying something else: it failed twice in
full runs and passed four times in isolation, then again later.

That test drives ten unrelated things across a hundred and sixty lines —
navigation, presence policy, lock detection, Queen policy, resources, saved
reports, the theme — with **seven separate waits**, each on the default
one-second budget. Every one of them is a chance to exceed it while the whole
suite competes for CPU, which is exactly the pattern that produced "fails
together, passes alone".

The waits are given room, because they assert behaviour rather than speed —
the same reasoning applied to the lazy settings surface in `4a42df9`. The size
is the underlying reason there were so many chances to trip, and that is
recorded in the test rather than fixed: splitting a test somebody else wrote,
while chasing a timing failure, is two changes at once.

Verified by five full runs and three shuffled runs, all 451 passing.

### 41. Requiring a new field broke every already-connected worker — *fixed*

Not operator-raised. Found by attempting the end-to-end demonstration spec
section 4 requires for `01a016be`, which is exactly what that demonstration
exists to catch: every unit test passed, because they build the input struct
directly and never through a client holding an older schema.

Filing a decision through MCP returned **"this agent is not authorized for that
outcome"**. Authority was never in question — the same refusal came back with
the `task_id` removed, which skips the only authority check on that path.

Two defects, one inside the other.

**`parse` reported every unreadable argument as an authorisation failure.** A
caller told it lacks authority does not retry with a corrected payload; it
escalates or gives up. This cost a long investigation into permissions that
were never involved. It now says what could not be read and points at the
schema.

**Requiring `summary` made the decision path unusable for running workers.**
Added in `d9dfe10` with no serde default, so a client that connected before the
field existed — holding a schema without it, and `additionalProperties: false`
— strips the field, and deserialization fails. Every already-running worker was
unable to file *any* decision until its MCP session reconnected. That was live
across the fleet from the moment `d9dfe10` deployed.

The field is now tolerated at deserialization and still refused by the store,
which already had a message saying what is wanted. Required for clients that
can see it, actionable for clients that cannot.

Worth carrying forward: a worker's tool schema is fixed when its MCP session
connects. Any change that makes a field required is a breaking change for every
worker already running, and no test that constructs the input struct directly
can see it.

### 42. An unconfirmed briefing marked a worker forever — *fixed*

Raised as: Public Website shows the unconfirmed-briefing mark and has been that
way for ages, with no open tasks; Sculpt Studio the same, showing it in the
header, with nothing in Needs you.

Six unconfirmed briefings were outstanding, the oldest **eight days old**. Their
tasks had since been completed, blocked, or sent to review — one belonged to a
worker with no open work at all.

"Swarm could not confirm this briefing landed" is a question about work still
waiting to be done. Once the task has left ready or active, the question is
answered or moot, and the mark is asking the operator to go and check a terminal
about something already finished. Nothing cleared them, so the mark was
permanent.

They are now forgotten once their work moves on. The mark still means what it
says for work still waiting.

### 43. Swarm Next needs a versioning system before others can use it

Raised as: "we need to introduce a versioning system as we get close to opening
Swarm Next to others."

Today every build is `0.1.0-dev-<sha>-<timestamp>-<pid>` — a development
identity, unique per build, with no ordering anyone outside this machine could
use. Nothing distinguishes a release from a rebuild of the same commit.

Related to item 34 and probably decided with it: a user-mode updater needs
something to compare, so "which version is newer" has to have an answer before
"is there an update" can.

### 44. The product is now called Swarm

Raised as: "At this point we can change it to just Swarm. I am going to update
Swarm Legacy to start referring to it as that now too."

So the naming becomes **Swarm** for this product and **Swarm Legacy** for the
Python one, and the operator is making the matching change on that side.

Worth separating when this is picked up, because the risk is not evenly spread:

- **User-visible naming** — window title, control-room lockup, page copy,
  documentation. Cheap and safe.
- **On-disk and service identifiers** — `~/.local/state/swarm-next`, the
  systemd unit names, the release directory, the MCP server key `swarm-next`,
  crate names. Renaming these is a migration with real failure modes and no
  user-visible benefit, and the two Swarms have to coexist on this machine
  while Legacy is still running.

The first is the rename the operator asked for. The second is a separate
decision that should not ride along silently.

### 45. Diagnostics does not read as a quick scan

Raised as: "the diagnostics page isn't terribly the quick scan."

Fourteen rows of equal visual weight, each a label and a value, with nothing
saying which of them the operator should look at. The page's own heading is
"Know which layer needs attention", and it does not answer that.

### 46. Diagnostics judges layers without showing the machine — *fixed*

Raised as: Critical on Loaded worker runtimes makes no sense on this machine;
and separately, "the diag doesn't show the machine status — a machine with 32g
of ram is very different than one with 8. Same with CPU load."

Both are the same defect. Layer verdicts came from a fixed ceiling —
`RESOURCE_CRITICAL_BYTES` of 512 MiB — applied to whatever was being measured.
That was sized for one process and was being applied to the total of every
loaded worker, where a single Claude process is already larger than the whole
allowance. Ten healthy workers holding 6.0 GiB were therefore reported Critical
on a machine using 45% of 31.3 GiB, while the kernel reported **0.0% memory
stall**.

The machine's own verdict was already being computed from PSI, correctly, and
simply never shown.

Fixed: a layer is judged as a share of the machine it is running on, and only
named when the machine is actually under pressure. When nothing needs
attention, the answer is nothing.

The thresholds are 15% of the machine for advisory and 25% for critical, and
neither fires while the machine itself reports Normal. The absolute byte
constants are deleted rather than left unused, and the `policy` field of
`/api/v1/runtime/resources` now reports the percentages it actually applies
instead of byte ceilings it no longer consults.

Diagnostics also states the machine once, above the rows: total memory,
processor count, and whether it is under pressure. Every memory figure below is
read against that number, and the page was not printing it. Compute load now
carries a verdict too, from the kernel's CPU stall reporting where it exists and
from load-per-processor where it does not — a load of four is idle on forty
processors and saturated on four.

### 47. A worker had no way to record work it found — *fixed*

Raised as: "the Swarm Next MCP has no way for workers to create tasks. Is that
by design? I just asked scout to do a couple for architecture and it got
confused because there is no tool to do it."

It was by design, and the design was wrong. `swarm_create_task` existed but was
gated `require_queen` and only listed for Queen, so a worker asked to file
follow-up work found no tool, could not say why, and the work was lost.

The gate was defending the wrong thing. What must stay Queen's is **routing** —
deciding who works on what and when. Recording that work exists is not routing.
A created task lands as an unassigned `draft`: it is not queued, not claimable,
and cannot be worked until someone readies and assigns it.

Fixed: any worker may create a draft task, in its own repository or another —
the cross-repository case is the one that prompted this. `swarm_assign_task`
stays Queen-only, and the boundary is now asserted as such rather than as a
blanket denial.

**A connected worker will not see the new tool until its session reconnects.**
A worker's MCP tool schema is fixed when the session connects, so Scout keeps
the old list until it is restarted.

### 48. The decision list reflows under the operator — *fixed, with a second door still open*

Continues task `01a016d2`. `9668d65` held scroll and focus still; the list
itself was still moving.

The server orders decisions `created_at DESC`, newest first. A decision
arriving during an ordinary poll is therefore inserted **above** the cards
already on screen and shoves every one of them down by a whole card — between
the operator reading an action and pressing it. On a busy Hive that is not an
edge case. Shown failing first: with one card on screen and one arriving, the
arrival took the top slot.

Fixed: a pending card the operator can already see keeps its place, and
arrivals land at the bottom and announce themselves through the count.
Resolved history keeps the server's order, which is the useful one there.

**Still open, same failure through a different door.** Three attention cards
sit above the decision list in `web/src/App.tsx:1367` — unanswered email, Queen
automation, and Apiary assistance. Each appears and disappears on live state,
and Queen's status changes on its own cycle. Any of them mounting pushes the
entire decision list down exactly as an inserted card did.

Not fixed here because the fix is a layout decision rather than a bug fix:
either the cards move below the list, which reads oddly against the "nothing
needs your attention" empty state, or the list gets an anchored position. That
is the operator's call on how the page should read, not mine to take silently.

### 49. "Adjusting terminal layout…" covers the terminal for reasons that are not layout — *fixed*

Raised as: "This randomly pops up", with a screenshot taken while a build was
emitting heavily.

Nothing about the layout was being adjusted. The cover is driven by
`data-terminal-restoring`, set on every canonical snapshot restore, and there
are three causes: first attachment, the renderer falling more than 3 MiB behind
its bounded queue, and a gap in the output sequence. Only the first is a layout
fit. The other two are the screen being rebuilt, and both were describing
themselves as a layout adjustment — which is exactly why it read as arriving
for no reason.

The likely cause in the screenshot is the second: a `cargo test --workspace` or
`pnpm vitest run` passes 3 MiB of output easily, the client discards its state
and asks for a fresh snapshot, and the operator gets a full-surface cover
saying something that does not match anything they did.

Fixed: the reason travels with the snapshot, so the cover names it — "Adjusting
terminal layout…", "Catching up after a burst of output…", or "Rebuilding the
screen after dropped output…". The cover itself stays, because the screen is
genuinely blank between reset and rewrite and a blank flash reads worse.

Deliberately not changed: `MAX_PENDING_RENDER_BYTES` is still 3 MiB. Raising it
trades memory for fewer rebuilds and is a number that should be set against a
measurement, not a guess. Now that each rebuild names its own cause, the next
occurrence says whether backpressure is the one worth tuning.

**Second report, and a different defect underneath.** "I got adjusting terminal
again right now, until I did a refresh." The word that matters is *until*: the
cover was not flashing, it was stuck.

`#finishRestore` — which removes the cover — was reachable from exactly two
places: `fit()`'s `finally` block, and `restore()`'s error path. Nothing removed
it on the success path. The cover therefore came down only if a `fit()` happened
to follow.

One does not always follow. `TerminalController.onSnapshot` returns early,
skipping the re-fit, when the window is not focused — deliberately, so two
viewers of one PTY stop arguing over its size forever. That early return
silently took the cover-removal with it, and the cover then stayed until the
page was reloaded. Shown failing first: a completed restore with no fit after it
left `data-terminal-restoring` set.

Fixed: `restore()` uncovers itself once the snapshot bytes are on screen. What
the cover hides is the blank between reset and rewrite, and after that write
there is no blank left to hide. The repaint that guards Chromium's blank-canvas
behaviour now runs with the rebuild instead of riding on a fit that may never
come.

### 50. One busy terminal held up every other worker's delivery — *fixed*

Raised as: "The Queen was busy so I'm not sure if this was on purpose or not
but the prompt is sitting waiting to be sent on a decision that was made — it
should be able to handle multiple of these at one time."

Measured rather than guessed. Every coordination message is submitted by
`submit_coordination_message`, which can spend up to ten seconds waiting for
the terminal to settle and up to ten more waiting for it to accept — twenty
seconds worst case per message. All three delivery loops — decision outcomes,
task briefings, task outcomes — iterated **strictly sequentially**, and the
whole cycle runs under one Hive-wide `coordination_delivery` mutex.

The live database shows eight distinct sessions with delivery records. A
message at the back of that queue waits behind terminals it has nothing to do
with, and Queen streaming output is exactly the case where the front of the
queue burns its full budget.

**It does not merely delay — it manufactures the (!) mark.** The live database
holds five `uncertain` decision deliveries. `uncertain` is what a message
becomes when its ten-second acceptance window expires, and a message that
waited minutes to start is far likelier to meet a terminal that is still busy.
That is the same "Swarm wrote a briefing to this worker and could not confirm
it landed" mark reported twice before, and this is one of its sources.

Fixed: distinct terminals now proceed at the same time, and each terminal's own
messages stay in order. The grouping is the correctness boundary, not the
concurrency — two messages for one worker share a single input line and
interleaving them would produce a prompt made of both; two for different
workers never touch. Both properties are pinned by tests.

The Hive-wide mutex stays. It serialises *cycles*, not messages within one, and
it is what stops two overlapping cycles claiming the same delivery twice.

### 51. Mobile: a frozen roster, and a picker whose footer was unreachable — *two fixed, two open*

Seven emails, five with content. Taken together they are mostly one defect.

**Frozen roster (fixed).** "It doesn't seem like the status of the workers are
updating because scout is busy doing stuff and it's showing resting still. I've
kicked all the workers in different ways and they're all still sitting in a
resting state." With, from the day before, "there's no refresh button on mobile
to force it to redraw" and "completely unusable on mobile right now" after
locking the machine and walking away.

Not a misclassification. The live feed is a long poll: the server holds an
unanswered request for up to twenty seconds, and the client loops. A phone
suspends a backgrounded tab, and the fetch it left in flight can never settle —
it neither answers nor fails. The loop then waited on that promise forever,
while the last state it published was `connected`. So the roster showed work
frozen exactly where it stood, the pill claimed a healthy connection, and
nothing the operator did moved it. Shown failing first.

Fixed twice over: a poll is abandoned at a 35-second ceiling and retried, which
is well clear of the server's twenty; and the feed reconnects the moment the
page becomes visible, so returning to the tab is immediate rather than a wait
for that ceiling.

**Picker footer cut off (fixed).** "Scrolling doesn't seem to work on the picker
modal. The bottom 'Manage Workers' is cut off and I can't scroll up to see the
button." The dialog has four children and its template declared five rows, left
over from an earlier layout. The roster list therefore landed in an `auto`
track, which sizes to content and never shrinks, so a long roster grew past the
dialog's `max-height` and pushed the footer out of a box that clips overflow.
The list's own `overflow-y: auto` never engaged, because the row had given it
every pixel it asked for. Shown failing first.

**Still open — needs the operator's eye, not a guess.**

- "On mobile, the selector gets bumped over by the open pill. Needs a
  redesign." The operator says redesign, and a redesign is a layout decision
  rather than a defect to fix silently. The two fixes above may change what
  this looks like, so it is worth re-reading on a phone first.
- A manual refresh control on mobile. Asked for explicitly. The reconnect above
  removes the case that made it necessary, so the question is now whether it is
  still wanted as an escape hatch or was only ever a workaround for the freeze.

## Landed

Earlier items, kept as a record rather than a queue.

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
