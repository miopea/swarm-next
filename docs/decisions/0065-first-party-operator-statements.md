# ADR 0065: First-party operator statements are not terminal activity

Status: Accepted architecture for approved QUEEN-03; implementation pending.

## Evidence and scope

ADR 0054 already permits a worker to verify a resolved decision from the durable
store. It does not verify an answer given directly in a worker terminal.
`TerminalWriteProvenance` distinguishes operator, coordination, and Steward
writes. `TerminalWriteAuditEntry` deliberately retains no content: actor, input
kind, byte count, result, session, sequence, and time. That audit must remain
content-free. An acknowledged PTY write proves neither a submitted provider turn
nor its meaning. Arrow keys and Enter in AskUser cannot reconstruct the selected
answer without the exact question and option identity.

## Decision

Add a separate, bounded first-party statement record through the application
service and persistence boundaries. Do not turn diagnostics or the terminal write
audit into a keystroke recorder. Queen and workers may read a verified statement;
neither may manufacture one by supplying an operator actor label or quoted text.

A statement needs an immutable ID, authenticated operator origin, worker and
session identities, exact submitted text, time, and evidence source. Keep transport
acceptance distinct from provider submission confirmation. Unknown delivery stays
unknown and must not trigger automatic replay. Worker-authored claims and provider
output are not eligible sources. Pasted material proves what the operator supplied,
not authorship or endorsement of instructions quoted inside it.

The composer can supply a complete submission boundary. Raw terminal writes
cannot: shell editing, history recall, TUI selection, interrupts, and provider
commands alter meaning. Raw-terminal support must use provider-native submitted
turn evidence correlated to authenticated operator input in the same session;
a timestamp coincidence or matching text alone is insufficient. Unsupported or
uncertain evidence remains unverified, not a fabricated authoritative statement.
This requirement includes desktop input and AskUser answers; a composer-only
implementation is an intermediate slice, not completion of QUEEN-03.

Decision correlation requires the full immutable decision ID and exact question
revision/identity, with an answer valid for that question. A worker's semantic
guess cannot close Needs You. Verified exact correlation resolves the decision
and records the source statement atomically, using the existing resolution
service. Duplicate consumption is idempotent; a different answer cannot overwrite
an already resolved request. If the originating worker already consumed the answer,
record that fact separately from notifications to Queen or other recipients: do
not inject the answer into that worker a second time. Ambiguous matches remain
available for Queen to reconcile without silently closing unrelated requests.

The initial domain contract compares the complete bounded question snapshot,
including header, wording, option order, and multi-select behavior. An interview
requires exactly one confirmed answer per declared question before resolution;
partial receipts cannot close the whole request. Receipt arrival order does not
matter. Individual answer text is capped at 16 KiB without trimming or truncation.
This is a payload bound, not the remaining durable retention budget. Shared domain
question validation also governs the existing persistence creation path.

Schema 130 introduces the initial private receipt store. Admission is capped at
4,096 records and 16 MiB of combined question/answer payload; IDs and metadata
are additionally bounded by the row count. On admission, resolved-decision
receipts older than 90 days expire. Open-decision evidence remains pinned and
exhaustion rejects new evidence explicitly without changing terminal delivery.
This is admission-time retention, not a periodic deletion guarantee. The initial
write path stores only confirmed consumption, verifies the local decision and
active worker/session binding, and never resolves or queues another delivery.
Exact ID retries are idempotent, even after a session ends. This store is not
exposed to agents; authenticating source and consuming evidence remain separate
integration work. Downgrade requires a compatible pre-migration database backup.

Schema 131 links the exact consumed receipt IDs to a resolution. A bounded set
of at most four receipts is reread and matched inside the resolution transaction;
the active worker session is revalidated before a pending request can close.
Identical linked-set retries are no-ops, not new deliveries. Resolution, links,
the decision-changed event, and delivery state commit together. When the receiving
worker is the requester, its confirmed consumption records delivered state without
another injection. A different requester (for example Queen) still receives the
existing queued answer notification. This is persistence integration, not proof
of authenticated end-to-end provider capture. Partial evidence leaves Needs You
pending until every question has a matching confirmed receipt.

Verification reads return the statement and its scope, evidence status, and exact
decision link when present. A full ID is required; no prefix lookup. Verified
origin never expands the action authorized by the actual words. Preserve ADR 0054's
distinction between first-party evidence and an agent's relay.

## Ownership, bounds, and recovery gates

The application service owns authentication and correlation orchestration; domain
rules own admissibility and transitions; persistence owns atomic resolution,
uniqueness, retention, and migration. The independent engine continues to own PTY
delivery. Browser component lifetime cannot own a pending durable statement.

Before runtime integration, specify and test hard text, record-count, total-byte,
and age limits. Referenced decision evidence must not disappear while an unresolved
decision depends on it; capacity exhaustion must be explicit rather than silently
dropping evidence or blocking ordinary terminal input. No statement content goes
into automatic Dogfood captures, diagnostic bundles, or general activity payloads.

Required tests include forged agent provenance, quoted third-party instructions,
rejected and uncertain input, superseded sessions, duplicate receipts, changed
questions, conflicting answers, interrupted submissions, AskUser option identity,
capacity exhaustion, restart recovery, and no duplicate worker delivery. Prove the
complete operator-to-worker-to-Queen lifecycle before claiming double-answering
fixed. Provider-specific evidence support belongs in provider maturity validation.

No compatibility inference upgrades historical content-free audit entries into
statements. Existing resolved-decision verification remains supported unchanged.
