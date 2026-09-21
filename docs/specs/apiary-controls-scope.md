# Apiary controls: settings, watching, and takeover

Status: Accepted direction from the operator interview of 2026-09-21. Product
scope only; nothing implemented, and one part of it SUPERSEDES accepted ADRs.

Three capabilities were asked for together and are scoped together because they
share one authority model: what a Keeper or Steward may see and do inside a
member Hive.

## ⚠️ THIS SUPERSEDES THREE ACCEPTED DOCUMENTS. Read this first.

The operator's decision is that Keeper gets "literally everything, basically a
window into that hive, either watching or being able to do a takeover".

That contradicts, and overrides, the following as they currently stand:

- **ADR 0034** bounds even a Steward to four counts, active Jira claims and a
  timestamp, on the stated grounds that copying remote worker or terminal state
  into Keeper "would violate the one-operator Hive boundary".
- **Doc 21** places private transcripts, tokens, private tasks and repository
  contents outside federation.
- **Doc 98's authority table** lists "private transcripts, tokens, private tasks
  or repository contents" as explicitly outside the Keeper default, and
  "membership itself granting terminal control" as outside it too.

None of those are wrong about what they were deciding; the operator has changed
what is wanted. They must be amended rather than left to contradict this, and
until they are, an implementer reading them will build the wrong thing.

## Authority gradient

Visibility follows the grant, not the person:

- A **Steward** sees the Hives in their exact granted scope. Moleek stewards
  Paul's Hive, so Moleek has line of sight into Paul's Hive and nothing else.
- The **Keeper** sees everything in the Apiary.

This is the existing Stewardship scope model carried through to observation and
takeover, not a new one. Grants remain explicit, revocable and rechecked at
Keeper on every authenticated request.

## Watching is a live window, never an archive

⚠️ THE TWO PROPERTIES BELOW ARE WHAT MAKE FULL VISIBILITY SAFE ENOUGH TO BUILD.
Without them this is standing surveillance of every member's machine.

**Live only, never stored.** Keeper relays frames and does not persist them or
turn them into an Apiary transcript. This is exactly how ADR 0036 already
specifies takeover relay, and it is kept deliberately: a compromised Keeper then
exposes what is on one screen now, rather than months of every member's terminal
history including whatever they have typed. The member Hive keeps its own
ordinary private bounded history, as it does today.

**Always visible while watching.** The watched operator can see that they are
being watched, for as long as it lasts. Takeover is already visible because it
replaces the local engagement lease; passive watching must be too. Invisible
observation is the property most likely to make people distrust software they
are asked to run on their own machine, and it would be discovered rather than
disclosed.

## Takeover extends ADR 0036 rather than replacing it

ADR 0036 stands as written and is already half-built: the two-phase lease store
and terminal-host authority primitive are implemented and tested, the tables
exist and are empty, and no route exposes them. Its release gate — outbound
relay, restart reconciliation, automation recovery, audit presentation, and
desktop/mobile control shipping together — is unchanged.

Two things are settled here:

- **Keeper may take over, on the same terms as a Steward.** One bounded lease,
  Queen only rather than an arbitrary private worker, at most five minutes,
  renewing only while authenticated input continues.
- **Instant reclaim always, including against Keeper.** The local operator can
  reclaim from any authenticated local surface, immediately, and Keeper may
  simply take over again. The operator at the machine may be mid-deploy or
  mid-incident and is the only one who knows; removing reclaim would make the
  lease unsafe rather than merely powerful. Every reclaim is audited with its
  reason, and Keeper losing a few seconds to a re-take costs nothing.

## Policy becomes shared defaults that members converge to

Today `apiaries.policy_revision` is a bare INTEGER and there is NO policy
content table anywhere in the schema. Members accept "revision 3" and revision 3
has no body. That is an acceptance protocol with nothing in it, and it is why
"Keeper settings distribution" keeps reappearing as unfinished.

A policy revision should carry **shared defaults a member applies locally**:
check and validation configuration, required workflow, agreed conventions —
things that can be seen to have been applied or not.

Three properties, in order of importance:

1. **Drift is visible rather than prevented.** A member may still override
   locally; the override shows as drift from the Apiary default instead of
   silently winning. A federation that cannot be deviated from is remote
   control; one that cannot detect deviation is decoration.
2. **It has teeth because it is checkable.** A default nobody can measure
   adherence to is the acknowledged-statement model, which is what exists today
   plus prose, and changes nothing on any machine.
3. **Never credentials, filesystem roots or provider permissions.** Doc 98
   excludes these and that exclusion is NOT superseded. Visibility into a Hive
   is one thing; Keeper writing its credentials or execution permissions is
   another, and nothing in this interview asked for it.

## Steward depth: same window, fewer Hives

Your grant decides WHICH Hives you can see, never HOW MUCH. A Steward sees each
Hive in their exact scope exactly as Keeper would; Keeper additionally sees every
other Hive. One rule rather than two observation depths to build, test and
explain — and it matches the operator's own framing, where Moleek stewarding
Paul's Hive "would have line of site".

ADR 0034's context already conceded that counts-only was "insufficient for a
Steward to know whether help was useful".

## The Observe grant is upgraded in place

Full watching rides on the existing **Observe** grant rather than a new one.

⚠️ THE PREMISE WAS "there is only one apiary out there, mine", AND IT IS ONLY
MOSTLY TRUE. Checked against the live store on 2026-09-21: one Apiary, but also
ONE live stewardship carrying FOUR capability grants across EIGHT memberships.
So the upgrade is not zero-impact — it widens exactly one Steward's authority
over the Hives that stewardship covers, and whoever holds it gains terminal
visibility without having agreed to it. The operator owns the Apiary and made
this call knowingly; the affected Steward should be told rather than discover it.

## Fleet versions are shown AND raised

Each Hive reports its running Swarm version and schema version. The Apiary view
shows them, and a Hive behind the Apiary's newest raises an attention item.

⚠️ DISPLAY ALONE WOULD NOT HAVE CAUGHT THE THING THIS COMES FROM. On 2026-09-20
this Hive sat wedged on a stale build for a day. A version column would have
shown that the whole time and been read by nobody; what was missing was
something that SAID SO. Raising it is the difference between dogfooding being
visible and being guaranteed.

Nothing here lets Keeper push an update. Seeing drift is visibility; triggering
a member's build and restart is a much larger authority step and was not asked
for.

## Transport: WebSocket, chosen now

One outbound persistent WebSocket per Hive, member to Keeper. Not one per
worker, not one per browser. Members still need no inbound port.

⚠️ THIS DELIBERATELY OVERRIDES DOC 98, which instructs measuring actual latency
and resource use before choosing anything more complex than the simplest
transport. The operator chose to commit now on the grounds that takeover relay
will need bidirectional frames regardless, so the choice would be made twice.
Recorded as an override rather than an oversight: if polling would have
sufficed, this is where the extra complexity entered.

## The watch notice names the watcher, not the reason

While watching, the member sees WHO is looking. A reason is not required.

⚠️ THE CONSEQUENCE, RECORDED BECAUSE IT IS ASYMMETRIC WITH TAKEOVER: ADR 0036
requires a reasoned takeover command and audits the reason. Watching will not.
So the audit trail answers "who looked and when" and can never answer "why".
That is the deliberate trade for keeping a quick glance low-friction; if the
question "why was this Hive being watched" ever matters after the fact, this is
the decision that made it unanswerable.

## Version drift, settled in full

**The bar is the latest cut release, read from the signed release manifest
Swarm already publishes** — not from GitHub's API. The manifest is already the
trusted distribution path behind "offer 1.12.0 to Hives", so this keeps one
source of truth, adds no third-party network dependency, and has no way to
report a Hive as current because api.github.com was unreachable. A version check
that fails open is the silent-negative failure this codebase keeps finding.

**Dev Hives are exempt** from the Apiary raise, and the operator expects them to
be rare. ⚠️ THIS IS NOT A COVERAGE GAP, and should not be read as one: a dev
Hive behind its own checkout is already reported locally by the reload card,
which is what yesterday's wedge showed. Dev drift moves from a fleet signal to a
local one; it does not become unmonitored.

**Grace then raise.** A window after a release is cut — hours, not minutes —
then raise, and stay raised. Yesterday's wedge ran a full day, so the window can
be generous and still catch it. Raising instantly would fire across the whole
fleet on every cut and train everyone to dismiss it.

**Schema drift is louder and has NO grace window.** An app version behind is
stale; a schema behind can mean a member cannot read what Keeper sends. That is
a correctness problem rather than a freshness one and must read differently.
Yesterday's reload carried 183 to 184, which is exactly this kind of change.

**Every member sees the whole fleet's versions**, not only Keeper and Stewards.
A real widening of today's public-identity-only roster, but a small one beside
what has already been decided — and it makes "am I the one who is behind"
answerable by the person who can fix it.

## Policy is a signed manifest of key/value settings

Not a document, not a file bundle. It reuses the signed-catalog transport
already carrying an ordered manifest with a canonical digest that fails closed
on removal, substitution or renaming.

⚠️ THE FORM IS CHOSEN FOR ONE REASON: SETTINGS ARE COMPARABLE, SO DRIFT IS
COMPUTABLE. Visible drift is the property the whole policy decision rests on,
and a prose document would have to be diffed as text to answer "has this member
applied it". A file bundle was rejected separately: Keeper writing files into
member checkouts is a long step past distributing settings and close to the
remote control doc 98 refuses.

## The socket is a doorbell, not a pipe

⚠️ RECOMMENDED RATHER THAN CHOSEN, and recorded as such. The operator was
unsure and asked whether one mechanism would be better; this is my
recommendation and is open to being overruled.

Durable state keeps arriving through the existing polled, cursored, fail-closed
feed. The socket only says "something changed".

They are NOT two mechanisms for the same data. The feed carries state; the
socket carries the fact that state exists. A doorbell holds no data and so
cannot disagree with what is behind the door — the drift risk applies to two
paths both carrying state.

The failure severities differ sharply, which is the deciding argument. Doorbell:
the socket drops and state arrives slightly later, which is today's behaviour.
Pipe: the socket drops and state stops. Rebuilding ordering, idempotency, gap
detection and fail-closed-on-gap onto a stream would also mean reimplementing
the cursor and calling it one mechanism.

Live terminal frames are the one genuine exception and must ride the stream:
there is no durable state to fetch and the entire value is immediacy.

This is also doc 98's own instruction — announce durable changes, reuse the
existing outbound reconciliation owner to fetch and apply them — and it is the
reversible direction. Moving state onto the stream later is possible; adding a
safety net under state-on-socket is the harder retrofit.

## Not settled

- **What a policy revision physically is** — a signed document, a key/value set,
  a file bundle. It must fit the existing signed-catalog transport, which
  already carries an ordered manifest with a canonical digest.
- Nothing further from the 2026-09-21 interview. Remaining opens live on the
  individual tasks.
