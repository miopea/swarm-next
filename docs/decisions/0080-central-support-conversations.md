# ADR 0080: Central support conversations, separate from Hive execution

Status: accepted implementation design under the operator's September 6 scope.
Implementation underway, not connected. Central deployment URL and credentials
remain unset. Initial domain validation and isolated persistence do not activate
public intake, Admin reads, Hive delivery or customer replies.

## Outcome and ownership

Every Hive can explicitly submit Swarm feedback to one central support source.
Email is the only required contact identity. No BFG account, public customer Hive,
terminal access or repository access is required. BFG Admin owns triage and the
operator-approved customer response. Swarm owns linked implementation tasks.

The central source is a separately deployed support runtime built from this
repository, not public routes added to the development Hive's execution router.
It has no terminal-host connection, worker credentials, task-execution API or
access to customer Hive databases. Support business rules belong to the domain
and application boundaries; only persistence repositories access SQLite.

BFG Admin pulls the central source's privileged conversation resources. This is
not a new public ingress on Admin. ADR 0072 remains the independent, scoped path
for approved Admin requests becoming Swarm development tasks and progress reads.
Task progress never re-enters support as a new customer report.

## Three separate trust boundaries

1. Public submission accepts explicitly reviewed feedback and supplied email.
   It does not issue list/thread/download/send privileges. Supplied email is
   contact information, not proof of account ownership. Preserve additional
   supplied identity but never fabricate missing account IDs or names.
2. Admin conversation reads/downloads use a dedicated privileged credential.
   Public submitters, AI, ordinary Ops readers and worker MCP principals cannot
   use it. Credentials are deployment-provisioned, never bundled into Hives.
3. Customer replies require the Admin-approved command, a source-verified target
   and the reviewed conversation revision. AI drafts cannot send or choose a
   different recipient. Only source-verified provider callbacks can change
   provider-delivery evidence.

Public requests are bounded by body size, attachment limits, owned admission
limits and durable storage capacity. Saturation is an explicit refusal, not
unbounded memory or silent removal of unanswered conversations. Request bodies,
email addresses and diagnostic contents do not enter ordinary logs or metrics.

## Durable identities and recovery

Use separate IDs for submission, support conversation, message, attachment,
diagnostic report, Admin request, development task and delivery. Do not substitute
an email address for a conversation ID. Unrelated reports by the same sender stay
separate; subsequent messages require the exact verified reply reference.

Each Hive creates a stable submission key before sending. Its bounded local
outbox freezes the reviewed content and retains the same key/content on retry.
The central transaction records the key, content digest, conversation and first
message together. Exact replay returns the original receipt; changed payload
under the same key conflicts. A lost response must not create a second report.
Browser lifetime never owns delivery. Pending, confirmed, failed and uncertain
states remain visible; local save alone is never labelled sent.

Inbound provider event IDs are deduplicated durably. Delivery callbacks update
delivery state, not customer history as a new request. Self-sent and automated
events must not trigger reply loops. Failed or unmatched inbound messages remain
available for operator investigation instead of being guessed into another thread.

Admin's old email `/replies` and channel-aware `/deliveries` share one source
idempotency namespace and one unresolved-send lock. Normalize both to the agreed
canonical reviewed email envelope, preserving exact recipient and actor. Replays
use the saved target binding before current thread state. If an older receipt has
no provable target binding, recover through its original endpoint; never infer a
new recipient or send again. Acceptance is not delivery, and delivery is not read.

## Contract, attachments and diagnostics

Implement the paired contracts in BFG Admin's `docs/contract/`:
`operator-conversations-v1.md`, `swarm-requests-v1.md`,
`fleet-feedback-onboarding.md` and the agreed `conversation-delivery-proposal.md`.
Admin owns schema/proxy/UI changes; this repository owns the central source and
Hive submission client. Do not silently add fields that Admin validation strips.

Preserve source links, plain-text paginated history, full known sender identity
and supported attachment metadata. Each immutable attachment ID belongs to one
conversation. Download authorizes that ownership every time; no arbitrary URL
fetches, redirects or bearer credentials in metadata. Explicit downloads stream
with cancellation, a 60-second total deadline and a 32 MiB hard byte cap (a
deployment may lower it). Check Content-Length early and enforce actual bytes.
At most 20 metadata entries per message; opening a thread downloads none of them.
Oversized/unavailable attachments remain visible with an honest reason.

Diagnostic bundles remain separate, linked records. Explicit user review is
required for export; do not automatically upload terminal history, repositories,
credentials or arbitrary files. Diagnostics and attachment bytes are excluded from
AI summaries unless separately authorized. Do not migrate old private Hive reports
to the central source merely because the feature becomes configured.

## Completion loop and activation gate

### Central source discovery and durable health

BFG Admin confirmed the single source identity `swarm-support` / `Swarm Support`.
Optional ordinary Ops polling uses a credential distinct from privileged
conversation reads. Reusing those credentials is refused. Manifest advertises
only `operatorResources: ["conversations"]` when private reads are enabled;
there is no implied write grant or per-Hive registration. Core health contains
no contact, message, attachment, or diagnostic payload. Metrics/incidents may be
truthful empty envelopes until those source capabilities exist.

Support schema 2 adds one constrained health-probe row. A health check reads the
support tables and commits a toggled probe value; a failed read or commit cannot
report a healthy durable store. The separately configured disk-space check uses
the actual database filesystem and an explicit positive threshold. Missing disk
observation is degraded, not healthy. This is operational evidence, not a complete
corruption scan or proof of future availability. Existing frozen submissions and
retry identities are unchanged during migration. These routes remain optional
and do not activate the source in Admin or send any customer reply.

The initial persistence foundation admits a configured positive maximum number
of conversations and refuses new reports at capacity without deleting history.
Exact idempotent replay remains available at capacity. Conversation, frozen
submission and initial message commit atomically; admission and duplicate checks
use one immediate transaction. A separate application ID/schema gate refuses
existing execution databases before support schema changes. Later migrations
must preserve frozen submission encoding for retry comparison; serialization
changes are not a reason to accept different content under an old key.

Closed implementation work supplies linked completion evidence to Admin, which
prepares the response for operator approval in each originating conversation.
Several requests may share a task without merging customer threads. Closed is not
deployed; customer wording must reflect independently verified deployment facts.

Activation needs a central source URL, mail-provider ingress/outbox wiring,
privileged Admin registration and credentials, and a rollback/recovery path.
Verify fictional feedback/bug/feature/email journeys, multi-page history, identity
retention, stale revisions/targets, cross-thread refusal, duplicate ingress,
lost-response replay across both reply endpoints, callback/reply-loop handling,
offline Hive submission recovery, attachment limits/revocation, and explicit send
approval. Never send test messages to real customers. Native in-app replies remain
explicitly unavailable until a recipient-visible inbox and delivery proof exist;
do not silently substitute email for an original in-app channel.
