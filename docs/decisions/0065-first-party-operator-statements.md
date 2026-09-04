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

Verification reuses `swarm_list_decisions` with an alternative `statement_id`
argument. Supplying both selectors is rejected. Full typed IDs only; a missing
retained receipt reports unverified rather than granting authority. The response
contains the exact statement and question, operator, worker/session, recorded time,
and resolution-link status. A verified statement is not a claim that every question
in the decision is answered. This read adds no agent write capability, general
statement listing, or control-room invalidation. Existing provider sessions need
their normal tool-schema refresh before the optional argument becomes visible.

## Provider capture boundary

The [Claude hook reference](https://code.claude.com/docs/en/hooks#userpromptsubmit)
documents submitted text on `UserPromptSubmit`, before processing; a hook may
still block that prompt. Its `PostToolUse` event supplies tool input and response,
whose shapes are tool-specific. These facts do not independently identify a human
terminal writer or give a stable decision ID for an AskUser response.

The initial prompt parser therefore returns only `ProviderPromptObservation`:
conversation and exact text, no consumption or human-authorship claim. It drops
paths/other metadata, rejects child-agent events, caps transport at 64 KiB and
decoded text at 16 KiB, and redacts Debug output. It is not installed as a hook
yet. Transport authentication, engine-owned operator-input correlation, provider
consumption evidence, and decision identity mapping must precede receipt creation.
Do not wire `UserPromptSubmit` directly to confirmed operator statements.

### Installed-provider reconciliation, 2026-09-04

Read-only checks on the operator's remote host found Claude Code 2.1.260 at
`/home/bschleifer/.local/bin/claude`. Its help limits `--input-format` and
`--output-format` to `--print`; `--replay-user-messages` requires both formats to
be stream-json. This does not establish an acknowledgement channel for the current
interactive PTY. No agent session was started, stopped or changed by the checks.

Do not switch workers to print/SDK mode to make evidence collection easier. That
would change native interactive behavior, including AskUser, and requires its own
architecture and product acceptance. Likewise, matching a hook's text against the
most recent input is insufficient: identical prompts may recur, another hook may
block processing, and automation uses the same PTY interface.

The next integration must separate two independent facts: authenticated operator
authorship of a complete composer submission, and confirmed provider consumption.
An authored statement can be verifiable even while delivery is uncertain; it must
not be represented by the existing confirmed-answer receipt until consumption is
proved. General operator statements also need not reference an existing decision.
The current schema 130 receipt store is intentionally the narrower consumed-answer
side, not the completed general statement model. Add the source-side record and
correlation without weakening confirmed-answer admission or deriving authority
from arbitrary terminal bytes. Raw terminal and native AskUser capture remain
separate required acceptance paths, not silently satisfied by composer support.

Schema 132 adds immutable `operator_submissions`, distinct from consumed-answer
receipts. Sources have their own typed ID, local operator and worker/session,
exact text and recorded time; no decision link or delivery claim is fabricated.
Admission allows 64 KiB UTF-8 text to accommodate the existing 16K UTF-16 composer
limit, with 4,096 rows, 16 MiB text payload and admission-time 90-day expiry.
Future decision dependencies must pin source evidence before depending on it.

`POST /api/v1/terminal/sessions/{session_id}/submissions` requires an explicit
operator bearer credential or browser session even on localhost. It does not use
the general loopback authorization exemption. This authenticates the operator
credential, not physical keyboard ownership on a compromised shared account.
Ordinary terminal endpoints retain their existing authorization behavior. The
response states authored source and unconfirmed provider consumption and is not
cacheable. No raw text is echoed or added to general activity events. UI recording,
source verification reads and consumption correlation remain integration steps.

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
