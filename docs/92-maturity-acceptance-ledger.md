# Current maturity acceptance ledger

Checkpoint: September 9, 2026. Scope remains [45](45-daily-driver-maturity-plan.md),
including BFG Admin integration. This is a routing index over evidence, not a
replacement specification or a declaration that partially verified rows are done.
Historical implementation notes in [47](47-maturity-remaining.md) remain evidence;
an old unchecked deployment note is not automatically current missing code.

## Verified runtime and delivery

- App/API: `1.6.0-dev-f476861e2d57-20260909072626-2380411`.
- Engine: retained PID 2271655, `cf83c980`; candidate `cf179f73` remains pending.
- Latest App/API deployment preserved all 34 worker records and twelve exact
  running session/conversation pairs. Swarm Next and D365 remained asleep.
- CI `34323789584` completed successfully. Subsequent support backend changes
  and the no-Hive image-reader fixture are not another runtime deployment.
- No release is authorized. No customer-facing send is authorized by task closure.

## Requirement-by-requirement disposition

No row labelled partial is a program-completion pass. Links identify the evidence
or implementation boundary to inspect before changing it again.

| Requirement | Established evidence | Remaining acceptance or implementation |
| --- | --- | --- |
| PERF-01 | Process attribution, one-hour mostly-idle server observation, bounded output bursts; [87](87-live-resource-and-restore-checkpoint.md). | Matched fresh/aged 10–15-worker interactive workload, tab CPU attribution, instrumentation overhead; slow navigation/event-entry tails remain unresolved. |
| PERF-02 | Bounded controller experiment and output handling; task reader now bounds/cancels preview reads. | Decide warm-pool policy from equivalent workloads; do not adopt it from a single good cold-return run. Prove resource plateau and no missing output under normal use. |
| DIAG-01 | Browser/server surfaces and bounded content-free capture exist. | Correlated fault attribution and consequence routing across browser/network/provider/server; unsupported measurements must remain unknown. |
| DOG-01 | Private revision-linked browser history and bounded Queen explicit-finish history; ADRs 0063/0091. | Review yield/returns, duplicate questions, recovery/false-alert and update convergence comparisons; finish history alone is not task productivity. |
| TERM-01 | Engine-owned generation fencing; exact sessions preserved across App/API updates. | Repeated real Android/iOS desktop handoff, suspension/reconnect and uncertain-input journeys. Desktop reload jumping is not fully accepted. |
| TERM-02 | Operator accepted Android native AskUser questions 2/3 on September 7; renderer regressions recorded in [47](47-maturity-remaining.md). | iOS acceptance and current-build repeated redraw/scroll/replay checks; selection correctness alone is insufficient. |
| MOB-01 | Existing composer, draft and failed-submission protections retained. | Real keyboard/dictation, suspension and immutable-session draft recovery on both mobile platforms. |
| MOB-02 | Attachment lifecycle fixes and tests recorded in [47](47-maturity-remaining.md). | Camera/gallery selection through shared-file receipt and provider reference acceptance on Android/iOS; desktop task-image previews do not prove this. |
| QUEEN-01 | Autonomous two-demo dependency journey and routine documentation settlement; [91](91-engine-return-dependency-acceptance.md). | Supported code/deployment settlement and worker-first escalation journeys; resolve actual evidence gaps rather than normalizing incomplete runs to success. |
| QUEEN-02 | Engagement and task-scoped recovery guards; bounded recovery delivery. | Demonstrate safe kicks and escalation without mid-task derailment across representative work; no real-project terminal experiments. |
| QUEEN-03 | Authenticated composer source records and exact-ID reads exist. | Direct-terminal/AskUser authored-answer capture and exact decision correlation remain unwired. Proposed source-to-deferral linking awaits the operator's answer. |
| QUEUE-01 | Owner grouping, worker grouping, structural prerequisites, exact held-briefing reasons, calendar reassessment journey. | Prove owner/reason consistency through remaining handoff/review/operator-resolution paths. Text-only deferrals without authoritative links remain a real evidence gap. |
| ATT-01 | Concise custom-answer UI and resolved/pending decision projection exist. | Direct worker answers must reconcile the same Needs You item without duplicate questions; depends on QUEEN-03. |
| ATT-02 | Runtime diagnostics/update/recovery entry is rendered; no new alerts for recovered evidence. | Complete fault-to-consequence and quiet-resolution acceptance, including normal-user feedback versus development repair. |
| PRES-01 | Schedule/DST/desktop-return policy and persistence tests exist; live locked desktop observed as Reachable. | Real scheduled/manual Night Watch, mobile non-dismissal and desktop dismissal. Live schedule is unset; do not change overnight policy merely for testing. |
| REC-01 | Planned engine replacement returned all twelve workers to exact conversations; [91](91-engine-return-dependency-acceptance.md). | Full chosen-conversation switch and missing-context safe/continue/fresh ladder with real provider evidence, plus failure/cancellation journeys. |
| REC-02 | Isolated corruption containment, backup/restore and package failure drills recorded in [87](87-live-resource-and-restore-checkpoint.md) and [47](47-maturity-remaining.md). | Retain this acceptance; no reason to corrupt or restore the live Hive. |
| OPS-01 | App/API continuity and one planned engine-return journey verified; resource admission guards exist. | Automatic engine-owned all-session admission, durable exact return set and trustworthy native completion/background evidence; ADR 0085 is not implemented. Rolling provider/tool freshness and pressure-to-resumption acceptance remain. |
| PROV-01 | Opt-in framework, host availability and Night Watch exclusions checked; [48](48-provider-acceptance.md). | Required provider journeys remain distinct from framework acceptance. Unavailable alpha CLIs stay unavailable; only the builder promotes providers. |
| UX-01/P6 | Approved visual direction retained; focused rendered queue, runtime, support and task-reader checks. | Coherent complete desktop/mobile accessibility, empty/error/offline and shortcut journeys. Optional return briefing still requires its mockup gate. |
| BFG Admin integration | Actual development-Hive text intake/receipt/reload retention accepted; [86](86-support-ui-acceptance.md). | Native attachments; existing fictional linked-task dispatch/closure and Admin-approved original-channel reply. Admin contract clarification is now active, without new fixture duplication. |
| P7 | Operator dogfooding plus bounded server/demo observations. | Integrated current-build workday/overnight/mobile evidence and operator acceptance. Narrow passing tests do not close this gate. |

## Next work, in dependency order

Admin's owner confirmed source revision `3a0f8bdbac5640776704af5c1922d679fab1d6c9`:
the public `/api/feedback/swarm-support/submissions` route is strict text-only JSON;
unknown attachment fields are rejected. The retained-email attachment route is
authenticated download-only and its reader token must never be placed in a Hive.
The Admin-owned bounded attachment contract has now been agreed in ADR 0080;
Admin implementation is in progress, with migration/storage permission review
required before deployment. Swarm schema 155 and its transport now preserve
reviewed file bytes and identity through the outbox. Authenticated local ingress,
review UI and browser retry-byte retention are now implemented and verified in
isolation; no live attachment route acceptance is claimed. The existing fictional request remains untouched and
dispatch still requires restored Admin sign-in.

1. Complete paired Admin verification and review its deployment/storage changes.
   Reuse the existing source and idempotent report identity. Keep the
   existing linked-task fixture; Admin authentication is an external acceptance gate.
2. Resolve the pending scoped operator-source linking proposal, then implement
   exact authored-source/decision reconciliation with no implicit authorization.
3. Establish provider-native settled-turn/background evidence before automatic
   engine admission. The pending opt-in prototype proposal is not approval to
   change existing workers, replace their execution mode, or trust a Stop hook alone.
4. Finish comparable browser workload attribution and whole-surface UI acceptance
   while the above external decisions are pending. Do not repeatedly rerun passed
   image or dependency fixtures as a substitute for these missing gates.
5. Run the real-device and integrated acceptance journeys against the delivered
   candidate. Leave unavailable-device and operator-approval gates explicit.

The program is active, not achieved. Its remaining scope cannot truthfully be
reduced to a release checklist or a count of passing commits.

## Native attachment backend checkpoint — September 9, not deployed

Schema 155 adds immutable ordered metadata and private file BLOBs in the same
transaction as the report. Existing text payload/destination/attempt identities
remain unchanged. Files and their metadata count against the existing 16 MiB
outbox cap; new work is refused at capacity without purging pending reports.
The parent manifest prevents missing rows from becoming a silent text-only send.
Confirmed local-copy removal cascades to its files, never central attachments.

The application saves reviewed files before network effects. The sole sender
uses the agreed multipart route for saved attachment reports, retains the same
IDs, hashes, bytes and manifest order across attempts, refuses redirects, and
never downgrades failures to text. Existing text-only routing is unchanged.
Admin remains responsible for bounded raster decoding, private object storage,
atomic publication and original-channel customer replies.

Isolated checks passed: two domain validation tests; all 695 persistence tests
before the additional independent-connection race test, which also passed;
19 focused API tests including HTTP multipart replay and process-owner uncertain
delivery recovery; four application boundary tests and strict all-target Clippy
across domain, persistence, application and API. The optional paired Admin test was explicitly ignored without
its fixture, not counted as acceptance. The local upload/review UI, populated
browser tests and real paired Admin attachment journey remain required.

### Local ingress and browser recovery checkpoint

The authenticated bounded multipart route and file review/retry UI are now wired,
not deployed. Four new ingress tests cover auth/admission before stalled-body
reads, exact atomic save/replay, invalid/duplicate/missing files and deadline
failure. All 23 focused API tests passed (the optional paired fixture remained
ignored); strict API all-target Clippy passed. The compiler first hit an incremental
fingerprint ICE; disabling incremental compilation completed the same checks
without changing unrelated source or deleting shared build caches.

All 25 focused browser tests pass, including bounded file-reader cancellation,
durable byte-copy recovery, cross-tab
overwrite/deletion refusal, storage failure before sending, explicit same-report
retry and refusal to clear an unconfirmed payload merely from a status-only key.
A broader run before the ArrayBuffer correction passed 150 files/1410 tests and
failed the two new recovery tests; the focused rerun after the correction passed.
Type checking and production build passed; the test-only IndexedDB implementation
adds no runtime dependency.

In Edge at 1465 x 1339 (document width also 1465), the real dialog retained both
fictional filenames and report text after a failed upload and full page reload.
Explicit retry reached Saved to Hive / waiting to send, not delivery confirmation.
A subsequent reload showed the empty form, confirming browser-copy cleanup.
The test used the no-proxy local harness and its fictional retry seed; it is not
proof of native camera/gallery selection, mobile layout, or a central Admin send.
The owned tab and harness server were closed after the check.

### Paired native file transport — September 9

The opt-in Rust acceptance test passed against BFG Admin's real routes/schema in
its loopback-only, memory-object-storage fixture. A new fictional text attachment
was frozen in the Hive outbox and accepted remotely. Dropping the Hive handles
before settlement, reopening SQLite and recovering the interrupted claim replayed
the same bytes and returned the same conversation, message and creation timestamp.
Changed bytes under that same submission key returned Conflict; the original
receipt remained replayable afterward. No reserved linked-task fixture, customer
data, production Admin state or email sends were involved.

The contract now explicitly requires nonempty files; domain and browser rejection
match Admin. Two focused domain tests and 15 focused web tests passed. This paired
text-file journey does not establish raster decoding, real camera/gallery input,
mobile layout, private operator downloads or production storage behavior.

Admin's owner supplied and root reviewed the bounded relay upload deadline and
persistent deletion-marker/conditional-create fence. Production schema 17-to-18
and feedback-only storage credential activation remain held for explicit operator
approval. The Swarm attachment tranche is not deployed while that gate is open.
