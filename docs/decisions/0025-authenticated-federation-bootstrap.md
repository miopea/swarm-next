# ADR 0025: Authenticated federation bootstrap

Status: **Accepted**

## Context

Apiary membership currently has durable invitations, revision-bound policy
acceptance, and atomic local join checks. Those guarantees are real only when
the Keeper and invited Hive identities already exist in one database. Separate
one-operator Hives need a transport that establishes identity and trust before
membership without sharing Linux accounts, browser sessions, Jira credentials,
repositories, tasks, terminals, or provider conversations.

An opaque invite code alone is insufficient. It cannot prove which Hive the
Keeper intended to invite, which Keeper issued the invitation, whether a card
was altered, or whether a replay targets the current policy and protocol.
Relying on Cloudflare Tunnel identity would also couple core federation to one
deployment adapter.

## Decision

Each installation owns one durable Ed25519 federation-node identity. Its
private signing seed stays in the private Hive database and is included in the
operator-controlled encrypted/secured backup boundary. It is never returned by
an API, browser view, agent tool, diagnostic report, or federation message.

A Hive operator may deliberately download a short-lived signed **Hive
connection card**. Version 1 contains only:

- connection-card schema and federation protocol versions;
- stable node, Hive, and operator identifiers;
- display names;
- the node public key;
- issue and expiry timestamps; and
- an Ed25519 signature over the canonical payload.

The card contains no endpoint, bearer secret, membership, authority, project
catalog, Jira identity, repository path, task, terminal, or provider data.
Generating or importing it grants no access. Cards expire after at most seven
days; the product default is one day. Signature, versions, timestamps, and
bounds fail closed.

The staged bootstrap protocol is:

1. a personal Hive shares a current connection card out of band;
2. a Keeper verifies and pins that exact node/Hive/public-key tuple as a
   candidate, then explicitly creates a bounded one-time invitation;
3. the Keeper returns a signed invitation envelope containing the Apiary
   identity, immutable backend, current policy revision, promoted-project
   catalog digest, Keeper endpoint, invited identity, expiry, nonce, and only a
   digest of its one-time bearer secret in Keeper storage;
4. the invited Hive verifies the pinned Keeper signature, explicitly accepts
   the exact policy revision, and submits its readiness evidence plus the
   one-time secret over HTTPS;
5. the Keeper atomically consumes the invitation, registers membership, and
   returns a signed membership receipt and bounded node credential;
6. both sides persist sequence/idempotency state before ordinary catalog,
   status, claim, or rollup messages are enabled.

TLS is mandatory for remote endpoints, but application signatures and pinned
node identities remain authoritative. Deployment adapters may add Cloudflare
Access, mTLS, or a private network without changing this protocol. Rotating a
node key is a separately authorized, audited protocol; presenting a new
self-signed card never silently replaces a pinned key.

## Consequences

- A Keeper can distinguish identity discovery from invitation, policy
  acceptance, membership, and later authority.
- Copied card files are safe to inspect and intentionally low sensitivity, but
  still expire and must not be treated as invitations.
- The existing same-database invitation implementation becomes a local test
  adapter for the same state machine, not the final distributed transport.
- Cross-Hive project propagation, atomic claims, offline queues, and
  reconciliation build on authenticated node sessions rather than browser
  cookies or shared operator tokens.
- Losing the private node key requires an explicit recovery/rejoin procedure;
  silently generating a replacement would break pinned identity.

## Validation

The connection-card, Keeper-pinning, and invitation-issuance slices now prove that a fresh or
migrated Hive lazily creates one stable keypair, repeated cards retain the same
node/public identity, every card is signed and bounded, and
tampering/expiry/corrupt key material fails closed. Download and import require
operator authentication and disable caching. A Keeper can pin a card issued by
an independent database, while personal/member Hives, self-pinning, identity
collisions, and silent key replacement fail closed. Candidate persistence does
not create a Hive member or invitation, and the UI states that no access was
granted. An active Keeper can now issue one signed invitation bundle for an
exact pinned candidate. The envelope binds Apiary identity, immutable backend,
policy revision, promoted-project catalog digest, Keeper HTTPS endpoint, both
node identities, expiry, and nonce. The 256-bit bearer secret is returned once;
only a domain-separated SHA-256 digest is retained. Duplicate pending
invitations, insecure remote endpoints, tampering, expiry, wrong verification
keys, and Apiary collapse with a pending distributed invitation all fail closed.

Later slices must prove invited-Hive Keeper-key pinning, one-time invitation
consumption, replay rejection, protocol negotiation, and a complete join
handshake between two independent processes.
