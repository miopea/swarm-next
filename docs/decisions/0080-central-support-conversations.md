# ADR 0080: Central support conversations, separate from Hive execution

## September 9 native attachment contract agreement

### Independent intake activation

New Hive file intake defaults disabled even when text support is configured.
The deployment owner enables `SWARM_SUPPORT_ATTACHMENTS=true` only after Admin's
native route/storage activation is verified. False or absent keeps file intake
off; invalid configuration reports a degraded file subsystem without preventing
the Hive from starting. The status capability and authenticated upload admission
use the same setting, rejecting disabled intake before reading bytes. Existing
frozen files, receipts and sender retries are preserved; disabling intake never
deletes or silently downgrades a previously accepted report. Text support and
unrelated App/API improvements remain deployable independently of Admin changes.
This rollout compatibility gate is owned by central support; remove the default-off
rollout requirement only after supported central destinations all serve the native
contract. It grants no storage or mail permission and does not activate Admin.

### Hive upload/review implementation checkpoint

The local authenticated `/api/v1/feedback/support/attachments` adapter now accepts
the reviewed multipart envelope. Two process-owned upload permits cover stream
reading through the final blocking save; parsing has a 60-second deadline, a
13 MiB body cap and per-field bounds. Authentication and upload admission precede
reading. All files must match the ordered manifest before the single outbox
transaction; unknown, duplicate, missing or changed parts never save partial work.
The request only saves and wakes the existing sender; it does not perform a
central network send itself.

The browser retains one explicitly reviewed attachment report per origin in a
private IndexedDB record, including immutable byte copies and the existing report
identity. Its cap is four files/12 MiB plus bounded text and metadata; no periodic
sender or cleanup loop is introduced. Different pending reports cannot overwrite
each other across tabs. Exact replay is allowed. Only confirmation from the
content-comparing save endpoint clears the retry copy; a status row with the same
key alone does not prove that the reviewed content was saved. Existing text-only
tab retry copies remain compatible. The UI discloses file and image-metadata
sharing, allows removal before review, and never automatically attaches diagnostics.

This is not yet live acceptance: paired Admin upload/storage verification and
deployment remain gates. Desktop Edge fixture review/failure/reload/explicit-retry
passed; real mobile picker/device acceptance is not inferred from that check.

The Admin owner supplied and agreed the design in BFG Admin's
`docs/specs/native-feedback-attachments-proposal.md` (based on Admin main
`3a0f8bdbac5640776704af5c1922d679fab1d6c9`). This is an implementation contract,
not evidence that its routes or storage operations are deployed.

Keep the existing strict text-only JSON route unchanged. The new Admin-owned
`POST /api/feedback/:sourceId/submissions-with-attachments` accepts multipart
with a first JSON `manifest` part containing exactly `submission` and ordered
`attachments`. Each attachment has a nonnil UUID `id`, `file_name`, `media_type`,
`size_bytes`, and lowercase SHA256 `sha256`; exactly one `file:<id>` part supplies
its bytes. Manifest order is immutable; file-part order and multipart boundaries
are transport details. Both routes share source plus submission-key uniqueness.
The six-field receipt remains unchanged.

Limits are 1-4 nonempty files, 5 MiB per file, 12 MiB combined, 13 MiB HTTP body, and a
128 KiB manifest. Types are PNG, JPEG, WebP and UTF-8 plain text without NUL;
Admin validates raster signatures and bounded decoding up to 25 megapixels,
rejecting animation. Filenames contain 1-180 characters without separators or
control characters. No arbitrary URLs, public reads, SVG, PDF or archives.

Swarm must save exact reviewed metadata and immutable byte copies atomically
in its bounded private outbox before delivery. Recovered attempts never reread
an original path, recreate attachment IDs, omit a file, or downgrade to text.
Changing content, attachment metadata or manifest order under the same key
conflicts. Timeout, rate-limit and unavailable outcomes retain the same key
for exact retry. Local retention and global admission must include attachment
bytes, not merely JSON size. Existing text rows retain their encoding and retry
semantics. Browser lifecycle must not own delivery after the local save.

Admin owns private storage, reservation/quota accounting, atomic publication,
commit-versus-cleanup fencing and authenticated attachment retrieval. Hives
receive no storage or Admin-read credentials. Archive retains committed bytes;
explicit original deletion keeps receipt/hash tombstones, and an exact replay
must not resurrect deleted objects. No attachment is sent to AI or customers
merely by intake or task completion.

Implementation and activation remain separate gates: prove exact replay,
changed-file conflict, interrupted local save and delivery recovery, byte and
concurrency limits, explicit review, and populated mobile/desktop behavior.
Admin must separately prove private storage and cleanup safety. Review its
concrete migration and storage permissions before production deployment.
Use fictional fixtures only and preserve the reserved linked-task fixture.

## September 8 ownership and activation correction

This section supersedes the historical separate-support-runtime design below.
BFG Admin owns central conversations in its existing database, including
approved customer replies. Do not deploy another Swarm support service or
central database. Swarm owns execution tasks and a bounded private Hive outbox.
Schema 153 adds that outbox after deployed Queen focus schema 152; migration
alone neither activates delivery nor exports historical diagnostic reports.

Reviewed native submissions target the configured Admin HTTPS origin at
`/api/feedback/swarm-support/submissions`, with email as required contact and
without Admin credentials. Frozen destinations are never silently redirected.
Exact retries preserve the submission key and bytes. Conflicts stay visible;
rate limits retain the bounded server deadline across restarts. Native attachment
upload remains unavailable until its contract exists.

The operator approved development-Hive activation for fictional end-to-end
testing. Actual Admin-route restart/replay acceptance passed; live Hive UI
submission and recovery remain a verification gate. Admin separately verified
its approved mail roundtrip. No customer send is implied by task completion.
The separate `app_id=swarm` MCP workspace binding remains unapproved and must
not change as part of intake activation. Historical deployment and schema-143
checkpoints below are not the current production topology or migration ceiling.

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

### Hive outbox persistence (2026-09-06, integration pending)

Schema 143 introduces a separate execution-Hive outbox for explicitly reviewed
submissions. It freezes the submission key, exact encoded payload and configured
destination before delivery. Admission is bounded to 256 retained rows and 16 MiB
of frozen payload; metadata is additionally bounded per row. Capacity refuses new
submissions without deleting unanswered reports, while exact-key replays remain
available. No private historical Dogfood report is automatically copied into it.

Pending and uncertain reports may claim up to five automatic attempts. A claim
has a unique attempt ID; concurrent claims and superseded completion receipts
cannot settle another attempt. Confirmed and definitively failed records are not
automatically replayed. Process-owner recovery changes interrupted Delivering to
Uncertain, never to success, and retains the frozen key and payload. Recovery must
run only after the previous sender instance ends, not from a timeout alongside a
live sender. Matching remote receipt identities are required for confirmation.

This is not an activated sender. Application authentication, configured-origin
validation, bounded process-owned transport, safe error presentation, explicit
retry/retention controls and the operator UI remain required. No transport or
customer-facing reply is invoked by schema migration or opening an outbox record.

### Bounded Hive HTTP transport (September 7, not activated)

Local retention is an explicit operator action bound to the confirmed central
message receipt. Pending, in-flight, failed and uncertain reports cannot be
removed. Deleting a confirmed local copy releases bounded Hive outbox capacity,
not the central conversation or its history; it is not a central erasure request.
Exact absent-copy replay is harmless, while a changed receipt is refused. The
control remains available when central delivery is disabled. No automatic retention
timer or bulk deletion is introduced, and the UI warns before local removal.

Explicit operator retry is a separate command with a stable retry ID and the exact
observed attempt ID. It grants one additional attempt, never resets automatic
counts, and retains frozen content/destination. Replayed commands cannot replenish
the grant after consumption; changed and stale commands refuse. Additive delivery
JSON fields default to no manual grant for older rows. Startup recovery retains
the last attempt identity while its non-Delivering state rejects late settlement.
The browser retains an ambiguous command for exact replay; a definitive conflict
clears only that command and asks for refreshed state. Neither path creates a new
support submission. This is not permission for customer replies or unbounded retry.

The transport sends only the exact frozen report to its deployment-owned endpoint.
It refuses redirects, bounds connection establishment to five seconds and the
whole request/response to twenty seconds, and caps receipt bytes at 16 KiB even
without Content-Length. It carries no Admin credential and owns no retry loop.
Only a receipt revalidated by the application's durable attempt fence can confirm
delivery. Missing, invalid or mismatched receipts remain uncertain with the same
submission key; raw remote error bodies do not become operator diagnostics.

The process-owned sender and authenticated operator UI are not wired yet. That
owner must bound concurrency, retain admission through blocking persistence work,
recover interrupted claims only after its predecessor ends, and settle outcomes
durably. Constructing this adapter alone starts no task and sends no report.

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
