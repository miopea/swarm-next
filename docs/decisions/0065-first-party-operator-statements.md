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

September10 operator clarification: Queen may establish the exact applicability
link from a verified native answer to a pending decision using both full IDs and
the complete identical question/options snapshot, with a durable audit record.
Ambiguous applicability remains open. This grants no authority to manufacture
human provenance, substitute a worker's claim, infer permissions from wording,
or overwrite a resolved decision. It removes per-link operator confirmation only
within that exact verified boundary. Audit, authorization and resolution must
be implemented and tested before the agent-facing action is activated.

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

### Native interview observation contract, September 10

The isolated fictional Contract worker's retained native result confirms the
`questions` plus question-text-keyed `answers` shape for a three-question
AskUser interview. This is transcript evidence, not a captured live hook callback
or proof of terminal-writer identity. The current provider hook reference exposes
the invocation ID and tool-specific input/result on PreToolUse/PostToolUse.

`read_claude_interview` now parses those bounded main-session observations.
Requested and completed phases remain distinct: PreToolUse does not prove that
the question was displayed. Matching compares conversation, invocation ID and the
entire supported question shape, including option order, descriptions and
multi-select behavior. Unknown question/option features are unsupported rather
than discarded. Responses require every question exactly once; duplicate answer
keys, partial/extra answers, changed questions, failed/child events and nonempty
programmatic input answers are refused. Free text and joined multi-select text
stay exact; parsing does not infer which options were selected.

Transport stays capped at 64 KiB, each answer at 16 KiB, descriptions at 4 KiB,
invocation IDs at 128 ASCII bytes, and question count/text/labels at existing
domain bounds. Debug output contains no questions, answers, paths or invocation
ID. Observations are neither serializable receipts nor operator-authenticated
evidence. No hook is installed and no engine protocol or running worker changes
in this parser slice.

Next integration must authenticate the live process capability, order native
invocation boundaries against engine-owned input provenance, reject automation,
interrupted/uncertain input and replaced sessions, then bind an exact immutable
decision/question identity before confirmed receipt admission. Option descriptions
cannot be dropped to force a match against the narrower decision schema. A
successful tool result alone is insufficient: another hook may answer a tool.
Keep unsupported/ambiguous cases pending and do not invent a receipt from prose.
Only the complete fictional operator-to-worker-to-Queen test, including no second
delivery and API interruption, can close this milestone.

### Observed programmatic answer and output replacement, September 10

The decision question contract now retains optional `option_descriptions`, keyed
by the exact offered label, at most4096 UTF-8 bytes per description. Unknown
labels are invalid. Existing string options and absent descriptions remain
readable; no historical explanatory text is invented. Native question conversion
preserves labels, order, descriptions, wording, header and selection mode. It is
shape conversion only, not a decision-ID binding or an authenticated receipt.

Descriptions are stored in the existing question JSON and participate in exact
receipt comparison. Schema161 is a reader-compatibility fence: older code must
not open this database and ignore approval-relevant descriptions. No existing
question data is rewritten; downgrade requires a compatible backup. The agent
tool surface moves to revision25 so existing sessions report their stale schema.
Needs You displays descriptions with their options; changing a description clears
the form's answers/notes, while equivalent map ordering preserves a draft. Answer
payloads retain the option label or operator's custom text, not a concatenation
of the label and its explanation. Automatic native reconciliation remains gated.

The operator answer command now accepts the exact `questions` snapshot displayed
by the client. Application orchestration passes it to persistence, which invokes
the domain comparison inside the resolution transaction. Missing snapshots on
described questions, changed descriptions/labels/wording/order/mode, and unsupported
question fields cannot close a request or enqueue its reply. A mismatch returns
an explicit409 asking the operator to refresh/review; it does not automatically
replay input or wake the worker. Malformed stored questions remain an integrity
failure rather than being mislabeled as stale browser data.

The application decision boundary owns compatibility for omitted snapshots: only
questions with no nonempty option descriptions retain that legacy behavior. Remove
this omission branch when the minimum supported browser contract requires rendered
questions. New browsers always send their snapshot, including descriptions. This
is exact content correlation within authenticated operator resolution, not proof
of physical reading, provider consumption, or native terminal authorship.

A disposable native Claude Code 2.1.267 PTY exercised one fictional AskUser
invocation with explicit isolated hooks and no Hive settings changes. PreToolUse
supplied `Amber` through updated input. The observed PostToolUse callback retained
`Amber` in both input and response. A separate PostToolUse hook replaced the
result with `Blue`; the native UI displayed Blue and the model reported Blue.
Neither answer was human-authored. The reduced original callback is retained in
`crates/swarm-terminal/fixtures/claude-2.1.267-programmatic-interview.json`.
The existing parser refuses it because the input already contains answers.

This is a measured counterexample to treating PostToolUse as final consumption,
not evidence that normal human answers can yet be reconciled. Preserve the
distinction between tool completion, authenticated operator authorship and the
final result delivered to the worker. Installing capture hooks before that final
result is correlated would leave the integration incomplete even with durable
storage and a matching decision ID.

The callback probe also observed PostToolBatch with a `tool_calls` field. Its
original bounded recorder retained field names but not that field's contents;
no final-batch schema or consumption parser is accepted from this run. The probe
now records that field for a future explicit test. Two earlier headless attempts
advertised no tools and produced no callbacks; their model-written descriptions
are not execution evidence. Do not repeat those attempts or replace native PTYs
with headless workers. The bounded test process has exited.

Evidence directory: `/tmp/swarm-native-answer-contract.ssxOS0`. The reusable
probe is `scripts/dogfood/native-answer-contract-probe.cjs`; it deliberately
automates/replaces fictional answers and must never be installed in Hive settings.
Next: verify final-result correlation and full immutable decision binding, then
exercise genuine operator input plus API interruption and no duplicate delivery.

### Final batch counterexample and comparison, September 10

The next disposable native PTY run in
`/tmp/swarm-native-answer-contract.5ucwF0` captured PostToolBatch contents.
Claude Code2.1.267 returned Blue in its final string result for the same
invocation whose PostToolUse input/result contained programmatic Amber. The
batch input omitted those programmatic answers. Thus even the final batch cannot
authenticate authorship: earlier contamination must remain tied to the invocation.
The provider exited normally; no live Hive hooks were installed.

The reduced fictional callback is retained in
`crates/swarm-terminal/fixtures/claude-2.1.267-final-interview-batch.json`.
`matches_final_batch` compares an existing completed observation against the
same conversation, unique invocation, complete question and exact final string.
It cannot create an observation or receipt. Only the observed single-question
serialization is supported; multi-question final serialization remains unverified.
Changed context, duplicate invocation, child events, programmatic input and
oversize payloads refuse comparison. This adds no live capture caller.

Eight provider-interview tests, formatting and strict terminal-crate Clippy pass
in the isolated Linux verification tree. Exact immutable decision binding,
authenticated human input and multi-question final-result correlation remain
required before activation, followed by API-interruption/no-duplicate-delivery
acceptance. This comparison is not completion of QUEEN-03 or ATT-01.

### Three-question final batch observation, September 10

A further isolated native Claude Code2.1.267 run in
`/tmp/swarm-native-answer-contract.yt5S9Yco` asked three fictional questions
in one invocation. The probe supplied Amber for each; PostToolUse retained
Amber, while a replacement hook changed each to Blue. PostToolBatch contained
all three Blue answers in question order, and the native worker reported them.
The process exited normally; no Hive hook was installed. The reduced callback
is retained in the terminal fixtures as
`claude-2.1.267-three-question-final-batch.json`.

This supersedes the single-question-only comparison limit above. Comparison
checks every answer in declared question order. Multi-answer text containing
quotes, backslashes or control characters is refused because this unstructured
serialization cannot establish answer boundaries reliably. Full free-text
acceptance remains open and requires stronger final-result evidence; refusing
it is not fulfillment of that requirement.

Nine provider-interview tests and strict terminal-crate Clippy pass, including
changed answers, reordered questions and exactly matching ambiguous serialized
text. The comparison test uses a synthetic completion observation; neither that
nor the actual automated probe proves human authorship. Exact decision binding,
authentic input correlation and interruption/no-duplicate-delivery acceptance
still gate activation.

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

The composer now records the exact authored draft alongside Send after the first
terminal frame is accepted locally. This is not a provider acknowledgement.
Recording does not delay the Enter frame, append metadata to the prompt, retry
input, or clear a decision. Each mounted composer owns at most four source
requests with eight-second abort deadlines and cancels them on disposal. Capacity
or recording failure produces a local warning explicitly discouraging resend;
stale completions cannot overwrite the latest submission status. Nothing is
persisted in browser storage by source recording. [ADR 0069](0069-single-terminal-draft-continuity.md)
separately owns the single recoverable composer draft. A canceled request may already have committed on
the server, so absence of an acknowledgement is stated as unconfirmed, not lost.

`swarm_operator_submissions` provides a content-free index of the newest ten
retained submissions for one local-Hive worker, defaulting to the caller. A full
submission ID retrieves exact authored text and operator/worker/session/time.
Both selectors together, prefixes, and agent-supplied source text are refused.
Queen and other local workers share this read capability; it creates no evidence,
decision resolution, terminal delivery, or control-room invalidation. Reads exclude
sources older than 90 days even before admission-time pruning runs. The index
reports whether more records exist; older records currently require their exact ID.
Tool-surface revision 13 includes this tool and the statement selector added to
`swarm_list_decisions`; existing sessions need their normal schema refresh.

Verification reads return the statement and its scope, evidence status, and exact
decision link when present. A full ID is required; no prefix lookup. Verified
origin never expands the action authorized by the actual words. Preserve ADR 0054's
distinction between first-party evidence and an agent's relay.

## Ownership, bounds, and recovery gates

### Engine capture admission, September 10

The private protocol17 capture path authenticates the existing process capability,
live session and current selected conversation. A prepare/admit round trip orders
the requested native invocation against the engine's actual PTY write audit lock.
The helper must receive an opaque preparation ticket before admission. Input
between preparation and admission invalidates that preparation; a delayed callback
cannot claim input already received while its helper timed out. Exact retries do
not reset contamination. Conversation-selection revisions fence resume away and
back to the same conversation. Unsupported authenticated callbacks invalidate the
pending window, without replaying input or changing the provider conversation.

Only generation-checked operator writes qualify. Legacy actor labels, automation,
rejected or uncertain writes, interrupts, changed questions and input after the
last submit refuse evidence. The collector retains native questions and answers,
device identities and sequence boundaries, not keystroke text. It does not itself
prove exact decision identity or rule out every provider-side programmatic answer.

The engine owns at most32 prepared/pending observations and32 completed entries;
source payloads use the parser's64KiB bound. Admission failure does not block or
misreport an otherwise successful terminal write. Completed entries survive API
replacement and session stop until exact-ID acknowledgement, but not engine exit;
durable admission and acknowledgement remain application integration work. No
source content appears in ordinary terminal summaries, audit entries or Debug.

The helper shares one3-second deadline across stdin, protocol preflight, preparation
and admission, with no automatic replay. Unknown protocols receive only Ping.
Existing continuation recovery remains supported with a running protocol16 engine;
new capture requires17. The terminal host owns this compatibility floor until the
rolling-update support window for16 is explicitly retired.

### Durable native source boundary, September 10

The newer capture implementation holds PostToolUse results provisionally until
an authenticated PostToolBatch callback matches the exact invocation, current
conversation selection and complete final answer. Prepared, pending and
provisional entries share a32-slot limit; final sources retain the existing
32-slot limit. Any intervening input or invalidation discards provisional
evidence without affecting the PTY. Changed or unsupported final output cannot
enter durable intake through this implementation. No timer establishes completion.

This uses the existing bounded ProviderInterview transport; the helper's
PreToolUse handshake remains separate. Hooks are still disabled. Older engines
and previously stored source records do not gain final-consumption proof from
this change: activation must establish the exact capture implementation and
authentic human input before issuing confirmed receipts. Protocol17 alone is
not proof that a retained source passed the new final-result gate.


Schema 160 stores the engine's complete native question/result evidence privately,
including option descriptions, invocation and conversation identities, selection
revision, device identities and ordered write boundaries. Shared domain validation
rejects incomplete or unsupported source shapes. Storage admission verifies the
known local worker/session binding, including ended sessions with late retained
engine evidence. It does not authenticate provider-side human authorship, resolve
decisions, emit general activity or deliver another answer.

Admission is bounded to 4,096 records, 16 MiB total serialized UTF-8 payload and
128 KiB per source. Exact source-ID retries are idempotent; conflicting source or
invocation identities fail. Admission-time 90-day retention removes unreferenced
or closed-decision sources, never pending-decision references. The nullable link
is reserved for the forthcoming exact binding transaction, not agent assignment.
Exhaustion or failed persistence must leave the engine source unacknowledged;
ordinary terminal input remains independent. Downgrade requires a compatible
pre-migration database backup.

### Durable intake and acknowledgement ownership

Native evidence now carries optional typed final_result metadata. Only the
engine's successful final-batch comparison sets exact_batch. Older payloads
deserialize with no result, and serialize without adding a default field, so
their exact retry identity is preserved. Reusing an old source ID with a new
final-result claim is a conflict, not an upgrade. The metadata survives storage
and restart but is not a human-authorship claim or independently authenticatable
credential. Trusted engine intake remains mandatory. No historical evidence is
rewritten, no decision resolves, and no agent-facing write is added.


The private persistence boundary now supports explicitly binding a retained
source ID to a full decision ID. In one transaction it checks the local Hive,
worker/session identity, active session, pending decision and complete native
question shape against the decision snapshot, including descriptions and order.
It never searches for a decision by text. The existing nullable decision link
pins pending evidence during retention. Same-link retries are no-ops, including
after the original session ends; a different decision cannot replace that link.

This link is not human authentication, final-consumption confirmation or decision
resolution. It has no agent-facing route or production capture caller. Only the
future authenticated application correlation path may use it; semantic guesses
must not call it. No receipt, delivery or general activity is created. The
existing confirmed-resolution transaction remains the sole settlement path.


The existing API worker-supervisor pass collects at most32 native sources from
the trusted local terminal engine after reconciling known sessions. It checks
the actual protocol first; protocol16 and other unrecognized generations receive
only Ping, never capture commands. No additional polling service is introduced.
The application service admits each source independently and returns opaque
storage receipts only after a committed insert or exact retained-ID retry.
Only those receipts may cause an engine acknowledgement. Invalid/conflicting
sources cannot gain receipts or prevent valid siblings from being saved.

One shared admission permit covers the pass, including a blocking database job.
If cancellation outlives that job, the job retains the permit until completion,
preventing abandoned work from accumulating. The network/pass deadline is three
seconds; timing does not establish whether storage or acknowledgement happened.
Database failure stops that batch. Lost acknowledgements are unconfirmed rather
than proof the engine retained or removed anything; the durable source survives
either outcome. A replacement API safely retries the same source identity.
No question text, answer, capability or raw transport error enters warnings.
Ordinary terminal input and existing decision delivery remain separate.

This path is not installed in provider settings yet. Do not enable hooks or claim
Needs You reconciliation until authenticated durable consumption, exact question
binding and the complete fictional failure/recovery lifecycle are verified.

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
