# ADR 0050: Discovering and fetching a release

## Status

Accepted, 2026-08-21. Raised by `docs/31` item 34, on the operator's account of
Swarm Legacy: a "dev" mode for local work with fast hot updates, and a normal
user mode that polled for updates or checked on demand. Swarm needs the
equivalent before anyone else can use it.

Amended on acceptance. The original text proposed deferring all code until
signing infrastructure existed, and separately entertained shipping discovery
without fetch. Both were rejected by the operator on the same ground: **a user
who is told a release exists but cannot install it has not been helped.** The
signing decision below is therefore made here rather than postponed, and the
whole path — check, fetch, verify, install — lands together.

Depends on [ADR 0049](0049-claiming-worker-engagement-without-input.md) only in
spirit. It builds directly on the versioning decision recorded in `docs/31`
item 43, without which none of this is expressible.

## Context

The gap is narrower than it looks, and reading the surface rather than assuming
is what narrows it.

**Developer mode is real and complete.** `swarm-package` carries
`enable-development CHECKOUT`, `disable-development`, and `reload-development`.
`development.enabled` in the runtime response means precisely "is a development
reload path configured". This is the mode Swarm is being built in.

**Applying a release already exists.** `swarm-package install|update
RELEASE_DIR` installs a prepared release directory, and that directory carries
`SHA256SUMS`, `VERSION`, `PROTOCOL`, and `SOURCE_REVISION`. Integrity and
compatibility checking are already part of the release format.

**Nothing discovers a release.** There is no reference to GitHub, a release
channel, or an update check anywhere in the Rust code. A user with no checkout
cannot learn that a new version exists, and cannot fetch one.

So what is missing is discovery and fetch, not application. That is a smaller
gap than it first appears and a more sensitive one, because it decides where a
release comes from and what the operator is agreeing to when they accept it.

Two facts from this codebase's own history bear on it directly.

**A release is not isolated from the workers.** `docs/31` item 22: an App and
API deploy changed the worker-engine build id, and a reconcile timer applied the
engine update on its own thirty minutes later, stopping seven workers. Whatever
an updater does, it inherits that lesson. A card saying "workers stay online" is
a claim about one component, not about the consequences of installing it.

**A version has to be comparable before anything can decide to update.** Until
`docs/31` item 43, a release carried its revision, so no two releases could be
ordered. `SwarmVersion` now supplies that, and deliberately refuses to treat a
development build as an upgrade in either direction.

## Decision

### 1. A release is discovered from a signed manifest at a configured origin

Not from a code host's API. The updater fetches one small manifest describing
available releases — version, protocol, artifact URL, and the `SHA256SUMS` that
release format already carries — and the manifest is signed.

The origin is configuration, defaulting to the project's own. A Hive that must
not reach the internet sets no origin and the whole path is inert, rather than
failing repeatedly against a host it was never meant to contact.

### 2. Provenance is verified before an artifact is trusted, not after

The manifest signature is checked before any artifact URL in it is fetched, and
the artifact's digest is checked against the manifest before it is unpacked. An
artifact that fails either check is discarded and reported, never installed and
rolled back — rollback is a worse position than never having installed.

`PROTOCOL` is checked in the same pass. A release that cannot speak this Hive's
terminal protocol is not offered, because offering it would make the operator
the one who discovers the incompatibility.

### 3. Checking is consented to once; installing is consented to every time

Two separate consents, because they are two separate acts.

Whether to check at all is a setting. Default off until the operator turns it
on, so a Hive never contacts an origin its owner did not choose.

Installing always asks, and the asking says what stops. The control room already
distinguishes an App and API release, which keeps workers online, from a worker
engine or provider replacement, which does not. An update fetched from outside
gets the same treatment and the same wording, because the operator should not
have to learn a second vocabulary for the same consequence.

### 4. A development build is never updated automatically

`SwarmVersion` already refuses to order a development build against a release.
The updater inherits that: a Hive built from a working copy is told a newer
release exists and offered nothing else. Replacing someone's checkout-built
binary with a release would discard work whose contents nothing can enumerate.

### 5. The engine consequence is stated at the point of installing

Item 22's lesson, encoded rather than remembered. Before installing, the
updater compares the release's worker-engine build id with the running one and
says plainly whether applying it will stop workers — at the moment of consent,
not after, and not left to a reconcile timer to discover half an hour later.

### 6. The verifying key is compiled in, never fetched

The decision that makes the rest worth anything. A public key retrieved from
the same origin as the manifest verifies nothing — whoever replaces one
replaces the other.

Swarm carries the release verifying key as a constant in the binary. The trust
chain is therefore: the operator trusts the build they installed, that build
carries the key, and the key vouches for every release offered afterwards.
Nothing is bootstrapped over the network.

The cost is rotation. A new signing key cannot be announced through a channel
the old key protects, so rotating one means every existing install verifies
nothing signed by the new one and has to be updated by hand. That is the
correct failure: silent key replacement is precisely the attack being refused.
Rotation is an announcement and a manual update, and it should be rare.

The private key is held by the operator in 1Password and used only by the
release script. It never exists in the repository, the release bundle, the
manifest, or any log.

### 7. Installing is a request, not a call

The API cannot install a release, because installing restarts `swarm-api`
— the process making the call would be killed mid-command and the result
reported to nobody. This is the same shape as the migration script, which runs
detached for the same reason.

So the API writes a request file naming the verified directory, and a systemd
path unit runs `swarm-package` against it. That mechanism already exists twice
in this product — `swarm-host-reconcile.path` and
`swarm-development-reload.path` — and it already reports progress through a
status file the control room reads. A third use adds no new concept.

`swarm-package` re-verifies the bundle's own `SHA256SUMS` before installing
whatever the request names, so the integrity check does not depend on the
requester having been honest.

## Consequences

An operator with no checkout can learn a release exists and install it, with the
same warnings the local path already gives. A Hive that never opts in behaves
exactly as it does today.

Signing infrastructure is a prerequisite and is paid for here rather than
deferred: a manifest nobody signs is a manifest anybody can replace. The bill
is one Ed25519 keypair, held in 1Password, and a signing step in the release
script. `ed25519-dalek` is already a workspace dependency and `federation.rs`
already establishes the canonical-payload-then-signature idiom, so the code
cost is small. The operational cost — that a lost or rotated key strands every
existing install — is real and is accepted above.

An update check is an outbound connection, and that is a privacy fact whatever
its purpose. Swarm sends no version, no Hive identity, and no counts: it
fetches one static document and compares locally, so the origin learns what any
static file host learns and nothing more. Checking is off until the operator
chooses, and a Hive that never chooses never connects.

Not decided here, deliberately: the release cadence, who holds the signing key,
and whether member Hives in an Apiary learn about releases from their Keeper
rather than from the origin directly. The last is genuinely attractive and
genuinely a different ADR.
