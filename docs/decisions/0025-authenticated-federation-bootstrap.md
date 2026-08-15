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

### Operator-facing invitation flow

Connection-card files are an implementation-safe bootstrap artifact, not the
primary product experience. The normal flow is a short-lived Keeper invitation
URL (and matching QR code):

1. Keeper chooses **Invite a Hive**, scopes the invitation, and receives one
   expiring, single-redemption URL rooted at the Keeper's reachable HTTPS base
   URL.
2. The invited Hive opens the URL and sends its signed public connection card
   outbound to Keeper. The one-time redemption binds the invitation to that
   exact Hive/node key before Keeper issues the existing signed invitation
   envelope.
3. Keeper shows the bound Hive and operator identity for explicit approval.
   The invited Hive polls that same Keeper URL outbound and receives the
   targeted signed invitation only after approval; Keeper never calls the
   member machine.
4. The Hive previews Apiary, Keeper, policy, backend, and promoted-project
   requirements locally, then explicitly accepts and joins through the existing
   fail-closed protocol.

The URL is bootstrap capability, not durable membership authority. It expires,
is consumed once, and cannot replace the signed identities, exact policy
acceptance, readiness checks, receipt, or node credential. Download/upload of
signed cards and bundles remains an advanced manual/offline recovery path.

The Keeper base URL must be reachable from every member Hive. A public domain,
private VPN/mesh name, or trusted LAN HTTPS name is valid. Loopback names are
valid only for explicitly marked same-machine development because `localhost`
on a member points back to that member. Member Hives initiate outbound
authenticated connections to Keeper; ordinary developer machines require no
inbound port or public URL.

Keeper unavailability must degrade coordination, not local work. Hives retain
workers, private tasks, terminals, and their local Jira operation, queue bounded
outbound federation events, show Apiary transport as offline, and reconcile in
order after reconnect. Cross-Hive rollups, new shared assignments, project
promotion, and takeover wait for Keeper reachability.

For Jira-backed Apiaries, each Hive synchronizes promoted issue content,
comments, status, and assignment directly with Jira using its own operator
identity. Keeper polling carries membership, policy, project-catalog,
Stewardship, and coordination facts, but is not a Jira data proxy. For Native
Apiaries, Keeper is the canonical shared-task service, so members also retrieve
and reconcile Apiary task events through that same outbound poll channel.

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

The invited-Hive import slice now verifies the independently signed Keeper card
and invitation envelope, requires every Keeper identity field to match, requires
the invitation target to equal the local node/Hive/operator tuple, rejects
unsupported Native invitations and insecure or expired material, and stores the
decoded bearer secret only in private Hive persistence. The browser first shows
the exact Keeper, Apiary, backend, policy revision, and expiry; a separate
authenticated command explicitly pins the key. Its response and later reads
exclude the secret, public key, and complete envelope. Membership and policy
acceptance remain unchanged.

The promoted-project manifest slice now sends the bounded, ordered public Jira
project identities beside the invitation and verifies their canonical digest
against the signed envelope before import. The invited Hive stores those rows
in an immutable normalized table and exposes them for operator review without
credentials or issue content. A separate authenticated local command now
accepts only the exact signed policy revision; it grants no membership and
contacts no Keeper. Server-derived preflight joins each signed immutable
project ID to private local Jira binding evidence and reports access and
workflow-map readiness per project. Mutable project key and name are not
authorization identities. Manifest tampering, duplicate identities, stale
policy revisions, incomplete access, and incomplete mappings fail closed.
Clearing the preflight means only that the Hive is ready to submit the one-time
handshake.

The Keeper-consumption slice now accepts a signed submission without browser
or operator-session authentication: the one-time bearer secret and the exact
pinned Hive signature are the credential. The submission binds invitation,
Apiary, policy revision, catalog digest, and all invited identities. One
transaction rechecks the current Keeper policy/catalog, consumes the pending
invitation, creates the remote operator/Hive membership, stores a bounded node
credential, and signs a membership receipt. An identical authenticated retry
returns the same durable receipt and credential; altered or expired replay
fails closed. The response is `no-store`, and independent-database persistence
and HTTP tests prove only one membership is created.

The invited-Hive application slice now verifies the receipt against the pinned
Keeper key, matches every Apiary/Keeper/member/policy/catalog identity to the
imported invitation, validates the bounded credential, and atomically stores
the private credential, mirrors public Apiary/Keeper identity, marks the local
Hive as a Member, consumes the local invitation, and revokes competitors.
Identical application is idempotent; altered material fails closed.

Later slices must prove protocol negotiation beyond version 1 and the complete
outbound HTTPS call between two independently running processes.
