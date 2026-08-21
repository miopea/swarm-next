# ADR 0050: Discovering and fetching a release

## Status

Proposed. Raised by `docs/31` item 34, on the operator's account of Swarm
Legacy: a "dev" mode for local work with fast hot updates, and a normal user
mode that polled for updates or checked on demand. Swarm needs the
equivalent before anyone else can use it.

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

## Consequences

An operator with no checkout can learn a release exists and install it, with the
same warnings the local path already gives. A Hive that never opts in behaves
exactly as it does today.

Signing infrastructure becomes a prerequisite: a manifest nobody signs is a
manifest anybody can replace, and this design is worth nothing without it. That
is the real cost of this decision and it should be paid before any code here is
written.

Not decided here, deliberately: the release cadence, who holds the signing key,
and whether member Hives in an Apiary learn about releases from their Keeper
rather than from the origin directly. The last is genuinely attractive and
genuinely a different ADR.
