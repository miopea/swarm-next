# Remaining maturity work — UX/UI first

## Operator priority override — Apiary onboarding, September 10

Latest execution: combined joining is committed cb17759c, with27 focused tests,
TypeScript and fictional Edge interaction verified. Temporary Keeper HTTP408/429
and5xx recovery classification is verified by11 transport tests, bounded-health
test and strict API checks. Neither package is deployed. WSL's original stopped
sync cause remains unknown; do not call it fixed or automatically clear it.
Next substantive implementation: ADR0096 public directory and owned profile
exchange, with independent-database tests and then Keeper/member browser checks.
Do not append fields to the existing signed catalog: old readers would reconstruct
different signature bytes. Keep directory failures separate from shared-work sync.

Operator subsequently approved broad onboarding improvements, explicitly including
the proposed Accept policy and join action. Preserve Keeper approval and server
readiness validation; acceptance alone must not join if readiness changes.
This supersedes the earlier pending product approval for the combined button.
Publication permission remains separate from this product-direction approval.

Pause native-answer integration; no runtime edits for its audit tranche have
started. Real member onboarding exposed a higher-priority user-facing gap:
invite generation, paste, Keeper approval, readiness, member confirmation and
Keeper finalization feel fragmented. Consolidate the journey and its outcome in
Apiary, preserving required consent/security checks rather than deleting them.
Automate readiness checks where safe and show one next action and its owner.

Explicit requested outcomes:
- A default My Hive becomes <firstname>'s Hive at join; preserve custom names.
- Show the actual person's name and email, not the generic Operator label.
- Propagate subsequent Hive renames (Vicky's rename remained local).
- Show all authorized Apiary member identities consistently; do not expose
  other Hives' private work, credentials or terminals through the roster.
- Investigate WSL's Runtime update required and catalog_stale signals against
  actual member/Keeper versions and responses. Screenshots alone do not prove
  which component requires an update or that WSL is the cause.
- Distinguish membership success from shared-work readiness, with a clear final
  joined state. Move low-level counters/details out of the primary journey.

Verify Keeper/member flows with fictional identities and an isolated member;
do not modify real memberships, Vicky's identity, or the WSL instance without
first identifying the authoritative target. Preserve the existing user-approved
native exact-link boundary for later. Main publication approval is separate.

## Active handoff — September 10, 14:15 Eastern

Use this section before the historical checkpoints below. Full scope remains45.

- **Local package ready for publication review:** task-title keyboard/click access;
  accurate Apiary navigation/departure guidance; bounded invitation reads and
  mutation waits; truthful confirmed-versus-uncertain recovery. Commits64922011
  throughb45c87c0. Full web tests and production build passed atb45c87c0; an
  additional restored real-fetch regression and TypeScript pass afterward.
- **Publication requires the unanswered explicit approval:** do not retry the
  previously refused main push or deploy around it. Last fetch found no incoming
  main commits. Main/live App/API6ac6c145; live engine30ad01d,protocol17,
 13 running/retained,0 unreadable,no degraded flags. Do not restart the engine
  just to make version labels match.
- **CI correction ready locally:**32dc50ee fixes the schema-ceiling fixture,
  not an observed live data-loss bug. Rerun CI after approved publication.
- **Do not spend another tranche polishing invitation copy or repeating these
  passed fixtures.** After publication, verify the live title and invitation
  recovery surfaces without creating actual invitations/access grants.
- **Next substantive open UX behavior:** direct native answers still do not
  reconcile Needs You. ADR0065 requires exact decision identity, final result
  and authenticated input. Three-question final serialization is now observed
  and comparison passes9 tests plus strict Clippy; ambiguous free text remains
  unsupported. This is not human provenance or completed reconciliation.
  Local engine capture now waits for authenticated matching final output before
  intake (170 terminal tests pass;1 ignored). Older stored evidence/engines do
  not gain this proof. No hooks or engine changes have been deployed.
  Private exact-decision source pinning now has transactional tests; it does not
  resolve or authenticate an answer. Next wire authenticated final consumption
  and exact invocation/decision correlation through existing receipt settlement.
  Operator approved Queen establishing an audited exact-match/full-ID link;
  ambiguous applicability stays open. Implement Queen authorization and audit,
  then the existing confirmed-receipt resolution transaction and no-duplicate
  delivery verification. Do not substitute semantic matching or source text
  alone for human provenance. Durable final-result metadata0607305c keeps older
  sources unchecked. Publication approval remains separate and unanswered.
  Hooks remain disabled. No safe-recovery or permission rule may be bypassed.
- **Separate external gates:** optional return-briefing visual approval;
  real Android/iOS picker,keyboard/suspension/handoff and actual browser zoom.
  Preserve existing successful Android AskUser/task-editor acceptance.
- **Backend scope remains open:** automatic engine admission,chosen-conversation
  recovery,Queen release/recovery loop,sustained performance,metrics and the
  linked Admin attachment/reply lifecycle. No BFG Admin communication without
  fresh approval. No release.

The broader goal is not complete. Ledger92 and journey matrix95 hold evidence;
older pending/deployed/version paragraphs below are historical,not current state.

Latest checkpoint: App/API6ac6c145 is live; engine30ad01d has the same engine
fingerprint and13 healthy running/retained sessions. Older staged-migration notes
below are historical. Settings finish evidence is in94. CI's schema-ceiling
fixture fix32dc50ee is local pending main-push permission, not a live defect fix.
Native final-batch comparison passes9 tests and strict Clippy but remains disabled:
full free-text final results, provenance and exact decision binding are still
required. Do not promote this partial result to QUEEN-03/ATT-01 acceptance.

### Active acceptance checkpoint — ordinary-user UX first

Operator applied the staged migration September10 at12:54 Eastern. Health and
both active links now confirm `1.7.0-dev-30ad01dc55be-20260910164740-555599`.
Engine PID565859 reports protocol17,13 running/retained sessions,not draining,
and0 unreadable sessions. Edge's live roster also shows13 active workers.
This proves runtime activation and running count, not exact prior-conversation
identity: four existing conversation-history-unconfirmed notices remain visible.
The legacy activation left its manual marker after removing pending; its exact
candidate matched both active links before that obsolete marker was removed.
The corrected installed helper is now active; reset-failed/start restored only
the development build watcher, with no second app/worker restart. Local follow-up
cleanup commit `b152b739` remains outside the staged/deployed candidate.
Live Settings displays the saved22:00–07:00 America/New_York schedule; it was not
edited. Fictional Needs You free-text submission preserved the exact typed answer.
The full program remains open, including direct-answer decision reconciliation.

September 10 current checkpoint (supersedes the historical incident notes below):
main and the Linux checkout are `30ad01dc55be`. The authorized transient service
`swarm-authorized-preparation-30ad01dc` completed with exit 0 and development
status `deferred`. Candidate
`1.7.0-dev-30ad01dc55be-20260910164740-555599` is staged under the managed releases
root; pending and manual-hold markers both name it. Both active links remain on
`1.6.0-dev-dba2868be982-20260910095752-189394`; health is OK and engine PID242380
is unchanged. This is preparation, not deployment or worker-return acceptance.
Explicit permission to apply the warned worker-interrupting migration was asked.
The old installed watcher remains failed; do not reset it against its old helper.

Exact prepared-version consent now passes API refusal tests and strict API
Clippy. Post-merge UI verification passed123 tests plus TypeScript. Edge's
fictional preparation journey verified Cancel leaves the action untouched and
confirmation reaches deferred, not installed;390px visual proof is retained in
`artifacts/protocol-preparation/phone-confirmation.png`. No native-device claim.
A follow-up isolated lifecycle regression passes for clearing both pending and
manual markers when the host already speaks the prepared protocol. This cleanup
is not in the immutable staged candidate and must not mutate that candidate.

### Historical implementation checkpoints

The explicit preparation journey is now wired locally: a separate authenticated
`POST /api/v1/runtime/development/prepare` records `operation=prepare-protocol`.
The existing service consumes that operation and invokes preparation after a
successful source-stable build; absent operation remains an ordinary reload.
Settings offers separate staging confirmation without granting activation.
The full packaging lifecycle test covers request -> build -> deferred preparation
with no service/engine calls; endpoint authentication/deduplication and original
reload compatibility pass. All54 Settings/update tests, TypeScript, production
web build and strict API all-target/all-feature clippy pass. Evidence is in
`/tmp/swarm-native-answer-check.6fkv8E/development-{preparation*,reload-compat.log}`.
Actual browser acceptance, exact-version apply consent and worker return remain
open. The operator authorized triggering the update once the fix is installed;
no live update has been triggered yet. The live watcher remained failed at the
12:05 check, with the11:52 request queued and no build process; old app healthy.

Explicit migration staging is now local commit `5de86971`: `prepare-protocol`
validates/installs one pending package under the lifecycle lock without touching
services, engine input or active links. A manual hold prevents timer activation
even with no loaded sessions. Different pending packages refuse; exact retries
are safe; stale managed maintenance requests cannot activate the candidate.
The complete isolated packaging lifecycle suite passes, including these cases
and lifecycle lock contention. Log:
`/tmp/swarm-native-answer-check.6fkv8E/protocol-preparation-lifecycle.log`.
No live preparation or deployment occurred. Next wire the development build
request and UI to preparation, bind maintenance consent to the exact prepared
version before stopping workers, then verify explicit application/return. This
is not yet an operator-usable prepare-and-apply workflow; do not claim OPS-01.

September 10 development-update incident: the live checkout `0e35d90b` requires
protocol17 while the running host remains16. The App/API is healthy at
`1.6.0-dev-dba2868be982-20260910095752-189394`; host PID242380 still has its
07:25:33 Eastern start. The development reload rejected before compilation, but
left its request file present, causing immediate PathExists retries and ultimately
`unit-start-limit-hit`. Local packaging now consumes the validated request before
protocol preflight. The complete isolated package lifecycle test passes, including
new assertions that refusal consumes the request, never runs the builder, and
makes no service calls. Log: `/tmp/swarm-native-answer-check.6fkv8E/protocol-refusal-lifecycle.log`.
Not deployed; the failed live watcher has not been reset or retried.

The user-facing flow remains open: installer writes `step=protocol-change` but
the API reads `reason=`; the failed card also hides protocol-specific guidance.
The local card now uses the independently observed current protocol mismatch to
show "Worker engine migration required", explicitly says no migration was prepared,
and removes the ineffective retry. All25 component tests pass; TypeScript passes
after correcting the new fixture's explicit runtime type. Clearing the mismatch
restores ordinary failure recovery. This is a diagnostic correction, not the
missing preparation action; browser acceptance and deployment remain open.
The engine-update indicator compares the running API/engine, not the newer
checkout. Implement and test an explicit preparation-to-maintenance journey;
do not make a plain retry claim to prepare a migration or silently restart workers.
The operator separately reports that clients upgraded to1.7.0 smoothly. That is
client-upgrade acceptance, not proof of the development protocol17 migration.

The in-progress exact-question package now preserves option descriptions in the
shared decision format, JSON persistence, native conversion, agent schema25 and
Needs You. Schema161 fences older database readers that would ignore conditions.
The current form clears drafts on changed descriptions but preserves equivalent
map order. Edge fictional desktop and390px phone checks show readable descriptions,
no horizontal overflow,44px minimum option buttons, and exact label submission.
Screenshots: `artifacts/decision-descriptions/{desktop-light,phone-light}.png`.
Those screenshots precede a final CSS-only change making description text inherit
the option's font size for readability. Edge disconnected during the replacement
capture; final-font visual verification and clearing its temporary390px viewport
override remain pending. Do not call this native-device acceptance.
All63 focused UI tests, TypeScript and production web build pass. Full domain153,
affected persistence54 and served tool-contract1 pass; the initial fingerprint
mismatch was expected and corrected with the observed revision25 fingerprint.
Strict domain/persistence/API all-target, all-feature clippy now passes after
replacing the four ambiguous default constructors. Linux logs are
`/tmp/swarm-native-answer-check.6fkv8E/description-{domain,persistence,surface-verified,clippy-final}.log`.

The stale-browser gate is now implemented: the browser sends its rendered question
snapshot, and the domain comparison runs inside the answer transaction. Missing
rich-question snapshots or changed context return409 without resolution or reply;
unsupported question fields are rejected rather than discarded. The full-App and
client tests verify snapshot submission and no automatic retry. Plain legacy
questions remain supported. Domain154, affected persistence54 and two HTTP route
tests pass; the strict unknown-field refinement was rechecked with all154 domain
tests and the described-answer HTTP test. Strict all-target domain/persistence/
application/API clippy, TypeScript and the production web build pass. Logs:
`/tmp/swarm-native-answer-check.6fkv8E/snapshot-*.log`.

Next finish immutable decision-ID binding and final native-result confirmation;
provider capture remains disabled until the complete fictional lifecycle passes.
Final-font Edge verification is still pending reconnection, and main publication
remains approval-gated. No deployment or worker interruption; the frozen1.7.0
candidate is unaffected. Account usage42% consumed at this checkpoint.

Native Claude 2.1.267 contract probe now demonstrates that a PostToolUse observer
can retain a different answer from the final result received by the worker:
programmatic Amber became Blue through another hook. The current parser rejects
the observed programmatic callback correctly; preserve that actual fixture as a
regression. This is not genuine human-answer acceptance. PostToolBatch fired with
`tool_calls`, but its contents were not captured in the first probe; no final
consumption parser is verified yet. Details and evidence location are in ADR0065.
Do not enable automatic Needs You closure from the early callback. Next integration
must correlate final received output, authenticated input and full decision identity.
The disposable native process exited; no additional provider run is necessary to
establish the early-callback limitation. Release1.7.0 remains untouched.
All seven focused native-interview parser tests and strict terminal all-target,
all-feature clippy passed; probe JavaScript syntax and Git whitespace checks pass.
Logs: `/tmp/swarm-native-answer-check.6fkv8E/native-contract-{tests,clippy}.log`.

Durable intake is now wired into the existing supervisor after session binding.
The application returns acknowledgement identities only for committed or exact
duplicate sources; the API never acknowledges rejected/unsaved entries. One
shared permit and a three-second pass budget bound collection and its blocking
save job. Protocol16 receives only Ping. No new polling loop, terminal input,
decision resolution or provider hook installation is introduced.
Seven new application/API tests pass for save-before-acknowledge, lost reply and
API replacement, invalid siblings, oversized batches, unsupported protocol and
bounded concurrent/hung reads. Eight targeted supervisor/recovery/real-PTY shutdown
tests pass; strict application/API all-target clippy and formatting pass.
Logs: `/tmp/swarm-native-answer-check.6fkv8E/native-intake-{clippy,tests,supervisor}.log`.
Native-source persistence is local commit `7a6d963c` on
`codex/native-answer-storage`; pushing main was refused by the execution gate.
The operator has been asked for approval; do not retry main or deploy around that
gate. Continue local integration meanwhile. Exact decision identity, native
authorship/programmatic-answer exclusion, actual hook installation and the full
fictional operator-to-worker-to-Queen lifecycle remain open. Needs You has not
been claimed fixed by these intake tests.

September 10 release handoff: operator approved preparing 1.7.0, not publication
or deployment. Built and signed candidate `9ae7a0e3` is based on verified UX
`dba2868b`, excluding the unfinished native capture/storage work. Handoff is at
`/home/bschleifer/releases/swarm/1.7.0-candidate-9ae7a0e3/README.md`; independent
signature/checksum and isolated packaged-runtime smoke passed. Eight curated
highlights and sixteen fixes have desktop/phone component previews. Another
worker is to verify install/upgrade and publish/deploy under operator direction.
Do not mutate that immutable candidate when continuing this integration.

Native-source storage now has schema160 and shared domain question validation.
Nine focused tests pass for exact preservation/idempotency, conflicting evidence,
invalid/partial input, ended-session ingestion, migration/restart, save failure,
row/UTF-8 byte limits, expiry, pending-decision pinning and private corruption
reporting. The first full run passed151 domain and724 persistence tests but found
24 migration failures: the new migration omitted its schema-version update.
That defect is corrected; all99 selected storage/upgrade/restart/migration tests
now pass, covering every previously failed test. Strict all-target
domain/persistence/terminal clippy passed. Full terminal regression passed165 with
one opt-in profiler ignored; API/terminal-host consumer checks and formatting pass.
The first full persistence failure log is retained alongside the targeted corrected
run; a second all748-test green run is not claimed. All nine native-source tests
are included in the99 passing correction checks.
the Linux validation tree is `/tmp/swarm-native-answer-check.6fkv8E`, now with its
own stripped-debug cache (1.0 GiB observed, 8.2 GiB root space free). The older
attachment debug cache was explicitly removed during release preparation; do not
assume it is still warm. No live schema migration or provider hook installation.
Durable admission/acknowledgement is implemented in the intake checkpoint above.
Next is exact immutable decision binding and native authorship verification,
preserving descriptions and ruling out programmatic answers.
Account usage39% consumed at this checkpoint; no resets used or available.

Native capture foundation now has a private protocol17 prepare/admit handshake,
engine-ordered generation-checked input, exact conversation revision fencing and
bounded read/ack evidence. Linux full terminal165 passed/1 opt-in profiler ignored,
host library27 and host command11 passed; strict terminal/host all-target clippy
passed. These include delayed admission after operator input, stale generation,
legacy actor forgery, automation/uncertainty, duplicate callbacks, resume boundaries,
dead processes, API-reader replacement and successful-input/capture-failure isolation.
Source verification ran in /tmp/swarm-native-answer-check.6fkv8E using the existing
bounded test cache. No hooks installed, engine update, live deployment or release.
API cargo check passed. Protocol16 retains native continuation recovery and the
helper's existing startup/resume callbacks; only native capture requires17.
The final helper compatibility change passed all7 provider-session-start tests
(including real socket exchanges against16 and17) and strict all-target clippy.

Next: durable native-source admission and exact immutable decision/question binding,
including full option descriptions (never discard them to force a match). Connect
the existing consumed-answer resolution transaction only after authenticating the
native source and checking the complete fictional lifecycle. Missing, ambiguous or
provider-programmatic evidence stays unconfirmed. Engine retained evidence is not
yet a durable statement or Needs You closure. Do not restart the completed UI sweep.
The developer-release candidate remains deployed dba2868b, excluding this unfinished
integration; publishing still needs explicit operator authorization. Usage38% used
at this checkpoint, no resets consumed.

Direct-answer integration is now the sole next package; do not repeat the web
checkpoint below. Verified live dba2868b healthy, engine PID3408834/startSep9
17:01:49EDT unchanged. The fictional Contract transcript has exact three-question
native answers. Added the bounded native interview observation parser described
in ADR0065; full Linux terminal library148 passed, one opt-in profiler ignored.
Final annotation/unknown-result guards passed all6 parser tests and strict
terminal all-target clippy. Nonempty annotations remain unsupported, not dropped.
This is NOT installed hook capture, authenticated authorship, decision settlement,
or closure of QUEEN-03/ATT-01. No deployment or engine update in this slice.
Continue with engine-owned invocation/input provenance and exact decision binding,
then persistence/service integration and a fictional complete lifecycle. Do not
wire a native result directly to a confirmed operator receipt, drop option
descriptions to force a match, or substitute worker prose/timestamps for evidence.
Isolated Linux source:/tmp/swarm-native-answer-check.6fkv8E; reused existing
target:/tmp/swarm-attachment-check.EBT6ei/target. Keep that cache bounded; disk3.2GB
free at entry. User-account usage37% consumed; no resets used or available.

UI-first browser implementation checkpoint: full current web suite passed1540
tests/158 files at ed51204c; deployed dba2868b CI34463462108 is green. Ordinary
presentation/recovery increments are ready for operator dogfooding, not full45
completion. Do not continue polishing already-passed tiny UI cases.
Actual browser zoom remains manual: supported CUA Control+plus did not change
viewport1438x959, DPR1.5 or visual scale1; this is not zoom acceptance. Narrow
reflow has separate320px evidence. Do not spend another loop trying substitutes.
Next highest user-facing gap: QUEEN-03/ATT-01 direct worker answers leaving stale
Needs You. Inspect authenticated operator-submission/native hook provenance and
exact decision linkage before implementing; worker prose alone remains insufficient.
This is source-state recovery for an existing UI pain, not a return to broad CPU
profiling or unrelated engine updates. No BFG worker contact or release authorized.

Dark-theme/reflow checkpoint: fixture presentation preferences were empty, so a
dark button label could coexist with light CSS. The harness now owns valid per-
device preferences and PUT updates, with explicit theme=dark seeding. Verified
actual root theme/colors before accepting screenshots. Dark Needs You, Queues,
custom answer, task editor and Settings search fit320px. See95 for sampled ratios.
This tranche changes only fixture/evidence; production remains dba2868b, so no
app/API or engine update is necessary. Native browser zoom is not claimed from
viewport emulation. Next close the remaining zoom/reflow gate using a supported
browser zoom action, or explicitly retain that gate if the browser cannot expose
it. Then consolidate UI-3/4 disposition without reopening passed journeys.

Delivered dba2868b: linked task/decision navigation respects reduced motion instead
of always requesting smooth scroll.116 focused tests/build pass; Edge emulated
reduce preference verified both exact-item focus journeys. Healthy app/API and
all34 records/12 running identities preserved; enginePID/start unchanged. See95.
Sampled light-theme contrast: muted rail4.89:1, primary panel13.35:1; these samples
are not whole-app contrast acceptance. Next finish representative dark-theme and
zoom/reflow checks, then consolidate the UI-first checkpoint against96, keeping
native-device/backend gaps explicit. Do not repeat passed history/search tests.

Delivered b20e2e22 preserves searchable decisions but distinguishes Attention,
Waiting for reply and Decision history.132 focused tests/build pass;390px fixture
and answered-history focus verified. Healthy app/API; all34 records/12 running
identities and enginePID3408834/start unchanged. See95 for deployment receipt.
Next UI-4 gate: reduced-motion navigation, contrast and zoom. Inspect explicit JS
scrollIntoView({behavior:"smooth"}) against reduced-motion preferences; CSS alone
does not prove that imperative navigation respects the preference. Do not reopen
passed search/history checks. Full45 scope and native-device gates remain intact.

Delivered fd0385d2 fixes the rendered missing Queues command;83 tests/build and
desktop keyboard/320px acceptance pass. Live Queues command shows55 waiting,
matching the sidebar; health and all34 records/12 running identities preserved.
Next concrete finding: commandChoices maps every decision into Attention with
only reason text, so historical requests look like current questions. Live search
for Queues showed three Attention results while Needs You counted2 pending.
Inspect exact decision states and distinguish/filter historical results; preserve
searchability deliberately and do not mutate or answer decisions to fix the view.
The consolidated user-facing milestone map is in96. Next close explicit remaining
UI-4 reduced-motion/contrast/zoom browser gates, without repeating native phone
questions or deep performance work. Full45 scope remains active.

Current package2d7b1cd7 closes the missing queue-observation retry: one bounded
GET through the existing owner, no dispatch/restart or task changes.124 targeted
App/Queues/read-owner tests and build pass.390/320px browser checks retain the
unavailable warning after retry; the44px button fits without horizontal overflow.
Delivered healthy runtime2d7b1cd7: all34 records/12 running identities unchanged,
enginePID3408834/start unchanged. See95 for deployment receipt.
Prior83a88edd display-recovery deployment retained all34 records/12 running
identities and enginePID3408834/start. CI34458764563 needs its completion check.
Next: reconcile the UI-1 through UI-4 acceptance matrix against retained receipts
and close any remaining ordinary-user browser journey, rather than repeating
passed checks. Keep native Android/iOS and optional briefing approval separate.
Deep performance and orchestration remain in45 but are not this UI-first tranche.

### Historical package checkpoints (newer receipts above supersede pending notes)

DELIVERED1e410476: healthy app/API, all34 worker records/12 running identities
preserved, enginePID3408834/start unchanged. See95. CI34456655548 is pending.
Do not redo update confirmation/cancellation: existing a716dad8 desktop/phone
evidence below covers that unchanged path. Next inspect remaining UI-4 empty/
offline/error journeys against retained receipts; close only genuinely unmet
browser gates. Support native pickers and paired Admin acceptance remain open,
but do not contact Admin or repeat this passing fictional support tranche.

Current package1e410476: saved diagnostic bundles can be previewed exactly as
stored, and clipboard refusal exposes the bundle plus recovery guidance instead
of silently failing.45 focused tests/build and390/320px fictional browser checks
pass. Support attachment lost-response/exact retry is also browser-verified at
390px; see95. App/API deployment requested, continuity receipt pending. Previous
d69e9b16 CI34454887235 is green. Do not rerun these passing suites; next close
remaining UI-3 update confirmation/cancellation and UI-4 empty/offline journeys.
No BFG contact, central test send, release, or engine-update authorization added.

DELIVERED d69e9b16: healthy app/API, exact34-record/12-running identity match,
enginePID3408834/start unchanged; see95. No fixture server remains. CI34454887235
is the specific pending run. No release or engine update. Next: remaining UI-3
support progress/errors/diagnostic-preview and UI-4 empty/offline/error journeys;
do not repeat this passed prerequisite/history/schedule responsive tranche.
Native-device gates and optional briefing approval remain separate, not blockers
to safe ordinary-user work. Keep the full45 objective intact.

Newest product package: d69e9b16 fixes the phone prerequisite-confirmation overlap
found during responsive acceptance.118 focused tests/build pass; actual320px and
desktop geometry confirms the footer no longer intersects the form. History at
390px and Night Watch controls at320px were also inspected; see95. Deployment is
in progress through the existing app/API reload service; do not restart it merely
because a check still shows the old version. Verify live revision, worker snapshot
and enginePID/start before claiming delivered.80c7d1c4 CI is green. Usage33% used.
The optional briefing preview remains awaiting visual approval; not installed.

Current continuation: operator has locked the computer and permits overnight live
Hive verification. No further approval is needed for scoped app/API deployment;
preserve workers, do not cut a release, and do not contact BFG Admin. The optional
return briefing now has a fictional conversation preview at the thread-owned
visualizations directory, `hive-return-briefing.html`. This closes mock preparation
only, NOT visual approval or implementation. No live landing behavior changed.
Default preview omits ordinary moving work and offers return to the previous
worker or the exact Needs you decision. Await operator design review without
blocking other work. Older "Next" paragraphs below are historical, superseded
when the package they describe appears as delivered above them.

Integrated web gate at80c7d1c4: all1,525 tests in157 files pass. Production build
passes; live health/worker continuity and browser bundle match are verified.
Account usage32% consumed/68% remaining; no resets available or used. Do not
repeat this unchanged full web suite in the next turn. Update the UI acceptance
matrix from real current evidence and select the next remaining gate in45/93.

LATEST: prerequisite recovery80c7d1c4 is delivered and healthy. The failing-before
locked-dialog regression now passes; finite client wait, unmount/late-response
fences and honest unconfirmed-close semantics are browser-verified in a fictional
Hive.88 focused tests, final12 tests/build pass;34 workers/12 running identities
and enginePID3408834/start preserved. See92/95. CI34452563704 is pending; all prior
product CIs throughcb915ab1 are green. No fixture server remains; dedicated live
tab is Settings with the new browser bundle. No real task mutation or release.
Next: complete the integrated UI-4 acceptance matrix/checklist in95/96 and inspect
remaining ordinary-user gaps against actual requirements, not another replay of
these passed tests. Optional While you were away remains a mockup approval gate,
not permission to install a modal. Device/provider/backend gates remain open.

Next verified code gap: TaskPrerequisiteDialog.submit awaits an unbounded
changeTaskPrerequisite request while requestClose refuses closing and all controls
remain disabled. Inspect/add the API cancellation signal and owned finite save
deadline, fence unmounted/late responses and preserve explicit uncertainty (the
server may have committed). Never automatically replay a mutation. Add focused
timeout/unmount/recovery tests and a fictional browser proof before deployment.
This is UI-4 linked-task failure recovery, not an orchestration-policy rewrite.

The concise operator checklist and remaining release risks now live in96. It
does not close the overall goal or request another AskUser/task-editor test.
Current product remains cb915ab1 with verified unchanged schedule/worker sessions.
Next implementation direction: remaining user-facing linked-task/prerequisite
failure and recovery journey coverage, then the final integrated UI matrix. Keep
native device and backend gaps explicit; no performance/BFG detour. Dedicated
live tab is in Settings with night watch search; no unsaved live form or fixture
server remains. CI34450368822 is the one pending product check for this package.

LATEST after Night Watch UI package: cb915ab1 is live, healthy and browser-checked.
Saved schedule versus draft and definitive validation versus network uncertainty
are now explicit.48 tests/build pass. Exact configuration and34-worker/12-running
identity comparisons unchanged; no engine update/release. Live Settings search
for night watch exposes schedule and Queen autonomy together. CI34450368822 needs
completion check; prior historyCI34449170913 is green. No fixture server remains.
Next: consolidate the actual UI-1/2/3/4 acceptance gates into a short operator
dogfood checklist and release-risk summary, then address remaining reproducible
ordinary-user journey gaps. Do not mistake that artifact for overall completion.
Keep physical Android/iOS picker, suspension/handoff and presence gates explicit;
do not change the live Night Watch configuration to prove them. No BFG contact.

LATEST September10 03:25EDT: bounded older/latest task historycb7d4206 is live,
healthy, verified in Edge and authenticated live cursor reads. Exact34-worker/
12-running projections and enginePID3408834/start preserved. See92/95; desktop
history journey is delivered, narrow/native paging remains explicit. CI34449170913
is running; cancellationCI34447225894 is green. Account usage31% used/69% left.
No fixture servers or Rust tests remain running; no release/engine update.
Next bounded package: finish UI-3 presence/Night Watch Settings journey using
fictional configuration changes only. Dedicated live tab is on Settings. Existing
NightWatchSettings has load protection, save timeout/retry and three tests; inspect
actual rendered hierarchy, schedule/manual/autonomy explanation and remaining
failure/focus behavior before changing it. Do not change the live schedule or
repeat the delivered history/clarification gates. No BFG Admin communication.

Current delivery checkpoint: history cancellation444f7a56 is pushed and live;
healthy runtime `1.6.0-dev-444f7a565411-20260910065313-92358`. Exact34-worker,
12-running identity match and unchanged enginePID3408834/start verified. No
engine update/release. Local harness stopped. CI still needs its completion
check. Next: bounded older-history navigation, not another cancellation retest.
Persistence currently selects newest limit+1 by sequence then reverses; HTTP
TaskActivityQuery exposes only limit. Additive cursor pagination can reuse that
ownership, keeping one visible page and Latest/Older controls. Read relevant
contracts and add failure/recovery tests before implementing; not yet started.

UI-4 continuation: see95 for the current journey matrix. Live prerequisite ->
exact task -> history passed. A failing-before regression proved reopened history
could be overwritten by an older response. The existing bounded read owner now
cancels hidden/unmounted history and fences late responses, with a stable App
callback and API cancellation propagation.146 focused tests passed; final typed
fixtures,69 task/API tests and production build pass. Fictional browser history
close/reopen passes. Commit/deploy this coherent change, verify live and preserve
worker identities. Next inspect bounded older-history access: latest30-only is
not full history on demand. Do not detour into performance or BFG communication.
All prior clarification/mobile/diagnostic CI runs are now fully green.

LATEST: the clarification milestone and two bounded UI follow-throughs are
committed/pushed/deployed through `b01aef1d`. Live runtime:
`1.6.0-dev-b01aef1d1c9b-20260910061835-74026`. All three deployments preserved
the enginePID/start and exact34-worker/12-running-session identity projections.
No release, real decision answer or BFG communication occurred. Native fixture
units, SSH forward and local UI fixture servers are stopped; the separate live
Edge tab remains on diagnostics with its viewport restored.

Clarification CI34442276338 is fully green. Follow-through CI34443735026 and
34444605660 have separate final gates; inspect their status once at the next
package boundary, not in a tight loop. Local gates and live browser acceptance
are recorded in92/94. Next: finish the UI-4 linked task/dependency/history and
navigation journey matrix, using fictional mutations only. The direct live
operator-queue -> exact decision focus path passed. Keep Queen orchestration,
direct-terminal reconciliation, provider/device and sustained performance gaps
explicit; do not reopen passed native clarification tests or contact BFG Admin.

Current checkpoint after closure: clarification is live with full CI green.
Mobile runtime navigation follow-through `fd35ca3e` is also live at
`1.6.0-dev-fd35ca3e7f17-20260910060616-63692`, with71 App tests, build, fictional
and live390px browser acceptance. Engine and34/12 worker identities preserved.
CI34443735026 is pending. Local harness stopped, viewport restored; no release.
Diagnostic follow-through is now locally verified: the capacity line no longer
repeats the machine aggregate as an all-clear; the existing assessment owns the
summary across CPU and process evidence. Detailed verdicts and thresholds are
unchanged. All31 diagnostic/report/assessment tests, production build and a
fictional390px CPU-pressure browser journey pass. Publish/deploy this change,
record its live receipt, then continue UI-4 linked task/queue navigation journeys.
Do not start broad performance work or rerun
the already-passed native clarification/backend gates. Account29% used/71%
remaining at this boundary. See92 for exact receipt and observed inconsistency.

September10 deployment checkpoint: the clarification vertical slice is live at
`1.6.0-dev-f05f7f217a7c-20260910054522-50600`. Both native requester journeys,
failure/recovery gates, full backend/web tests and live editor checks passed.
All34 worker records and12 running session/provider identities were preserved;
enginePID3408834/start unchanged. The pending engine update was NOT applied.
See94 for exact receipts and remaining device/tool-refresh boundaries. This
supersedes the historical undeployed checkpoints below. CI34442276338 passed all
four jobs. Next: UI-3/UI-4 Settings and whole-product journey consistency.
Live390px testing found System stays expanded after navigating to Diagnostics,
consuming most of the screen. The navigation owner now collapses it; inline
details/retries remain open. Verify and deploy this bounded UI follow-through.
Do not restart broad backend tests,
reopen reload-only performance work, contact BFG Admin or cut a release.

Final pre-deployment gate:561 API tests passed (three opt-in tests ignored),
48 application tests passed,739 persistence tests passed;1512 web tests and the
current production web build passed. The first broad API run exposed an existing
five-second fixture lifetime; the corrected reattachment fixture is owned until
explicit stop and passes in the combined suite. Both isolated native services,
their browser tab and temporary SSH forward are stopped; evidence is preserved.
Deploy app/API only. Baseline receipt directory:
`/tmp/swarm-clarification-deploy.cvm5VI` —34 worker records,12 running; production
enginePID3408834, startedSeptember9 at17:01:49EDT. Compare exact identities after
deployment; do not claim conversation continuity from that projection alone.

Native acceptance now passes for both Queen and an ordinary sleeping requester;
see [94](94-clarification-native-acceptance.md) for exact decision/round/session
IDs and evidence. Browser question -> guarded native delivery -> actual MCP reply
-> browser attention -> explicit final answer was exercised, not simulated.
The sleeping author woke without a manual start and replied in14 seconds.
The ordinary Queen reply took6 seconds and survived browser reload. Dark390px
layout and keyboard final answers passed; this is not real-phone acceptance.
Native testing found and corrected a trusted-local session handoff defect: the
existing local restore now issues the existing browser cookie, while the stricter
first-party command still rejects bare local/worker-token requests. All34 matching
auth tests, strict API Clippy and the full1512-test web suite passed. Production
still414de057. Next: commit this correction/evidence, final candidate check, deploy
coherently and verify the live Hive. No release. Preserve older-provider tool-list
refresh limitations per ADR0053 rather than restarting unrelated workers.

Sleeping-author admission is now implemented through the existing bounded worker
return queue in the same transaction as the operator question. Running authors
get no return intent. An existing failed/unconfirmed attempt is preserved, not
reset; explicit stand-down and HTTP replay cannot resurrect a cancelled wake.
Question/event failure rolls back the wake as well. The existing supervisor owns
startup, resource/drain/provider guards and failure attention; no separate sender
or retry loop. All 25 clarification persistence tests and strict persistence
all-target Clippy pass. Native provider/browser roundtrip is still required.
Upstream and the clean live clone rechecked at414de057; /health is healthy with
version1.6.0-dev-414de0575d76-20260909232601-3621464. Root disk has3.4GB free.
Use CARGO_INCREMENTAL=0 for the isolated Rust checks after the compiler cache ICE.
Account usage is26% used/74% remaining at this package boundary; no resets used.
No changes deployed, worker restarts, releases or BFG Admin contact in this slice.

Queen attention now also includes a bounded explanation-wait projection independent
of task membership and primary task owner. It includes no-task and mixed-requester
decisions, actual requester roles, exact question/decision IDs and delivery state;
64 details plus an exact total and truncation flag, with no question/reply text.
The Queen-only coordination tool directs her to read/reply to the exact exchange,
not infer permission. Task assignment and existing ownership counts do not change.
Two persistence tests, the application authorization regression and the API Queen
attention test pass. Strict API/application/persistence all-target Clippy passes.
The first API compile hit a Rust incremental fingerprint panic; the same test
passed with CARGO_INCREMENTAL=0, without live-service changes or cache deletion.
This closes the missing compact attention projection, NOT native Queen behavior
acceptance. The next gate remains sleeping-requester delivery/current-tool readiness,
then fictional native roundtrip and coherent deployment. No release or BFG contact.

Explicit delivery recovery is now connected locally through the operator-only
HTTP route and real Inbox panel. The exact observed claim/session fences each
choice: confirm checked delivery, or retry after acknowledging duplicate risk.
Eight immutable audit records per question cap recovery at32,768 Hive-wide;
question retention cascades them. Replaying an old choice cannot requeue a newer
claim; conflicting choices, stale sessions and resolved parents are refused.
An audit-insert failure rolls back the retry state. History exposes claim/session
identities, not credentials. No automatic resend loop or task approval is added.
Strict domain/persistence/API all-target Clippy passed. The HTTP/MCP integration
test verifies worker and absent credentials are rejected on recovery, while the
operator confirmation leaves the decision pending. All20 clarification tests and
four previous-schema tests passed; the added rollback test then passed with the
three focused recovery tests and strict persistence lint. Fifty-four targeted
Inbox/panel/recovery tests, TypeScript and production build passed; the final
question-before-controls layout also passes the seven panel/recovery tests.
In Edge, a fictional question -> uncertain delivery -> explicit consent -> retry
returned to waiting without changing final-answer choices. Dark390x844 rendered
without horizontal overflow; viewport reset. This is not native worker delivery
acceptance and is still UNDEPLOYED. Schema159 remains unreleased.
Next: mixed-requester Queen attention, sleeping/current-tool-surface readiness,
then the native demo round trip and coherent deployment. Do not cut a release.

Checkpoint a17de76e commits the first queue projection increment after 55de9ad2's
inbox/notification work; both remain undeployed. The queue change adds exact clarification requesters to the shared
task projection, including linked decisions. An unanswered explanation changes
attention ownership only when no other linked decision still needs an operator
answer. Task state, assignment and pending-decision permission gates stay intact.
Queues groups by the actual explanation requester(s), not the task assignee;
multiple requesters share one row and a combined roster-ordered heading.
All-Queen explanations use the Queen section; mixed requesters use Workers with
every requester named. Queen's handling of mixed-requester explanation waits
still needs integration verification, not an assumption from these owner counts.
Five domain tests, 13 shared-decision tests and 54 queue UI tests pass. Strict
all-target Clippy for domain, persistence and API passes after test-only clone
cleanup. TypeScript also passes. This queue increment is not yet browser-accepted
or deployed; preserve the remaining mixed-owner coordinator and native gates.
Active tasks with an outstanding clarification now remain in the waiting view:
the executing worker keeps task ownership, while each explanation names its own
requester. After the reply, ordinary active work is minimized again. Assignee
prompt hints are suppressed when the current queue move is a clarification from
another requester. The 54 queue tests pass with these regressions. A separate
Edge tab at localhost:5211/harness.html?surface=queues-clarification verifies the
actual component with fictional shared/active waits at desktop and 390x844;
scrollWidth equals 390 and text wraps without clipping. Viewport reset afterward.
This is layout evidence, not native phone or live Hive delivery acceptance.
No deployment, engine restart, release or BFG Admin contact has occurred.

Current local integration: the real DecisionInbox now renders Ask a question,
waiting status, replies and original final controls. App and inbox share the
actionable-decision predicate; waiting questions stay visible but are not counted
as operator actions. History has one owned/cancellable 15-second read and at most
eight cached conversations of at most 32 rounds; only the inspected conversation
refreshes when its summary changes. No per-card polling. Failed sends preserve
exact text/ID, and a local send receipt cannot hide a later reply indefinitely.
All 1508 web tests (156 files), TypeScript and production build passed. The
existing 552-KB terminal chunk warning remains. Edge testing of the actual Inbox
component in fictional local state verified 1 -> 0 -> 1 actionable count,
Queen reply attribution, dark 390x844 layout with scrollWidth=clientWidth=390,
failed-send exact draft recovery/retry and free-form final answer while waiting.
Temporary viewport was reset. This is not native Android/iOS or Hive delivery
acceptance. New Edge binding is edgeNow (browser 6); the old browser 4 is gone.

Server notification integration now passes its functional checks: shared next-move projection
suppresses operator pushes while waiting, and a new reply-round subject key
preserves the original decision FK without deduplicating away later replies.
Schema 159 is still UNDEPLOYED; its candidate now includes explicit round_index
so random browser IDs and same-second timestamps cannot misorder history/replies.
All 18 clarification tests, 10 notification tests and four previous-schema
checks pass in the isolated Linux checkout. Clarification reply receipts survive
queue acknowledgement, are unique per round/device, and cascade away with either
owner (at most4096 rounds *8 subscriptions). The tests cover two reply cycles
between sweeps with equal timestamps and deliberately reverse-sorting IDs.
Strict persistence all-target Clippy and the HTTP-to-worker-reply integration
test passed after the test helper cleanup. API all-target Clippy also passed.
Ordinary non-clarification notification completion also deletes its queue row;
the new receipt mechanism is scoped to clarification, not a claim that all
notification sources now have durable sent receipts. Retain that broader check
for the remaining maturity handoff rather than expanding this package.
Queues ownership, explicit uncertain reconciliation, sleeping-requester/current
tool-surface handling and native demo-worker round trip are still required.

### Timestamped resource observation requested during this work

2026-09-09 22:45 Eastern (2026-09-10 02:45 UTC): operator reported Swarm at 6-8%
in Edge Task Manager with fan noise, already improving. A 2.10-second Windows
process sample from 02:45:03.577Z to 02:45:05.679Z on 22 logical processors found
Edge total 0.37% whole-machine CPU (~8.14% of one logical CPU), largest Edge PID
10712 at 0.34% whole-machine CPU / 519.5 MB working set. This is all Edge, not a
verified Swarm tab attribution. Node sampled 0%. The full web suite had completed
at about 22:42:34 Eastern (started 22:41:38, duration55.64s); do not attribute the
later sample to still-running tests. Server snapshot at02:45:46Z: uptime7d9h08m,
load1.52/2.28/3.50, memory19468MiB available of32042MiB, swap729MiB used. ps reported
swarm-api PID3624356 lifetime-average CPU13.2% /RSS118928KiB and terminal-host
PID3408834 CPU5.1% /RSS337580KiB; those ps percentages are NOT an interval sample.
No cause or performance fix is established by this brief observation. No restart
or deployment was made. Continue the selected UX package, not a profiling detour.

Operator screenshot at22:49 Eastern (02:49 UTC) supplies app-specific Edge Task
Manager attribution: App:(1)Swarm CPU5.9%, memory342388K, network249KB/s, PID29400.
This is a separate measurement from the earlier all-Edge sample; do not assign
the earlier top PID10712 to Swarm. Source: operator attachment
codex-clipboard-b3ac8dee-33c8-4c38-b812-970a09fe8e42.png. No root cause inferred.

Prior committed checkpoint53f3fd6: the compact clarification summary reaches the real
operator inbox HTTP response. A single SQLite read transaction combines the
existing bounded/scoped decision projection with a text-free batch summary over
the bounded clarification table. It reports round count, outstanding round and
delivery state, latest reply time, and the domain-owned next mover. Original
decision fields remain flattened and unchanged; asking/replying does not resolve
permission. TypeScript accepts the additive response contract. All 17 isolated
Linux clarification persistence tests and the HTTP/MCP reply-to-inbox test pass,
including uncertain delivery, late replies and original pending state. This is
local candidate work, not deployed, and at that checkpoint not yet used by rendered counts.

Continue directly with the actual Inbox panel and bounded history cache, shared
actionable counts/Queues, and notification reply cycles. Notification delivery
currently deduplicates by decision subject identity: changing only its timestamp
is insufficient if an earlier push is already recorded. Preserve the original
decision FK while representing a new reply cycle; test suppression while waiting
and return after reply, including a question/reply between coordinator passes.
Do not remove pending-decision execution gates to change presentation ownership.
Explicit uncertain-delivery reconciliation, sleeping-requester/old-tool-surface
handling and the native demo-worker round trip remain required before deployment.
Latest verified account usage: 25% used / 75% remaining. No BFG Admin messages or releases.

The next user-facing closure is the operator's inability to ask about an unclear
decision without giving a final answer. ADR 0094 defines the approved distinction.
Do not detour into Apiary polish, engine profiling or BFG Admin coordination while
this path is being connected. This is the bounded backend exception already
allowed for the selected UI-1 journey below.

Implementation sequence:

1. Domain admission/next-move rules and transactional exact-ID question/reply
   history. Current local schema 159 adds a bounded exchange. All 150 domain
   tests passed. The full persistence run had 720 passes and one migration
   fixture failure; adding schema 159 to the recorded migration steps corrected
   it. The follow-up isolated Linux run passes all 9 clarification tests, all 4
   previous-schema tests, and the declared-schema-ceiling test. This includes
   migration/reopen and proof that clarification cannot grant command approval.
   The foundation is now connected to local HTTP/MCP candidate code below,
   but has not been deployed or integrated into the production inbox.
2. Exclusive guarded delivery with claim/session fencing, interrupted-claim
   uncertainty, explicit reconciliation, and cancellation on final answer or
   withdrawal. Never reuse the final-answer outbox to disguise a question.
3. Authenticated operator question and requester/Queen reply APIs/MCP, compact
   inbox summary and bounded per-decision history. Shared next-move projection
   must move pending clarification out of actionable operator counts without
   hiding the original decision or its final-answer controls.
4. Inline Ask a question, failed-send draft recovery, Waiting for a reply and
   returned explanation. Verify fictional round trip, desktop/phone, refresh,
   repeated/conflicting sends and parent resolution races; only then deploy.

Current checkpoint (not a user-facing closure): schema/domain/history and the
shared application commands are implemented locally. Delivery persistence now
claims at most 16 local-Hive questions, checks exact claim/session/requester and
engagement before submission, and fences acknowledgements against stale claims.
Interrupted writes become uncertain without automatic resend. Original button,
free-text and withdrawal paths cancel queued questions in the same transaction;
in-flight claims retain transport evidence. An exact reply retry from the same
worker after restart preserves its original reply source and cannot answer the
next question. Authentication rejects stale sessions and fabricated Queen roles.

Isolated Linux verification passes: 16 clarification persistence tests, 4 domain
tests, 1 application-service test, strict all-target Clippy for those three
crates, and all 35 existing decision tests. The earlier full domain run and
corrected migration checks above remain recorded; this is not a new full-suite
or browser claim. Latest usage check: 22% used / 78% remaining.

The current candidate increment connects guarded transport to the existing
exclusive coordinator. Interrupted clarification claims are recovered only
under that owner's lock; rejection/ambiguous writes never silently resend.
Same-terminal delivery remains serial and different terminals can progress
independently. The existing supervisor gains a coalescing wake-up signal for
operator questions rather than a new sender task, queue or polling loop. A
question returns its durable receipt without waiting for terminal submission.

Operator-only HTTP question/history routes and requester/Queen MCP history/reply
tools are implemented; tool revision 24 and its served-schema fingerprint agree.
The fictional HTTP -> worker-tool reply -> HTTP history scenario passes,
including rejected worker credentials on the operator route, exact retry,
conflicting retry, unchanged original decision and later explicit resolution.
Three API clarification tests, the wake-up/coalescing test and tool-discovery
test pass. All four owned-background-service checks also pass, including the
fictional real-PTY paste/shutdown/Enter test. Strict all-target API Clippy passes.
This does not yet prove delivery through a real provider session or prompt a
real worker.

The standalone question/reply panel and TypeScript clients are also implemented.
It has no resolution callback; failed sends retain exact text and retry identity.
Five component tests, all 36 style tests, TypeScript and the production web build
pass. The final focus-only adjustment passes its five component tests and
TypeScript again: completion focuses its receipt only when the operator has not
moved to another decision. Edge verifies the local fictional desktop and 390x844 iframe journeys:
ask without answering, failed-send recovery with original question intact, Queen
reply attribution, dark phone layout, and an original final choice while a new
question is still waiting. These are fixtures, not native Android/iOS acceptance
or a completed production inbox integration. The existing terminal chunk-size
warning remains; reload/performance work is not the current priority.

Next, finish explicit uncertain-claim reconciliation and the shared compact
Needs You/Queues/notification summary, then wire the panel into the actual inbox
and verify a full isolated demo-worker round trip. Check sleeping-requester
handling and old worker tool surfaces as part of that integration; do not leave
questions silently waiting on a worker that cannot receive or answer them.
Do not deploy this incomplete candidate. No real Hive decision, terminal or
worker was touched for these fictional tests. No BFG Admin communication is
needed or authorized.

Isolated Linux verification checkout: `/tmp/swarm-clarification-check.LS0pgn`.
Tests use fictional databases. The actual Hive remains on 414de057; do not apply
an unfinished schema to it merely to call this work deployed. Existing user
workers and all real decisions remain untouched. Direct terminal/AskUser answer
reconciliation is still the separate ADR 0065 gate, not solved by clarification.

### UI-4: saved-session connection recovery

The ordinary-user first/repeat-entry audit found that a failed session or initial
board read opened the token form even when authentication had not been rejected.
The browser now distinguishes a failed check from an actual HTTP 401. A failed
check offers Try reconnecting using the existing session, with technical details
collapsed; only confirmed missing/expired authentication opens the unlock form.
Retries use the existing bounded recovery policy and have no worker commands,
new timers, cookie/storage changes or automatic sign-in bypass.

Edge's isolated fictional journey verifies repeated failure, restored service
and successful entry without credentials. The 390px phone view is readable;
unlock/recovery buttons and details have 44px minimum targets. Tests cover both
session-check and snapshot failure, recovery, and retry discovering an expired
session; a message containing 4010 is not mistaken for authentication failure.
All 154 web files / 1,497 tests and TypeScript pass. After the final touch-target
adjustment, all 36 style checks and the production build pass again. Edge also
verified expanded technical details in 390px dark mode and phone recovery into
the app without a token field.

Deployed `414de057` as `1.6.0-dev-414de0575d76-20260909232601-3621464`.
Health is good with no degraded services. Engine PID/start and all 34 worker
identity projections, including 12 running, are identical before and after in
`/tmp/swarm-session-recovery-deploy.E3bxLd`. CI 34416874473 passed completely;
prior dialog CI 34414805240 passed. This does not close native-device,
clarification or engine gates. Next UX finish work: integrated Apiary/navigation
and non-authorizing decision clarification, not reload profiling or BFG messaging.

### UI-4 closed defect: modal navigation bypass

Edge reproduced a dirty task editor at 390px leaving the top navigation exposed.
Clicking Settings removed the editor without an unsaved-changes prompt. The
fixed-position backdrop was constrained by `.workspace { contain: layout paint }`.
Do not remove that terminal-rendering protection: place blocking overlays at the
document body through a shared ModalPortal, keeping their existing focus, draft,
save, and cancellation owners. Apply the same placement to task/prerequisite,
feedback, migration, broadcast, handoff, command, and update/release-note dialogs.
Shell and image viewers already use document portals and remain unchanged.

Local Edge now shows the task dialog filling the 390px phone frame, with the
navigation covered and the changed title retained through Keep editing. Desktop
DOM measurements show backdrop (0,0,1465,1339), matching the viewport exactly;
the hit-test at the navigation corner returns the backdrop, not navigation.
New regression checks require task/prerequisite overlays outside a contained
workspace and verify task-overlay cleanup. All 154 web test files / 1,494 tests,
TypeScript and the production web build pass. The existing feedback-primary-action
test now queries the whole portalled dialog rather than its old mount container;
the prerequisite rerender tests retain their original component identity. No
worker, save, cancellation or send policy changed.

Deployment completed: `d090bbb9`, live
`1.6.0-dev-d090bbb956bf-20260909225909-3569261`, healthy with no degraded
services. All 34 worker identity projections (12 running) and the engine PID/start
match before/after in `/tmp/swarm-modal-boundary-deploy.eJmFD3`. CI 34414805240
passed completely, as did prior 34412315650. Quick-navigation overlay
coverage was also measured at the full 1465x1339 fixture viewport. The operator
confirmed the Android PWA task-editor check with "Looks good": full-screen
coverage and Close -> Keep editing preserving the unsaved change. This closes
the Android confirmation for this dialog fix alongside desktop/390px fixture
proof; it does not close unrelated native picker, presence or iOS gates.

Operator-directed priority change: September 9, 2026.

**Design acceptance principle:** developer users should get the right information
at the point of action, with clear, logical, easy-to-use controls. Keep Swarm's
adorable Hive identity through lightweight visual personality, without obscuring
technical meaning or adding expensive effects. Judge increments by improved
understanding and task completion, not visual novelty. The operator explicitly
endorsed this direction; ordinary-user improvements remain ahead of reload-only
developer inconveniences.

Worker setup follow-through: the roster now exposes a named accessible group
for each worker, keeping its Edit/reorder/editor controls associated with that
identity without changing the compact visible layout or bee artwork. The real
Edge fixture verified opening Field Notes and cancelling back to its same group.
All 25 WorkerSettings tests pass, including group identity and cancellation;
TypeScript and web build pass. Together with a17d7b72 this is queued locally.
CI 34408932476 exposed one missed Settings integration assertion from the prior
diagnostics change: its epoch-second-1 sample must say Last known metrics, not
Live metrics. Update that assertion, retain it after refresh, and run the full
web suite before publishing. This is not a runtime regression or a full-program
completion claim. Verification now passes: all 154 web test files / 1,488 tests,
plus the earlier TypeScript and production build. Phone-width Edge inspection
retains readable worker names, bees, and separate edit/reorder controls. CI's
prior run has completed (web failed on that old assertion); publish this
corrected batch and verify the app-only deployment next.

Deployment verified: `55e51327`, live
`1.6.0-dev-55e513276534-20260909221157-3486945`, healthy with no degraded
services. All 34 worker identity projections (12 running) and engine PID/start
match exactly before/after; evidence `/tmp/swarm-worker-ux-deploy.4BdrNM`.
Provider conversation IDs are not exposed by that projection. CI 34410878270 is
running; do not cancel it by immediately pushing the next increment.

Next queued visible fix: the phone worker editor inherited roster-summary
ellipsis styling, clipping bee guidance and other small explanatory text.
Scope wrapping to editor small text only, preserving compact closed rows.
Edge 390px phone fixture shows the entire bee explanation after the fix;
61 style/worker tests and production web build pass. Queue for the next coherent
publication after current CI; continue UI-3/4 everyday journeys meanwhile.

**Latest verified deployment:** `85c2efc7`, live
`1.6.0-dev-85c2efc7cb45-20260909222759-3509505`. The worker-editor wrapping
and focus-return fixes are now deployed, not pending. Health is good with no
degraded services; all 34 worker identity projections (12 running) and the
engine PID/start match exactly in `/tmp/swarm-worker-editor-deploy.OPnGkT`.
Prior CI 34410878270 passed completely. Focused worker/style tests: 64 passed;
support/Night Watch: 15 passed; TypeScript and production web build passed.
Edge full-App task search also verified a no-match explanation and Show all
open work restoring the existing board. No real task or worker was edited.
Next: continue UI-4 task/notification navigation and empty/error-state coverage;
preserve the explicit native-device and backend gaps in the matrix below.

**Latest operator direction, evening:** stop the reload investigation. Prioritize
visible improvements for ordinary users with one to five workers; incremental
updates are welcome. Finish the already-running search-layout deployment, then
work through Settings/navigation, mobile controls/attachments, readable messages
and integrated layout consistency. Do not substitute a deeper reconciliation or
performance project for this user-facing pass. Those unfinished requirements
stay recorded, not silently dropped or represented as completed. This sequencing
supersedes the older reload-first pointers below and in the execution log.

Search-layout deployment completed: live version
`1.6.0-dev-f23bb3cba88c-20260909210727-3415103`, healthy with no degraded
services. The operator's engine update finished first (PID 3408834, start
17:01:49 EDT); this app-only deployment preserved that exact engine PID/start
and all 34 worker projections, including twelve running session/provider
identities. Evidence: `/tmp/swarm-terminal-search-deploy.zi0a90`.
No provider conversation-ID proof is implied by the worker-list projection.
Next rendered surface: Settings/navigation for everyday users, not reload work.

### Everyday Settings search increment

Live Edge reproduced a discovery defect: `night watch` showed Queen policy but
not the actual schedule. Search now indexes the visible presence/schedule,
opening-screen and notification labels, and matches all query words across one
card's existing title/section/keywords. `night watch schedule`, `dark theme`,
`opening screen` and `phone notifications` find their expected controls. Unknown
combinations stay empty; developer cards remain hidden outside development mode.
No fuzzy-search service, new polling or backend changes were introduced.

The phone fixture exposed duplicate native/custom clear buttons. Hide the native
clear affordance as the worker search already does; give Settings search and its
single labelled clear action 44px mobile targets. Full-App fictional desktop and
390x844 phone checks cover schedule discovery, light/dark appearance, notification
search, no-match state, clear and Escape recovery. The phone fixture is not a
claim about physical-device keyboard behavior. Focused tests: 11 navigation and
25 SettingsWorkspace tests pass; TypeScript and production web build pass.
This increment is local pending commit/deployment verification. Previous search-
layout CI 34405066863 remains in progress; do not cancel it just to publish notes.

### Clear restart consequences

The full-App Updates fixture showed two running sessions but the unavailable-
engine confirmation said `Restart 0 workers now?`. The confirmation now explicitly
says all loaded workers when the count is unknown and warns that running work
can still be interrupted. Confirmed zero and positive counts remain distinct.
Recovery wording describes an attempt, not a guarantee of restored conversation.
The execution path is unchanged. Desktop/phone fixture inspection covers the
warning and Not now cancellation; no live worker restart was requested.
SettingsWorkspace now has 28 passing tests, including unknown/zero/two-worker
counts and cancellation without a callback. TypeScript and web build pass.
Prior CI 34405066863 has now passed completely; the queued Settings changes can
be published without cancelling it.

Settings increments are deployed as `a716dad8`, live
`1.6.0-dev-a716dad84cea-20260909212842-3442412`. Health is good and all 34 worker
identity projections (twelve running) plus engine PID/start match the before
snapshot. Evidence: `/tmp/swarm-settings-ux-deploy.fzEfrZ`. Current CI is
34407100626, in progress; prior f23bb3cb CI passed. The phone support-form
review/cancel check remains unverified after browser-control timeouts and fields
not retaining automated fills; this is not established as a product defect.
No support submission or worker restart was performed. Continue ordinary-user
UI-3/4 journey checks, not reload profiling or BFG Admin coordination.

### Support draft-state clarity

The support form previously promised an unsent retry copy even before it had
saved one, while its discard confirmation correctly said the draft was unsaved.
The privacy note now distinguishes an unsaved draft (reload may lose it), a
retained exact retry, and a confirmed Hive save (the obsolete retry note clears).
No persistence or sending behavior changed. Fourteen support/file tests pass,
including those three states, and TypeScript/production build pass.
In Edge's isolated support fixture, closing an unsaved draft asks for confirmation
and Escape returns to the intact form. Existing fictional retained-file setup
shows the exact two attachments; its first simulated save failure preserves the
retry, and the second reaches the fictional Hive receipt. No external send or
BFG Admin communication occurred. Initial email-field automation and real phone
picker/keyboard acceptance remain separate, not proven by this seeded retry.
This increment is local pending publication; a716dad8 CI is still in progress.

### Integrated Settings evidence checkpoint

The diagnostics fixture exposed a stale sample headed `Live metrics`, above a
correct stale-evidence warning. Its heading/indicator now reuse the existing
performance assessment: fresh, last-known (stale or clock mismatch), or unavailable.
Old machine readings are labelled `Last sample`; no monitoring or pressure rules
changed. Edge verifies the stale heading, old machine reading and evidence agree.
Twenty-five diagnostics/assessment tests pass, covering fresh/stale/future/missing
samples. TypeScript and production build pass. Prior a716dad8 CI has now passed.

| Ordinary-user journey | Current evidence | Remaining gate |
| --- | --- | --- |
| Settings discovery and search recovery | Desktop/390px light/dark; schedule/theme/notification searches, empty result, clear/Escape; deployed a716dad8 | None for this increment |
| Restart decision and cancellation | Unknown/zero/positive count tests; desktop/phone warning and Not now; deployed a716dad8 | Does not prove actual engine recovery |
| Support draft and retry | Discard/Escape; fictional attachment review/failure/retry/receipt; full Edge text entry -> review -> edit -> review -> saved-pending receipt now verified | Native picker and end-to-end Admin delivery remain; fixture is not an external send |
| Night Watch form | 3 load/save/recovery tests; Edge changed timezone to UTC and showed Schedule saved with UTC still visible | Fixture does not validate server timezone rules or physical desktop/phone return behavior; empty-string browser fill remains inconclusive |
| Diagnostics preview | Visible fixture preview, distinct browser/server evidence; stale-state correction deployed 51cadc40; Settings integration assertion corrected in 55e51327 | No new presentation gap identified by these checks; long-term measurement acceptance remains separate |
| Worker editor keyboard return | Edge reproduced focus loss to BODY on cancel; fix returns focus to the same worker's Edit button. Tests cover cancel, discard, failed save and retry; 64 style/worker tests pass; deployed 85c2efc7, full CI passed | No live worker editing performed; fictional journey complete |
| Blocking task dialogs | Document-level overlay covers navigation; desktop and 390px draft/cancel proof; deployed d090bbb9, full CI passed; Android operator accepted "Looks good" | This dialog gate closed; not an iOS claim |
| Saved-session entry failure | Failed check -> repeated retry -> recovered connection opens app without a token; actual 401 still opens unlock; desktop/390px light/dark; deployed 414de057 | Current CI pending; not a physical network-suspension test |

This is an incremental evidence matrix, not closure of UI-3, UI-4 or the overall
goal. Next focus is ordinary-user worker/navigation/dialog usability. Do not
resume reload profiling or contact BFG Admin. Automated schedule saves in this
pass were confined to fictional responses and did not change the live schedule.

Clarity deployment verified: `51cadc40`, live
`1.6.0-dev-51cadc405a76-20260909214905-3459418`, healthy. Engine PID/start and
worker identity projections match before/after snapshots in
`/tmp/swarm-clarity-ux-deploy.lTVzMZ`. No engine activation or release occurred.

### Worker setup empty-state guidance

The roster fixture with no discovered repositories incorrectly said every
repository already had a worker and advertised a future settings location.
The message now distinguishes empty discovery from all discovered repositories
being assigned; both give the supported next step of entering an existing full
project path. The unmatched suggestion also points to that path and the existing
folder warning. No admission, folder-security or provider policy changed.
Edge confirms the empty-discovery guidance. All 24 WorkerSettings tests pass,
including both empty states and preservation of the explicit folder opt-in.
This increment remains local pending publication/deployment verification.

### September 9 evening checkpoint — local terminal layout correction

The operator is applying a worker-engine update. Do not overlap it with another
deployment; re-observe the completed update and use its engine/session identities
as the new baseline before any later app-only deployment.

Fixed a separately reproduced terminal-search layout defect: the stage was a
row flex container, putting search beside the output and squeezing its width.
It now stacks search above a flexible output mount without a competing 100%
height. The handoff fixture now constrains height like the application.
Edge verification: at desktop width 1433px, opening search preserves terminal
width and stage height (1157.56px); the 52px search row reduces mount height
from 1157.56px to 1105.56px. The grid settles from 1120px to 1072px high.
Search failure displays No match, Escape closes search and restores input focus.
The 390x844 fixture keeps search, No match, Close and composer controls readable.
TerminalView's 29 tests, TypeScript and production Vite build pass. This change
is local only until a subsequent deployment is explicitly verified.

This does NOT close desktop reload jumping. The preceding live demo sample
showed about 5.2 seconds before the terminal appeared and about 6.9 seconds
before connected canonical geometry, but no repeated oscillation in that sample.
Source inspection shows browser-session restoration awaits the whole control-room
snapshot (including tasks, decisions and Jira links) before terminal selection.
That is a candidate startup bottleneck, not a measured attribution or a fix.
Next: attribute the startup delay and verify reload/switch stability, then return
to the separately open clarification/direct-worker decision reconciliation gate.

CI 34402184205 for f482b460 has now completed successfully. Its earlier pending
references below are historical; do not rerun that unchanged gate.

Resume checkpoint: UI-1 presentation `eb931114` and UI-2 controls `221e66cd`
are deployed with passing full CI. Runtime safeguard placement is `f482b460`;
its normal dev reload completed with evidence in `/tmp/swarm-ui3-deploy.mlsUrp`.
It is live as `1.6.0-dev-f482b460b146-20260909203810-3396484`, healthy, with
twelve exact running identities and engine PID/start time preserved. Its full CI
is still a separate pending gate; `221e66cd` passed full CI. Next substantive
UI work is the remaining desktop terminal reload/worker-switch stabilization,
then remaining Settings and the integrated finish pass. Do not repeat completed
card/control refinements or restart native-provider experiments. The direct-worker
decision reconciliation and clarification gap remains explicit and uncompleted.

This changes delivery order, not the requirements in [45](45-daily-driver-maturity-plan.md)
or the evidence standard in [92](92-maturity-acceptance-ledger.md). The overall
maturity goal remains open. A user-facing release candidate and completion of
the entire maturity program are distinct outcomes. The operator chooses and cuts
releases; this work does not authorize a release.

## Outcome and working rules

Use Codex's browser work for the visible, daily experience first. Optimize for
ordinary users with one to five terminal workers while preserving the power-user
tasking, Queen and Jira workflows. Keep Swarm's warm, cute, feminine-leaning bee
identity; personality stays visual, lightweight and reduced-motion friendly.

- Work on one coherent user-facing package at a time, through implementation,
  focused checks, browser verification, commit and safe dev deployment.
- Inspect the live surface before changing it. Existing fixes are not new work.
- Use a separate Edge tab at `swarm.bfgsolutions.net`; intrusive interactions
  use fictional local fixtures or the disposable demo workers only.
- Do not let deep engine/performance investigation delay every UI checkpoint.
  Fix backend defects only when they directly block the selected UI journey or
  threaten data/input/session safety; otherwise record an explicit handoff.
- Do not hide backend inconsistencies with optimistic labels, omitted warnings,
  fake progress or a client-side guess about whether work/permission is complete.
- No BFG Admin worker communication without separate operator approval. A handoff
  document is not a message or assignment to another worker.
- Reuse passing evidence; rerun only changed behavior and its affected contracts.
  One final integrated suite/checkpoint follows the packages, not every CSS edit.
- No new architecture experiments, dashboards or optional features merely to
  demonstrate progress. The optional return briefing stays mockup-gated.

## Current verified starting point

- Live App/API: `1.6.0-dev-7c648e4d88f0-20260909183740-3162396`, healthy.
- The deployment preserved all 34 worker records and twelve running session/
  provider identities; engine PID 2947820 remained unchanged. Conversation IDs
  were not available in that worker-list projection, so it is not independent
  proof of provider conversation continuity.
- Source main includes `71a85719`, a test-only correction after CI exposed a
  package-return fixture expecting an outdated engine to permit a return.
  Both corrected current/old-engine cases and strict API checks pass locally.
  Full CI run `34391815461` passed all four jobs. The next UI commit has its own gate.
- September 9 live Edge inspection shows eight Needs You requests. Every card
  has Say something else, but the page remains tall: repeated recommendation/
  action wording, long summaries and generous internal gaps dominate the first
  viewport. Preserve exact decision meaning while improving scanability.
- The live Queues surface already groups owners and workers. It must be finished,
  not rebuilt as though grouping does not exist. Waiting reasons, long titles,
  stalled/active distinctions and access to the next action remain acceptance work.
- The native Claude probe was stopped when priorities changed. One isolated
  interactive 2.1.266 session emitted SessionStart, UserPromptSubmit and Stop
  with empty background/cron arrays. That does not establish safe maintenance:
  background-work and hook-continuation cases were not completed. No real worker
  settings or automatic engine admission were enabled. Preserve
  `scripts/dogfood/native-signal-probe.cjs` as an explicitly non-authoritative,
  content-free diagnostic, not product behavior.

## Delivery order and exit gates

### UI-1 implementation checkpoint — September 9, afternoon

Implemented the first cohesive presentation batch: recommended answers appear
once, explicitly labelled on their action; unmatched advice remains separate;
case-sensitive authored actions cannot be incorrectly highlighted. Empty risk
dividers are removed, but real risks and exact commands stay ahead of actions.
Resolved history says Answered and treats prior advice as historical. Queue rows
show full titles and wait reasons on separate lines within the existing worker
groups. Task detail reuses board status labels and shows recorded next ownership.

Verified in Edge against isolated fixtures: desktop light/dark decision cards;
390-by-844 phone Needs You, custom text, Queues owner jumps and task detail;
quick/custom failed-send retry, exact multiline receipt and cleared attention
count; linked queue prerequisite opens the correct task, with consistent Assigned
status in its detail dialog. No real decision was answered or worker woken.
Focused coverage passes: 45 DecisionInbox, 2 inbox activity, 1 answer fixture,
44 queue and 8 task-detail tests. TypeScript and production web build pass.

Deployed `eb931114` as `1.6.0-dev-eb931114675e-20260909194140-3319797`.
All 34 worker identities and twelve running session/provider identities matched;
engine PID/start time were unchanged. Health is good. Live Edge: the same eight
880px-wide cards are 44–49px shorter without removing answer wording; all 34
queue task titles fit without horizontal clipping. Fixture refresh preserves an
unsent custom answer, and quick navigation focuses its exact card without losing
the draft. A subsequent late-response regression passes (46 inbox tests total).
CI `34396506702` completed successfully: web, linux-package, audit and Rust passed.
Evidence: `/tmp/swarm-ui1-deploy.N3wwzR`. No release or engine update requested.

### UI-1 next: clarification is not a final answer

The operator reported clarifying Needs You items directly with RCG Networks and
resuming work, while Needs You stayed unchanged. A live API check confirmed the
four krbtgt/A1b/A2b/licence requests still Pending, their linked tasks still
Blocked/next owner Operator, and RCG Networks running. This is server-side
reconciliation debt, not just a refresh defect or something the layout fixes close.

Required regression journey: an operator opens an unclear request, asks its
context-owning worker a question, gets clarification, and settles the issue there.
Returning to Needs You must show the authoritative outcome without another answer
or a manual refresh. A clarification alone is not a final decision. A final answer
requires verified operator provenance and exact request/scope correlation. If the
question has instead become obsolete, use explicit audited withdrawal with a
reason, not an invented operator approval. Preserve the original exchange in
history and invalidate the attention count and linked task/queue projections.
Resolving one request must not clear other requests merely because they share a
worker, project or similar wording. Resumed worker activity is never sufficient.

Cover pending clarification, lost/failed send with retained draft, idempotent
retry, a late response after resolution, and direct-worker resolution while the
inbox is open. Do not add an optimistic Done button or treat Say something else
as an informal question: it currently submits a final answer. The direct-answer
backend gap remains separately visible in the handoff; no new native-provider
experiment is authorized merely to make this UI checkpoint look complete.

The current `/decisions/{id}/resolution` action is final; Say something else
also resolves. Do not disguise clarification as a resolution. Finish a distinct
non-authorizing question/reply path attached to the original request, preserving
exact operator text, context and history. A clarification must not grant command
permission or make its request appear answered. Distinguish waiting for a reply
from waiting for an operator decision. Correlate a proven final operator answer
given in the worker to the original decision; resumed activity or a worker's
prose claim alone is not proof. Keep QUEEN-03 open until that provenance and
reconciliation work is verified. This directly blocks the selected UI journey,
so it is an allowed bounded backend exception, not a return to engine work.

UI-1 is therefore not closed. Native-device acceptance also remains separate.

### UI-1 — Needs You, Queues and task detail form one understandable workflow

Requirements: ATT-01, QUEUE-01, user-facing QUEEN-01/03, UX-01.

Remaining implementation and inspection:

1. Make decision cards compact and scannable: clear question, owner/project,
   why the operator is needed, Queen recommendation, immediate actions and
   always-available custom text. Disclose supporting history progressively.
   Do not silently rewrite permission scope or truncate the actionable question.
2. Preserve drafts on failed submission; prevent double send; distinguish saved,
   resolved and delivered states. Verify the same card/count/history updates
   after a confirmed response, including refresh and an overtaken event response.
3. Finish Queues within existing owner sections, then worker and actual task
   order. Each item explains the current hold and what resolves it. Keep moving
   work secondary; show unknown or stale evidence honestly.
4. Make linked task/decision/dependency details useful without a wall of text:
   current status/owner/next move first, concise evidence next, full history on
   demand. Preserve reassignment and the established /task workflow.
5. Keep notification links direct to the relevant Needs You request, with no
   unnecessary intermediate confirmation/navigation page.

Exit: rendered desktop and narrow-screen light/dark evidence; fictional quick
answer and custom-text answer, failed-send recovery, refresh, history and linked
queue/task navigation pass. No real customer/operator decision is answered by a
test. Backend-originated stale/duplicate decisions remain explicitly open if the
source state cannot yet reconcile; a visual improvement does not close QUEEN-03.

### UI-2 — Terminal shell, worker switching and mobile input/attachments

Requirements: TERM-01/02, MOB-01/02, relevant PERF-01/02, UX-01.

September 9 controls checkpoint: optional key-visibility storage can no longer
crash the composer or stop its toggle from working. Drafts explain connecting,
disconnected and viewing-only states without automatic sending. Recovery notices
span the full composer grid instead of falling into its Send-button column.
Terminal toolbars, attachment explanations and tools wrap at narrow widths;
arrow targets use actual 44px grid tracks. Normal 390px arrow/action layout remains
compact. No worker, terminal ownership, input protocol or provider behavior changed.

Focused verification: 32 composer, 29 TerminalView and four draft tests pass;
TypeScript and production build pass. Edge fixtures at 390px in light/dark show
three simultaneous notices, retained multiline text, failed-send recovery, exact
single retry, no send on reconnect/redraw, collapsed keys with accessible redraw,
and real TerminalView Resume Here preserving an unsent draft. Transport and
redraw callbacks in these fixtures are fictional; this is not a provider or
native-device acceptance. These checks do not close the reload-jump, attachment
picker, actual keyboard/suspension or cross-device geometry requirements below.
Deployed `221e66cd` as `1.6.0-dev-221e66cdec78-20260909201955-3383452`.
All 34 worker identities and twelve running session/provider identities match
before/after; engine PID 2947820 and its start time are unchanged. Health is good
and Edge loaded the new runtime. Evidence: `/tmp/swarm-ui2-deploy.G5GcQu`.
Full CI `34400256735` passed all four jobs. No engine update or release occurred.

1. Prioritize the reported desktop reload jump and visible restore latency.
   Reproduce using the actual terminal renderer, recorded geometry/ownership
   and output sequence. Fix browser lifecycle/measurement defects where proven;
   do not add sleeps, hide output or restart the provider to mask them.
2. Preserve configured roster order, chosen worker and newest output on return.
   Verify focus, sidebar density, quick navigation and the supported shortcut
   fallback. Do not steal terminal/provider keys or promise browser-reserved
   Ctrl+Tab behavior the platform will not deliver.
3. Finish Resume Here and reconnect presentation: understandable ownership,
   stable dimensions, useful progress/failure, preserved draft and no duplicate
   input. A background/minimized test tab is not operator engagement.
4. Consolidate mobile terminal controls: arrows, Enter, redraw and attach image;
   handle keyboard/viewport changes without covering actions or losing text.
5. Verify visible upload progress, failure/retry and automatic provider insertion
   after success. Do not introduce a second Insert step or worker-switch overhead.
   Shared artifact storage remains the source used by workers and Queen.

Exit: fictional renderer journeys for reload, fixed-order switching, output while
typing, reconnect/refusal and attachment success/failure; no missing or duplicate
bytes. Desktop browser acceptance is recorded separately from real Android/iOS
keyboard, picker, suspension and device handoff. Android AskUser questions 2/3
already have operator acceptance; do not spend another session reproving that
unchanged case. Missing native-device checks stay in the release-risk checklist.

### UI-3 — Settings, runtime and recovery feel like the same product

Requirements: ATT-02, DIAG-01, DOG-01 presentation, PRES-01, PROV-01, UX-01.

September 9 runtime checkpoint: known `wake_not_admitted` observations are now
projected into one collapsed runtime safeguard notice rather than a Needs You
card/count. The resource safety rules and coordinator recovery are unchanged.
Uncertain wakes and unknown hold kinds remain attention; genuine decisions keep
their own cards. A failed observation retains and labels the last-known holds;
authoritative recovery removes the notice without acknowledgement. Reasons are
deduplicated, and diagnostics opens the existing maintenance section.

Seventy-two focused tests pass, including full-App queue/runtime placement and
badge consistency, explicit failure/recovery, and unknown-kind preservation.
TypeScript passes. Edge full-App fictional desktop and 390px phone journeys show
one decision plus a separate runtime start pause, expandable reasons, and correct
diagnostics navigation. The production build passes. This runtime change is now
deployed as recorded at the top of this plan; its full CI remains pending. This
does not close the entire Settings/finish package.

1. Use the runtime area as the compact home for system state. Avoid duplicate
   warnings and conflicting attention counts; disappearance after recovery needs
   no operator acknowledgement. Critical unresolved actions remain accessible.
2. Keep browser/server/provider/network evidence distinct. Show what is measured,
   unknown, consequential and actionable. Do not present browser timing as Edge
   CPU or server process totals as browser measurements.
3. Finish Settings hierarchy, readable update/recovery/provider states and the
   separate Developer Dogfood unit using existing dev detection. Ordinary users
   should not face developer-level verbosity by default.
4. Verify At Hive/Reachable/Night Watch controls, scheduling, timezone copy and
   desktop/mobile behavior. Do not change the operator's live schedule for a test.
5. Keep update types and interruption consequences explicit. Do not enable the
   unfinished automatic engine-admission path to make its warning disappear.
6. Verify support form progress/errors/receipt, diagnostic preview and source
   context. BFG Admin remains the customer-send owner; closure of a Swarm task
   does not authorize a send. Use only fictional submissions.

Exit: complete browser journeys for settings navigation, quiet recovery, error
details, diagnostic preview, update confirmation/cancellation and fictional
support state; no unintended service change, provider promotion or customer send.

### UI-4 — Whole-product finish and release-candidate evidence

Requirements: UX-01/P6 and the UI portion of P7.

Review the above together with Workers, Tasks, Apiary, empty/loading/error/offline
states, first/repeat login, dialogs and notification navigation. Check typography,
spacing, contrast, action hierarchy, touch targets, zoom/narrow layout, keyboard
focus/escape, nested modals, screen-reader names and reduced motion. Preserve
identity; show a mock before a materially new unapproved composition.

Exit: one concise desktop/mobile journey matrix with build identity, screenshots,
changed-code tests and explicit device gaps; latest CI result; healthy dev build
and exact worker continuity for deployments. Prepare a short operator dogfood
checklist and a factual release-risk summary. Do not cut a release or call the
entire maturity goal complete from this UI checkpoint.

## Backend/performance handoff — remains in the overall scope

Suitable for a separate Claude/backend worker when the operator assigns it.
Do not send or delegate this document automatically.

| Remaining outcome | Current boundary and next necessary evidence |
| --- | --- |
| Safe automatic engine updates, including Queen (OPS-01) | Admission library and schemas 157/158 exist and return records are deployed. Native settled-turn/background proof, combined durable admission receipts, IPC/package activation, lost-response/partial-stop recovery and an actual safe update/return remain. ADR 0085 is authoritative; do not restart loaded workers from a Resting screen or one Stop hook. |
| Correct chosen-conversation recovery (REC-01) | Finish chosen-conversation switch and safe/resume -> native continue -> clearly identified fresh fallback, with real failure/cancellation evidence. Preserve explicit provider choice; no automatic switching. |
| Queen closes the actual orchestration loop (QUEEN-01/02) | Routine and same-task idle recovery demos passed. Finish Queen-owned Awaiting Release handoff, worker-first escalation, protected-input/background-work cases and unresolved real-blocker reconciliation. Do not manually shuffle tasks to claim the system works. |
| Proven direct answers and one operator question (QUEEN-03/ATT-01) | Composer source records exist; direct terminal/AskUser authored-answer capture and exact decision correlation remain incomplete. Prose claims are not authenticated permission. Link existing decisions only within their approved scope. |
| Sustained efficiency (PERF-01/02, DIAG-01) | Match fresh/aged 10–15-worker workloads; attribute server/engine/browser cost, fix measured tails/leaks, bound instrumentation and verify plateau/output fidelity. Existing passing microbenchmarks and short idle observations are not this acceptance. |
| Useful long-term metrics (DOG-01) | Finish evidence-backed review yield, duplicate-question, recovery/false-alert and update-convergence comparisons. Run counts do not measure useful task progress. |
| Provider/presence acceptance (PROV-01/PRES-01) | Real native-provider journeys, experimental exclusions, manual/scheduled Night Watch and device return behavior. Only the builder promotes providers. |
| BFG linked development/support loop | Text intake is accepted. Finish fictional attachment and linked-task completion/reply approval paths under existing contracts. Prevent duplicates/loops and expose retries. No BFG worker communication without approval and no customer send without Admin approval. |
| Integrated maturity acceptance (P7) | Current-build normal workday, overnight and real-device evidence plus operator acceptance remain. REC-02 isolated corruption/restore acceptance is retained; do not repeat live-database risk to generate activity. |

## Usage discipline and the approximately 5-percent handoff

Read account limits at package boundaries and before beginning expensive work,
not on a tight polling loop. Remaining percent is account-wide and is not a
guaranteed number of tokens or hours. No reset credits remain.

At approximately 10 percent remaining, stop starting new packages. Finish or
safely checkpoint the current change and prepare the handoff. At approximately
5 percent, stop implementation and use the reserve for a durable final record:

- completed versus implemented-only versus unverified versus blocked requirements;
- local/main/live App/API/engine identities, commits, CI, and deployment state;
- exact remaining defects, reproduction, expected behavior and next action;
- outstanding device/mock/operator approvals, with no invented approvals;
- partial changes and running tool/job handles, owners, output paths and cleanup;
- whether the build is a recommended release candidate and every known material
  exception. The operator makes the release decision.

Keep [92](92-maturity-acceptance-ledger.md) current as packages close so that the
5-percent handoff is consolidation rather than another repository-wide audit.
Unfinished requirements remain open in the original goal. Do not reduce the goal,
mark it achieved, purchase credits, or consume a reset to make accounting fit.
